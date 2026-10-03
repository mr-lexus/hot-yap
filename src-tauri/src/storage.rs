use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

/// Persist a small configuration file without exposing a partially written file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let temporary = sidecar_path(path, "tmp");
    let result = (|| {
        let mut file = File::create(&temporary)
            .map_err(|error| format!("cannot create {}: {error}", temporary.display()))?;
        file.write_all(contents)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush {}: {error}", temporary.display()))?;
        replace_file(&temporary, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(format!(".{}.{}.{}", std::process::id(), sequence, suffix));
    PathBuf::from(sidecar)
}

#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|error| {
        format!(
            "cannot replace {} with {}: {error}",
            destination.display(),
            source.display()
        )
    })
}

#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(source, destination)
            .map_err(|error| format!("cannot install {}: {error}", destination.display()));
    }

    // std::fs::rename cannot replace an existing file on Windows. Keep the
    // previous copy until the new one is in place so a failed replacement can
    // be rolled back instead of losing the user's configuration.
    let backup = sidecar_path(destination, "bak");
    fs::rename(destination, &backup).map_err(|error| {
        format!(
            "cannot prepare {} for replacement: {error}",
            destination.display()
        )
    })?;
    match fs::rename(source, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let restore_error = fs::rename(&backup, destination).err();
            Err(match restore_error {
                Some(restore) => format!(
                    "cannot replace {}: {error}; restoring the previous file also failed: {restore}",
                    destination.display()
                ),
                None => format!("cannot replace {}: {error}", destination.display()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_file_repeatedly() {
        let root = std::env::temp_dir().join(format!(
            "hotyap-storage-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
