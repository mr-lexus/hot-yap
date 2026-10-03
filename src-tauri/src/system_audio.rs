//! Output-only volume ownership. All platform handles live on one dedicated
//! thread (including its COM apartment). Dropping the recorder restores sound.
use crate::providers::SystemAudio;
use std::sync::mpsc;

pub struct OutputGuard {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OutputGuard {
    pub fn start(mode: SystemAudio) -> Result<Option<Self>, String> {
        if mode == SystemAudio::Nothing {
            return Ok(None);
        }
        let (stop, stopped) = mpsc::channel();
        let (ready, result) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("dictation-output".into())
            .spawn(move || {
                let change = platform::Controller::open(mode)
                    .and_then(|control| Change::apply(control, mode));
                match change {
                    Ok(change) => {
                        let _ = ready.send(Ok(()));
                        let _ = stopped.recv();
                        drop(change);
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let guard = Self {
            stop: Some(stop),
            thread: Some(thread),
        };
        result.recv().map_err(|e| e.to_string())??;
        Ok(Some(guard))
    }
}

impl Drop for OutputGuard {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

trait OutputControl {
    fn read(&self) -> Result<Vec<f32>, String>;
    fn write(&self, values: &[f32]) -> Result<(), String>;
}

struct Change<C: OutputControl> {
    control: C,
    original: Vec<f32>,
    applied: Vec<f32>,
}
fn unchanged(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.0001)
}
impl<C: OutputControl> Change<C> {
    fn apply(control: C, mode: SystemAudio) -> Result<Self, String> {
        let original = control.read()?;
        if original.is_empty() || original.iter().any(|v| !v.is_finite()) {
            return Err("Invalid output volume".into());
        }
        let desired: Vec<_> = original
            .iter()
            .map(|v| {
                if mode == SystemAudio::Mute {
                    1.0
                } else {
                    v * 0.2
                }
            })
            .collect();
        // A multi-channel write may fail halfway through: undo partial changes.
        if let Err(e) = control.write(&desired) {
            return Err(rollback(&control, &original, e));
        }
        match control.read() {
            Ok(applied) => Ok(Self {
                control,
                original,
                applied,
            }),
            Err(e) => Err(rollback(&control, &original, e)),
        }
    }
}
fn rollback<C: OutputControl>(control: &C, original: &[f32], error: String) -> String {
    match control.write(original) {
        Ok(()) => error,
        Err(restore) => {
            format!("{error}. Could not restore system sound; check the output volume: {restore}")
        }
    }
}
impl<C: OutputControl> Drop for Change<C> {
    fn drop(&mut self) {
        match self.control.read() {
            // Do not overwrite a volume/mute adjustment made by the user.
            Ok(current) if unchanged(&current, &self.applied) => {
                if let Err(e) = self.control.write(&self.original) {
                    log::warn!("Could not restore system audio: {e}");
                }
            }
            Err(e) => log::warn!("Could not access original output device to restore audio: {e}"),
            _ => {}
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    pub struct Controller {
        endpoint: IAudioEndpointVolume,
        mute: bool,
        _apartment: Apartment,
    }
    impl Controller {
        pub fn open(mode: SystemAudio) -> Result<Self, String> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .map_err(|e| e.to_string())?;
                let apartment = Apartment;
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| e.to_string())?;
                let device = enumerator
                    .GetDefaultAudioEndpoint(eRender, eMultimedia)
                    .map_err(|e| e.to_string())?;
                let endpoint = device
                    .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                    .map_err(|e| e.to_string())?;
                Ok(Self {
                    endpoint,
                    mute: mode == SystemAudio::Mute,
                    _apartment: apartment,
                })
            }
        }
    }
    impl OutputControl for Controller {
        fn read(&self) -> Result<Vec<f32>, String> {
            unsafe {
                if self.mute {
                    self.endpoint
                        .GetMute()
                        .map(|b| vec![if b.as_bool() { 1.0 } else { 0.0 }])
                } else {
                    self.endpoint.GetMasterVolumeLevelScalar().map(|v| vec![v])
                }
            }
            .map_err(|e| e.to_string())
        }
        fn write(&self, values: &[f32]) -> Result<(), String> {
            unsafe {
                if self.mute {
                    self.endpoint.SetMute(values[0] != 0.0, std::ptr::null())
                } else {
                    self.endpoint
                        .SetMasterVolumeLevelScalar(values[0], std::ptr::null())
                }
            }
            .map_err(|e| e.to_string())
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    pub struct Controller {
        sink: String,
        mute: bool,
    }
    // PulseAudio and PipeWire's PulseAudio compatibility server, on X11 or Wayland.
    // Resolve and retain the sink name so a default-device change cannot affect a new output.
    impl Controller {
        pub fn open(mode: SystemAudio) -> Result<Self, String> {
            let sink = run(&["get-default-sink"])?;
            if sink.is_empty() || sink.starts_with('-') {
                return Err("No default audio output".into());
            }
            Ok(Self {
                sink,
                mute: mode == SystemAudio::Mute,
            })
        }
    }
    impl OutputControl for Controller {
        fn read(&self) -> Result<Vec<f32>, String> {
            if self.mute {
                let value = run(&["get-sink-mute", &self.sink])?;
                return match value.trim() {
                    "Mute: yes" => Ok(vec![1.0]),
                    "Mute: no" => Ok(vec![0.0]),
                    _ => Err("Unknown output mute state".into()),
                };
            }
            parse_volumes(&run(&["get-sink-volume", &self.sink])?)
        }
        fn write(&self, values: &[f32]) -> Result<(), String> {
            let mut args = vec![
                if self.mute {
                    "set-sink-mute"
                } else {
                    "set-sink-volume"
                }
                .to_string(),
                self.sink.clone(),
            ];
            args.extend(values.iter().map(|v| v.round().to_string()));
            run(&args.iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ())
        }
    }
    fn run(args: &[&str]) -> Result<String, String> {
        use std::io::Read;
        let mut child = std::process::Command::new("pactl")
            .env("LC_ALL", "C")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| {
                format!("System audio control requires pactl and PulseAudio/PipeWire: {e}")
            })?;
        let stdout = child.stdout.take().ok_or("Cannot read pactl output")?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout
                .take(1024 * 1024)
                .read_to_string(&mut text)
                .map(|_| text)
        });
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let output = reader
                    .join()
                    .map_err(|_| "Audio helper failed")?
                    .map_err(|e| e.to_string())?;
                return if status.success() {
                    Ok(output.trim().into())
                } else {
                    Err("Cannot control the system audio output".into())
                };
            }
            if start.elapsed().as_secs() >= 3 {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err("System audio request timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    fn parse_volumes(text: &str) -> Result<Vec<f32>, String> {
        text.lines()
            .next()
            .unwrap_or_default()
            .split(',')
            .map(|channel| {
                channel
                    .split('/')
                    .next()
                    .and_then(|head| head.split_whitespace().last())
                    .and_then(|v| v.parse::<u32>().ok())
                    .map(|v| v as f32)
                    .ok_or_else(|| "Unknown output volume".into())
            })
            .collect()
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn pulse_channel_balance_is_preserved() {
            assert_eq!(super::parse_volumes("Volume: front-left: 32768 / 50% / -18 dB, front-right: 65536 / 100% / 0 dB\n balance 0.5").unwrap(), vec![32768.0,65536.0]);
            assert!(super::parse_volumes("garbage").is_err());
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "system_audio_macos.rs"]
mod platform;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub struct Controller;
    impl Controller {
        pub fn open(_: SystemAudio) -> Result<Self, String> {
            Err("System audio control is unsupported".into())
        }
    }
    impl OutputControl for Controller {
        fn read(&self) -> Result<Vec<f32>, String> {
            Err("Unsupported".into())
        }
        fn write(&self, _: &[f32]) -> Result<(), String> {
            Err("Unsupported".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Clone)]
    struct Fake(Rc<RefCell<Vec<f32>>>);
    impl OutputControl for Fake {
        fn read(&self) -> Result<Vec<f32>, String> {
            Ok(self.0.borrow().clone())
        }
        fn write(&self, values: &[f32]) -> Result<(), String> {
            *self.0.borrow_mut() = values.to_vec();
            Ok(())
        }
    }
    #[test]
    fn duck_restores_original_channel_balance() {
        let fake = Fake(Rc::new(RefCell::new(vec![0.8, 0.4])));
        let change = Change::apply(fake.clone(), SystemAudio::Duck).unwrap();
        assert!(unchanged(&fake.read().unwrap(), &[0.16, 0.08]));
        drop(change);
        assert_eq!(fake.read().unwrap(), vec![0.8, 0.4]);
    }
    #[test]
    fn manual_volume_change_is_not_overwritten() {
        let fake = Fake(Rc::new(RefCell::new(vec![0.8])));
        let change = Change::apply(fake.clone(), SystemAudio::Duck).unwrap();
        fake.write(&[0.6]).unwrap();
        drop(change);
        assert_eq!(fake.read().unwrap(), vec![0.6]);
    }
    #[test]
    fn already_muted_output_stays_muted() {
        let fake = Fake(Rc::new(RefCell::new(vec![1.0])));
        drop(Change::apply(fake.clone(), SystemAudio::Mute).unwrap());
        assert_eq!(fake.read().unwrap(), vec![1.0]);
    }
    #[test]
    fn unmuted_output_is_restored() {
        let fake = Fake(Rc::new(RefCell::new(vec![0.0])));
        let change = Change::apply(fake.clone(), SystemAudio::Mute).unwrap();
        assert_eq!(fake.read().unwrap(), vec![1.0]);
        drop(change);
        assert_eq!(fake.read().unwrap(), vec![0.0]);
    }

    #[test]
    fn a_partial_channel_failure_rolls_back() {
        struct Failing {
            fake: Fake,
            fail_once: std::cell::Cell<bool>,
        }
        impl OutputControl for Failing {
            fn read(&self) -> Result<Vec<f32>, String> {
                self.fake.read()
            }
            fn write(&self, values: &[f32]) -> Result<(), String> {
                if self.fail_once.replace(false) {
                    self.fake.0.borrow_mut()[0] = values[0];
                    return Err("Second channel is unavailable".into());
                }
                self.fake.write(values)
            }
        }
        let fake = Fake(Rc::new(RefCell::new(vec![0.8, 0.4])));
        assert!(Change::apply(
            Failing {
                fake: fake.clone(),
                fail_once: std::cell::Cell::new(true)
            },
            SystemAudio::Duck
        )
        .is_err());
        assert_eq!(fake.read().unwrap(), vec![0.8, 0.4]);
    }
}
