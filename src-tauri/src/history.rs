use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const HISTORY_VERSION: u32 = 1;
const MAX_HISTORY_ENTRIES: usize = 1_000;
const MAX_TRANSCRIPT_CHARS: usize = 250_000;
static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    pub text: String,
    pub created_at: u64,
    pub favorite: bool,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub source_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    entries: Vec<HistoryEntry>,
}

pub struct HistoryStore {
    path: PathBuf,
    entries: Mutex<Vec<HistoryEntry>>,
}

impl HistoryStore {
    pub fn load(path: PathBuf) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str::<HistoryFile>(&contents) {
                Ok(file) if file.version == HISTORY_VERSION => normalize(file.entries),
                Ok(file) => {
                    log::warn!(
                        "ignoring unsupported transcription history version {}",
                        file.version
                    );
                    Vec::new()
                }
                Err(error) => {
                    log::warn!("cannot read transcription history: {error}");
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                log::warn!("cannot open transcription history: {error}");
                Vec::new()
            }
        };
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    pub fn list(&self) -> Vec<HistoryEntry> {
        self.lock().clone()
    }

    pub fn get(&self, id: &str) -> Option<HistoryEntry> {
        self.lock().iter().find(|entry| entry.id == id).cloned()
    }

    pub fn add(
        &self,
        text: String,
        provider: String,
        model: String,
        source_name: Option<String>,
    ) -> Result<HistoryEntry, String> {
        if text.trim().is_empty() {
            return Err("Cannot save an empty transcript".into());
        }
        if text.chars().count() > MAX_TRANSCRIPT_CHARS {
            return Err("Transcript is too large to add to history".into());
        }
        let created_at = now_millis();
        let entry = HistoryEntry {
            id: format!(
                "{created_at}-{}",
                NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed)
            ),
            text,
            created_at,
            favorite: false,
            provider,
            model,
            source_name,
        };
        self.update(|entries| {
            entries.insert(0, entry.clone());
            trim_to_limit(entries, MAX_HISTORY_ENTRIES);
            Ok(entry)
        })
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<HistoryEntry, String> {
        self.update(|entries| {
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| "History entry was not found".to_string())?;
            entry.favorite = favorite;
            Ok(entry.clone())
        })
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.update(|entries| {
            let previous_len = entries.len();
            entries.retain(|entry| entry.id != id);
            if entries.len() == previous_len {
                return Err("History entry was not found".into());
            }
            Ok(())
        })
    }

    pub fn clear(&self, keep_favorites: bool) -> Result<usize, String> {
        self.update(|entries| {
            let previous_len = entries.len();
            if keep_favorites {
                entries.retain(|entry| entry.favorite);
            } else {
                entries.clear();
            }
            Ok(previous_len - entries.len())
        })
    }

    fn update<T>(
        &self,
        operation: impl FnOnce(&mut Vec<HistoryEntry>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut entries = self.lock();
        let previous = entries.clone();
        let result = operation(&mut entries)?;
        if let Err(error) = persist(&self.path, &entries) {
            *entries = previous;
            return Err(error);
        }
        Ok(result)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<HistoryEntry>> {
        self.entries.lock().unwrap_or_else(|poisoned| {
            log::error!("transcription history mutex was poisoned; recovering the last value");
            self.entries.clear_poison();
            poisoned.into_inner()
        })
    }
}

fn normalize(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut ids = HashSet::new();
    let mut entries = entries
        .into_iter()
        .filter(|entry| {
            !entry.id.is_empty()
                && !entry.text.trim().is_empty()
                && entry.text.chars().count() <= MAX_TRANSCRIPT_CHARS
                && ids.insert(entry.id.clone())
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| Reverse(entry.created_at));
    trim_to_limit(&mut entries, MAX_HISTORY_ENTRIES);
    entries
}

fn trim_to_limit(entries: &mut Vec<HistoryEntry>, limit: usize) {
    while entries.len() > limit {
        let index = entries
            .iter()
            .rposition(|entry| !entry.favorite)
            .unwrap_or(entries.len() - 1);
        entries.remove(index);
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn persist(path: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    let contents = serde_json::to_vec_pretty(&HistoryFile {
        version: HISTORY_VERSION,
        entries: entries.to_vec(),
    })
    .map_err(|error| format!("Cannot encode transcription history: {error}"))?;
    crate::storage::write_atomic(path, &contents)
        .map_err(|error| format!("Cannot save transcription history: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    fn store() -> (PathBuf, HistoryStore) {
        let root = std::env::temp_dir().join(format!(
            "hotyap-history-test-{}-{}",
            std::process::id(),
            NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("history.json");
        (root, HistoryStore::load(path))
    }

    #[test]
    fn persists_favorites_and_deletions() {
        let (root, history) = store();
        let first = history
            .add("First".into(), "local".into(), "Tiny".into(), None)
            .unwrap();
        let second = history
            .add("Second".into(), "openai".into(), "STT".into(), None)
            .unwrap();

        history.set_favorite(&first.id, true).unwrap();
        history.delete(&second.id).unwrap();

        let reloaded = HistoryStore::load(root.join("history.json"));
        assert_eq!(
            reloaded.list(),
            vec![HistoryEntry {
                favorite: true,
                ..first
            }]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_history_written_before_source_names_were_added() {
        let (root, history) = store();
        drop(history);
        std::fs::write(
            root.join("history.json"),
            r#"{
                "version": 1,
                "entries": [{
                    "id": "legacy",
                    "text": "Old transcript",
                    "created_at": 42,
                    "favorite": false,
                    "provider": "local",
                    "model": "Base"
                }]
            }"#,
        )
        .unwrap();

        let reloaded = HistoryStore::load(root.join("history.json"));

        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].source_name, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_can_preserve_favorites() {
        let (root, history) = store();
        let favorite = history
            .add("Keep".into(), "local".into(), "Base".into(), None)
            .unwrap();
        history.set_favorite(&favorite.id, true).unwrap();
        history
            .add("Remove".into(), "local".into(), "Base".into(), None)
            .unwrap();

        assert_eq!(history.clear(true).unwrap(), 1);
        assert_eq!(history.list().len(), 1);
        assert!(history.list()[0].favorite);
        assert_eq!(history.clear(false).unwrap(), 1);
        assert!(history.list().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retention_prefers_favorites() {
        let entry = |id: &str, favorite: bool| HistoryEntry {
            id: id.into(),
            text: id.into(),
            created_at: 1,
            favorite,
            provider: "local".into(),
            model: "Base".into(),
            source_name: None,
        };
        let mut entries = vec![
            entry("new", false),
            entry("favorite", true),
            entry("old", false),
        ];

        trim_to_limit(&mut entries, 2);

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "favorite"]
        );
    }
}
