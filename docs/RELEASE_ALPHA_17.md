# HotYap v0.1.0-alpha.17

## New

- Personal replacement dictionary with manual rules, correction suggestions, and optional automatic learning from edits made in HotYap.
- Project dictionaries with explicit context selection and a reviewable scan of project terminology. Project rules take priority over personal rules.
- Local transcription while speaking: pause-aware chunks, provisional text, an independent preview toggle, and a choice between full-recording finalization and faster completion.
- Optional paste into the original active application, with platform-specific focus checks and clipboard fallback.
- Microphone selection by persistent device ID, a system-default option, and a local 15-second signal test.
- System sound during dictation: unchanged, muted, or reduced to 20% of the current level, with restoration and respect for manual adjustments.
- Launch at sign-in and configurable close-to-tray behavior.
- Responsive Russian/English settings, dictionary management, and improved small-window layout.

## Reliability and defaults

Live transcription, automatic paste, launch at sign-in, and system sound changes are opt-in. Close-to-tray preserves the existing default. A disconnected explicitly selected microphone produces an error rather than silently changing the recording source. Test audio is discarded and never sent for transcription.

Native output restoration runs after recording stops, capture errors, worker failure, and graceful quit. Forced process termination or removal of the output device can prevent restoration.

## Installation and platform notes

Windows: MSI or setup EXE. Linux: DEB, RPM or AppImage. macOS: the matching Intel or Apple Silicon DMG. The local worker and model weights download separately on demand. Existing application data and models keep the same application identifier.

- Live decoding currently supports local CTranslate2 and MLX models. Preview text and punctuation are provisional until finalization.
- Windows paste uses native input events; macOS requires Accessibility/Automation permission. Linux paste requires X11 and xdotool; Wayland uses clipboard fallback.
- Linux system audio control requires pactl with PulseAudio or PipeWire's PulseAudio server. Some external outputs lack software volume/mute controls.
- Apple Silicon MLX requires macOS 14 or later.
- Installers are unsigned alpha builds. Cross-platform hardware testing is still ongoing.

## Validation

Automated Rust and Python checks, frontend lifecycle tests, desktop/landing builds, and browser interaction checks cover the release. Native microphone, audio-device switching, OS startup, and paste behavior still need wider testing with real hardware and applications.
