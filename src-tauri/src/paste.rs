//! Paste only after a global dictation, into the same foreground target.
//! No focus stealing, no Enter, and clipboard-only fallback on unsupported desktops.
use serde::Serialize;
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub window: String,
    pub focus: String,
}
#[derive(Serialize)]
pub struct PasteSupport {
    pub platform: String,
    pub available: bool,
    pub reason: String,
}

#[tauri::command]
pub fn paste_support() -> PasteSupport {
    let platform = std::env::consts::OS.to_string();
    let reason = if cfg!(target_os = "linux") {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland")
        {
            "wayland"
        } else if run("xdotool", &["--version"]).is_err() {
            "xdotool"
        } else {
            "x11"
        }
    } else if cfg!(target_os = "macos") {
        "accessibility"
    } else if cfg!(windows) {
        "windows"
    } else {
        "unsupported"
    };
    PasteSupport {
        platform,
        available: !["wayland", "xdotool", "unsupported"].contains(&reason),
        reason: reason.into(),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let result = child.wait_with_output().map_err(|e| e.to_string())?;
            return if result.status.success() {
                Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
            } else {
                Err("System paste permission or desktop access is unavailable".into())
            };
        }
        if start.elapsed().as_secs() >= 3 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("System paste request timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn run(_: &str, _: &[&str]) -> Result<String, String> {
    Err("Unsupported".into())
}

#[cfg(windows)]
pub fn capture() -> Option<Target> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_err() || info.hwndFocus.0.is_null() {
            return None;
        }
        Some(Target {
            window: format!("{}:{pid}", hwnd.0 as usize),
            focus: format!("{}", info.hwndFocus.0 as usize),
        })
    }
}
#[cfg(target_os = "macos")]
pub fn capture() -> Option<Target> {
    let value = run("osascript", &["-e", r#"tell application "System Events"
set frontProcess to first application process whose frontmost is true
set processID to unix id of frontProcess
tell frontProcess
set frontWindow to front window
set windowSignature to (name of frontWindow as text) & "|" & (position of frontWindow as text) & "|" & (size of frontWindow as text)
end tell
return (processID as text) & linefeed & windowSignature
end tell"#]).ok()?;
    let (process, signature) = value.split_once('\n')?;
    let pid: u32 = process.trim().parse().ok()?;
    (pid != std::process::id()).then(|| Target {
        window: pid.to_string(),
        focus: signature.to_string(),
    })
}
#[cfg(target_os = "linux")]
pub fn capture() -> Option<Target> {
    if !paste_support().available {
        return None;
    }
    let window = run("xdotool", &["getactivewindow"]).ok()?;
    let pid = run("xdotool", &["getwindowpid", &window]).ok()?;
    if pid.parse::<u32>().ok()? == std::process::id() {
        return None;
    }
    let focus = run("xdotool", &["getwindowfocus"]).ok()?;
    Some(Target { window, focus })
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn capture() -> Option<Target> {
    None
}

pub fn paste(target: &Target, terminal: bool) -> Result<(), String> {
    if !same_target(target, capture().as_ref()) {
        return Err(
            "Active window changed or cannot be verified. Text is in the clipboard.".into(),
        );
    }
    send(target, terminal)
}
fn same_target(expected: &Target, current: Option<&Target>) -> bool {
    !expected.window.is_empty() && current == Some(expected)
}
#[cfg(windows)]
fn send(_: &Target, terminal: bool) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    unsafe {
        if [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&key| GetAsyncKeyState(key.0 as i32) < 0)
        {
            return Err(
                "Release modifier keys and paste manually. Text is in the clipboard.".into(),
            );
        }
        let input = |key: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    ..Default::default()
                },
            },
        };
        let mut events = vec![input(VK_CONTROL, false)];
        if terminal {
            events.push(input(VK_SHIFT, false));
        }
        events.extend([input(VK_V, false), input(VK_V, true)]);
        if terminal {
            events.push(input(VK_SHIFT, true));
        }
        events.push(input(VK_CONTROL, true));
        if SendInput(&events, std::mem::size_of::<INPUT>() as i32) != events.len() as u32 {
            let release = [
                input(VK_V, true),
                input(VK_SHIFT, true),
                input(VK_CONTROL, true),
            ];
            SendInput(&release, std::mem::size_of::<INPUT>() as i32);
            return Err("Windows blocked paste (the target may run as administrator). Text is in the clipboard.".into());
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn send(target: &Target, _: bool) -> Result<(), String> {
    let pid: u32 = target.window.parse().map_err(|_| "Invalid paste target")?;
    // Recheck in the same system operation. Only our numeric PID enters the script.
    run("osascript", &["-e", &format!("tell application \"System Events\"\nif unix id of first application process whose frontmost is true is not {pid} then error \"Focus changed\"\nkey code 9 using command down\nend tell")]).map(|_| ())
}
#[cfg(target_os = "linux")]
fn send(_: &Target, terminal: bool) -> Result<(), String> {
    run(
        "xdotool",
        &["key", if terminal { "ctrl+shift+v" } else { "ctrl+v" }],
    )
    .map(|_| ())
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn send(_: &Target, _: bool) -> Result<(), String> {
    Err("Automatic paste is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_requires_the_original_window_and_control() {
        let original = Target {
            window: "10:100".into(),
            focus: "20".into(),
        };
        assert!(same_target(&original, Some(&original)));
        assert!(!same_target(&original, None));
        assert!(!same_target(
            &original,
            Some(&Target {
                window: "10:101".into(),
                focus: "20".into()
            })
        ));
        assert!(!same_target(
            &original,
            Some(&Target {
                window: "10:100".into(),
                focus: "21".into()
            })
        ));
    }
}
