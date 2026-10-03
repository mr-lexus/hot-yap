use super::*;
use objc2_core_audio::*;
use std::ptr::{null, NonNull};

pub struct Controller {
    device: u32,
    addresses: Vec<AudioObjectPropertyAddress>,
    mute: bool,
}

fn address(selector: u32, scope: u32, element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: element,
    }
}
fn checked(status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "Output device does not support this audio control (Core Audio {status})"
        ))
    }
}
fn read<T: Default>(device: u32, mut addr: AudioObjectPropertyAddress) -> Result<T, String> {
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    unsafe {
        checked(AudioObjectGetPropertyData(
            device,
            NonNull::from(&mut addr),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        ))?;
    }
    if size != std::mem::size_of::<T>() as u32 {
        return Err("Unexpected Core Audio property size".into());
    }
    Ok(value)
}
fn write<T>(device: u32, mut addr: AudioObjectPropertyAddress, mut value: T) -> Result<(), String> {
    unsafe {
        checked(AudioObjectSetPropertyData(
            device,
            NonNull::from(&mut addr),
            0,
            null(),
            std::mem::size_of::<T>() as u32,
            NonNull::from(&mut value).cast(),
        ))
    }
}
fn settable(device: u32, mut addr: AudioObjectPropertyAddress) -> bool {
    let mut writable = 0;
    unsafe {
        AudioObjectIsPropertySettable(
            device,
            NonNull::from(&mut addr),
            NonNull::from(&mut writable),
        ) == 0
            && writable != 0
    }
}

impl Controller {
    pub fn open(mode: SystemAudio) -> Result<Self, String> {
        let device = read::<u32>(
            kAudioObjectSystemObject,
            address(
                kAudioHardwarePropertyDefaultOutputDevice,
                kAudioObjectPropertyScopeGlobal,
                kAudioObjectPropertyElementMain,
            ),
        )?;
        let mute = mode == SystemAudio::Mute;
        let selector = if mute {
            kAudioDevicePropertyMute
        } else {
            kAudioDevicePropertyVolumeScalar
        };
        let master = address(
            selector,
            kAudioObjectPropertyScopeOutput,
            kAudioObjectPropertyElementMain,
        );
        let addresses = if settable(device, master) {
            vec![master]
        } else {
            // A stereo device can expose only per-channel software controls.
            let stereo: Vec<_> = (1..=2)
                .map(|channel| address(selector, kAudioObjectPropertyScopeOutput, channel))
                .collect();
            if !stereo.iter().all(|a| settable(device, *a)) {
                return Err("This output has no writable volume/mute control. Choose Do nothing for this device.".into());
            }
            stereo
        };
        Ok(Self {
            device,
            addresses,
            mute,
        })
    }
}
impl OutputControl for Controller {
    fn read(&self) -> Result<Vec<f32>, String> {
        self.addresses
            .iter()
            .map(|a| {
                if self.mute {
                    read::<u32>(self.device, *a).map(|v| v as f32)
                } else {
                    read::<f32>(self.device, *a)
                }
            })
            .collect()
    }
    fn write(&self, values: &[f32]) -> Result<(), String> {
        for (a, v) in self.addresses.iter().zip(values) {
            if self.mute {
                write(self.device, *a, *v as u32)?;
            } else {
                write(self.device, *a, *v)?;
            }
        }
        // HAL may apply writes asynchronously; allow the readback to see the
        // quantized hardware value before retaining it for ownership checks.
        std::thread::sleep(std::time::Duration::from_millis(40));
        Ok(())
    }
}
