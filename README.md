# HotYap — Local RU/EN Voice Dictation

[![Verify](https://github.com/mr-lexus/hot-yap/actions/workflows/ci.yml/badge.svg)](https://github.com/mr-lexus/hot-yap/actions/workflows/ci.yml)
[![Landing](https://github.com/mr-lexus/hot-yap/actions/workflows/pages.yml/badge.svg)](https://github.com/mr-lexus/hot-yap/actions/workflows/pages.yml)

Website: [mr-lexus.github.io/hot-yap](https://mr-lexus.github.io/hot-yap/)

Alpha builds: [GitHub Releases](https://github.com/mr-lexus/hot-yap/releases)

Tauri v2 desktop app for speech-to-text dictation in Russian with embedded English technical terms. Local inference uses faster-whisper + CTranslate2 on Windows, Linux, and Intel macOS, or MLX + Metal on Apple Silicon. A configured cloud provider is optional. The result is copied to the system clipboard. Optional automatic paste can deliver the final text to the original foreground application; it is disabled by default.

The current release channel is **`0.1.0-alpha.17`**. Windows, Linux, Intel macOS, and Apple Silicon macOS packages are built automatically; installers remain early alpha builds while hardware coverage expands.

## What it does

```
hold Ctrl+Shift+Space → RECORD → release → TRANSCRIBE LOCALLY → COPY TO CLIPBOARD → DONE
```

- Models are downloaded from Hugging Face inside the app (one-time). The built-in catalog contains the verified `Russian First` Int8/Float16 and Int16 variants, `Code Switch` Int8/Float16, and the general `RuEn` line from `tiny` through `large-v3`. Apple Silicon builds also offer MLX Small, Large v3 Turbo and full Large v3 models. The RuEn catalog also includes the general multilingual Large v3 Turbo for CUDA/CPU. See [the model audit](docs/MODEL_AUDIT.md) for sources and selection rationale.
- The Models panel keeps model management in a modal. Each model is stored in its own directory and can be downloaded, loaded, or deleted independently. `Update catalog` performs a manual Hugging Face search, accepts only public repositories with a complete CTranslate2 layout and confirmed Russian/English language tokens, persists the result locally, and never downloads a discovered model automatically.
- Local mode has no cloud or telemetry. Optional text-only transcription history is disabled by default and stays on the device; audio leaves the device only when an external speech-to-text provider is explicitly selected.
- The file transcriber accepts common audio and video containers by picker or drag-and-drop, extracts audio locally, and runs the same local or cloud pipeline as live dictation. Long recordings are split into bounded chunks and can be cancelled; the editable result can be copied or saved as UTF-8 text.
- Russian speech + embedded English terms (e.g. `useEffect`, `git rebase`, `TypeScript`, `Docker`) are transcribed as-is and copied as UTF-8 text.

## Interface gallery

Real application screens with demonstration data. Explore all six scenarios on the [interactive gallery](https://mr-lexus.github.io/hot-yap/#gallery) ([Русский](https://mr-lexus.github.io/hot-yap/ru/#gallery)). Click any image to expand.

| Live dictation | Personal dictionary |
|---|---|
| [![Live dictation](public/landing/gallery/live-en.png)](public/landing/gallery/live-en.png) | [![Personal dictionary](public/landing/gallery/dictionary-en.png)](public/landing/gallery/dictionary-en.png) |
| Provisional local transcription while you speak. | Manual replacements, suggestions, and optional learning. |

| Project vocabulary | Microphone and sound |
|---|---|
| [![Project dictionary](public/landing/gallery/project-en.png)](public/landing/gallery/project-en.png) | [![Microphone settings](public/landing/gallery/audio-en.png)](public/landing/gallery/audio-en.png) |
| Review project terms before adding them. | Select and test your microphone; control output sound. |

| Dictation and delivery | Light appearance |
|---|---|
| [![Dictation settings](public/landing/gallery/workflow-en.png)](public/landing/gallery/workflow-en.png) | [![Light theme](public/landing/gallery/light-en.png)](public/landing/gallery/light-en.png) |
| Preview, finalization, and optional automatic paste. | Two themes and six accent colors. |

## Architecture

```
Tauri v2 / React / TS / Vite (UI + state)
        │  Tauri commands + events
        ▼
Rust backend (cpal recording → WAV via hound,
             media validation + chunking,
             clipboard-manager plugin,
             global-shortcut plugin)
        │
        ├─ local: JSONL → persistent Python worker → CTranslate2 or MLX
        └─ external: HTTPS → selected speech-to-text provider
                            → optional text post-processing provider
```

- The Python worker is a **single persistent child process** — the model loads once and is reused.
- `worker.py` speaks JSON Lines on stdout only; all diagnostics go to stderr.
- Commands: `status`, `download_model`, `load_model`, `prepare_media`, `transcribe`, `shutdown`.
- PyAV ships the FFmpeg libraries used to decode imported audio and video, so users do not need to install an external `ffmpeg` executable.
- Device auto-detection: MLX models use Metal (`float16`) on Apple Silicon; CTranslate2 models use CUDA when available and otherwise CPU (`int8`). The UI reports the device actually used.

## Development requirements

- macOS 14+ for Apple Silicon MLX development, with native arm64 Python and Xcode Command Line Tools
- Intel macOS with Xcode Command Line Tools, or Linux (X11 recommended; Wayland can lack the global shortcut while the UI button still works)
- Node.js 22.12+, pnpm 11.18
- Rust (rustup)
- Python 3.10+
- Tauri Linux system libraries (Ubuntu/Debian/Mint):

```bash
sudo apt install -y libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libssl-dev build-essential pkg-config curl
```

## Setup & run

```bash
./scripts/bootstrap.sh   # checks toolchain, creates backend/.venv, installs Python+JS deps
pnpm tauri dev
```

Model location (app data dir, not in the repo):

```
Linux:  ~/.local/share/com.voxshift.app/models/
macOS:  ~/Library/Application Support/com.voxshift.app/models/
Windows: %APPDATA%\com.voxshift.app\models\
```

The legacy application identifier is intentionally preserved so existing downloaded models are not lost after the HotYap rename.

## Interface languages

The interface is available in English and Russian. Use the `EN / RU` switch in the header. Translations live in `src/locales/en.json` and `src/locales/ru.json` and are loaded through `i18next` / `react-i18next`.

The public landing has separate, indexable language URLs:

- English: [mr-lexus.github.io/hot-yap/](https://mr-lexus.github.io/hot-yap/)
- Russian: [mr-lexus.github.io/hot-yap/ru/](https://mr-lexus.github.io/hot-yap/ru/)

## External providers

Open **Settings** to select the transcription backend and optional text post-processing stage.

- Speech-to-text: OpenAI, Deepgram, Groq, ElevenLabs, AssemblyAI, and Google Gemini.
- Text post-processing: OpenAI, Groq, OpenRouter, Anthropic, Google Gemini, xAI, Amazon Bedrock, Ollama, and LM Studio.
- Ollama and LM Studio default to loopback HTTP endpoints. Remote custom endpoints must use HTTPS.
- API keys are stored in the operating-system credential store and are never returned to the WebView. The matching environment variables are also supported.
- Accent color is stored locally in the WebView alongside the light/dark appearance setting.
- The favicon and native taskbar icon follow the operating-system theme by default. Settings can force a light logo for dark panels or a dark logo for light panels independently from the interface theme.

## Hotkey

`Ctrl+Shift+Space` — hold to record, release to stop and transcribe. The key can be changed with `Change key` in the Recording card; the setting is saved locally.

When the main window is not focused, the global push-to-talk shortcut opens a small always-on-top panel centered 80 pixels above the bottom of the screen. It shows live microphone activity, transcription progress, and the clipboard result, then hides automatically.

If the shortcut is already taken by another app, HotYap keeps running, shows a warning in the UI, and the UI button still works.

## Personal and project dictionaries

Open **Dictionary** in the toolbar. A replacement has a spoken form ("When I say") and an exact written form. Leave the spoken form empty to add a recognition hint. Replacements are case-insensitive, match whole words, run once without cascading, and support Russian and English. Explicit project rules override personal rules. The built-in technical-term defaults remain available and can be overridden.

- Add, edit, disable or delete entries. Origin badges distinguish manual entries, learned corrections and imported project terms.
- **Edit & teach** on the last transcript saves and copies a correction. Short word/phrase edits become reviewable suggestions by default. Dictionary learning can instead add them automatically or be disabled. HotYap does not observe edits in other applications. Corrections affect the current result; existing history records retain their original text.
- Add a project by selecting its folder. Scanning extracts source filenames, declaration names and `package.json` dependency names. It respects ignore files, excludes hidden files, dependency/build folders, symlinks and common secret filenames, and is bounded to 2,000 files / 16 MB / 300 candidate terms. Review the candidates before importing. No source files are uploaded or modified.
- Select the active project in the recording card, or activate it from the dictionary. Personal entries remain active. Each recording freezes its dictionary context at the start; changes apply to subsequent recordings. File transcription uses the dictionary selected when the job starts.
- Dictionaries are stored locally in `dictionary.json`. Approved terms become bounded prompt hints for local CTranslate2 and MLX models; explicit replacements also apply to cloud transcription and imported files. Approved terms may consequently appear in text sent to an enabled text-processing provider.

## Live dictation and automatic paste

Both options are in **Settings → Dictation & delivery**, and both default to off.

**Live local transcription** starts processing while the microphone is still recording. Completed phrases are cut at quiet pauses, with no forced cuts through continuous speech. At most one inference request runs at a time. Uncommitted previews are never appended to the final output, and preview frequency adapts to slower hardware. Continuous speech uses a bounded rolling draft; an ellipsis indicates that only the recent portion is shown. Audio is retained only for the current job and temporary chunks are deleted.

Choose how to finish:

- **Accuracy: full recording** (default): perform a final full-context decode. This preserves the ordinary model's punctuation behavior but does not promise lower post-recording latency.
- **Speed: remaining audio**: reuse completed phrases and decode only the remaining audio. This can reduce the wait on suitable hardware, but punctuation at phrase boundaries may differ. Actual acceleration depends on model speed and natural pauses.

In both modes, dictionary replacements and optional AI text processing are finalized before copying/pasting. The text provider runs once for the assembled dictation. Live decoding currently applies to local CTranslate2/MLX models; cloud audio is sent after recording stops.

**Automatic paste** captures the foreground target when a global recording starts and checks it again before sending the paste shortcut. It never activates another app and never sends Enter. If the target changes, clipboard contents change, keys are still held (Windows), permission is denied, or the desktop is unsupported, manual paste remains available. Recording from HotYap's own focused window has no external paste target. A successful status means the OS accepted the shortcut, not that every application confirmed insertion. The transcript remains in the clipboard; other clipboard formats are not restored.

| Platform | Behavior |
| --- | --- |
| Windows | Native `SendInput`, Ctrl+V or optional Ctrl+Shift+V. Checks foreground window, process and native focused control. Higher-integrity/elevated apps may reject input. |
| macOS | Command+V through System Events. Requires Accessibility and, when requested, Automation permission for HotYap. Checks the foreground process and window title/geometry; keep the intended text field focused within that window. |
| Linux X11 | Uses `xdotool` when installed, checks active window and focused control. Ctrl+V or Ctrl+Shift+V. |
| Linux Wayland | Automatic paste is unavailable; clipboard fallback is explicit in settings. Global shortcut availability also depends on the compositor. |

The paste delay is configurable from 150 to 2,000 ms. Ctrl+Shift+V is an explicit setting for terminals, not automatic terminal detection. Native focus checks cannot distinguish all browser/editor sub-fields inside a shared native control.

Platform references: [Windows SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput), [macOS Accessibility permission](https://support.apple.com/guide/mac-help/allow-accessibility-apps-to-access-your-mac-mh43185/mac), [xdotool and X11/Wayland limitations](https://github.com/jordansissel/xdotool).

## GPU / CPU / Apple Silicon

- CUDA is used automatically when CTranslate2 sees a CUDA device: `device=cuda`, preferring `float16` with lower-precision fallbacks.
- If the CUDA load fails (missing runtime libs `nvidia-cublas-cu12` / `nvidia-cudnn-cu12`, old driver, OOM), the app **falls back to CPU** (`int8`) and reports it in the UI. No crash.
- Note: modern CTranslate2 CUDA may require CUDA 12 + cuDNN 9. We do not modify your GPU driver. Optional Python deps for CUDA (add to `backend/requirements.txt` if you want them bundled):

```
nvidia-cublas-cu12
nvidia-cudnn-cu12
```

### macOS

- Apple Silicon builds (M1 and newer) require macOS 14 or later and include an MLX/Metal worker. `Apple Silicon · Small` is the recommended everyday model; `Apple Silicon · Large v3 Turbo` trades memory and latency for higher quality.
- The MLX models are shown only in native `aarch64-apple-darwin` builds. Rosetta launches and Intel Macs use the CTranslate2 catalog instead.
- Intel Macs run CTranslate2 on CPU. CTranslate2 has no Metal backend, and current Intel Mac Radeon GPUs are therefore not used for local inference.
- CPU models remain available on Apple Silicon as a compatibility fallback. For accelerated inference, select an Apple Silicon model and leave Device on Auto or choose Metal.

## Known limitations

- Recording uses the system default input device (no selector in this MVP).
- Automatic paste requires an external foreground target and OS support. Windows elevated apps can block it; macOS requires Accessibility/Automation permission; Linux X11 requires `xdotool`. Wayland uses clipboard-only delivery.
- Whisper-large-v3-turbo on a CPU without AVX2 is slow (see Troubleshooting); on CUDA or a modern AVX2 CPU, a short dictation takes seconds.
- Alpha installers are currently unsigned. Windows SmartScreen and macOS Gatekeeper warnings are expected.
- Linux, Windows, and both macOS architectures are built in the release matrix. Microphone permissions, global shortcuts, and real-model performance still need coverage across more physical Mac hardware.

## Troubleshooting

### CUDA

1. The UI shows `Engine: CUDA` only if a CUDA device was detected **and** the model loaded on it.
2. If you see `Engine: CPU` with a NVIDIA GPU, check the worker logs (stderr) for the fallback reason:
   - `CUDA probe failed: ...` — no CUDA runtime.
   - Install/update the NVIDIA driver, then `pip install nvidia-cublas-cu12 nvidia-cudnn-cu12` into `backend/.venv`.
3. If the model fails to load on GPU (e.g. out of memory), the worker logs it and loads on CPU instead.

### Microphone

- The worker prints which source was opened on every recording (`microphone 'default' selected: ...`).
- If you hear no transcription, check your input level with `pavucontrol`.
- Common issue: no default input device — plug in a mic or set one in sound settings.

### Global shortcut

- X11 only. On Wayland, registration fails safely: you'll see a warning in the UI, and the on-screen button remains functional.
- If `Ctrl+Shift+Space` is used by another app, that app may grab it first; change the other app's binding or use the UI button.

### Clipboard

To verify the clipboard contents manually after a dictation:

```bash
xclip -selection clipboard -o
```

### Slow transcription

- `faster-whisper` needs AVX2 for fast CPU inference. On CPUs without AVX2 (e.g. old AMD APUs), large-v3-turbo int8 runs at RTF ≈ 15, so a 5 s dictation takes ~80 s. This is a hardware limit — the same pipeline on a GPU (GTX 1650 Super, CUDA) runs at RTF < 1.
- Both `beam_size` (`inference.py`) and `device` are the knobs if you want speed on such machines.

## Build

Regression checks (no downloaded speech model or API keys required):

```bash
pnpm test
python3 -m unittest discover -s backend/tests -p 'test_*.py' -v
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

The worker protocol tests use a deterministic inference stub. Hardware recording,
CUDA/Metal inference, and real cloud providers still require integration testing. The
Apple Silicon release job additionally imports MLX and runs an MLX CPU operation inside
the frozen worker. This validates the native arm64 package without depending on GPU access
in a hosted runner; the application performs a real Metal operation when an MLX model loads.

```bash
pnpm build          # desktop frontend
pnpm site:build     # static bilingual landing -> dist-pages/
pnpm tauri build    # source-tree desktop build
```

Tagged alpha releases use `.github/workflows/release.yml`. The workflow creates a native PyInstaller worker for each target, adds it as a Tauri sidecar, builds the installers, and uploads them to a GitHub prerelease:

- Linux x86_64: `.deb`, `.rpm`, `.AppImage`
- Windows x86_64: `.msi`, NSIS `-setup.exe`
- macOS Intel and Apple Silicon: separate `.dmg` files

Speech models are intentionally not included in the installer because they range from roughly 78 MB to 3.1 GB. The user chooses and downloads a model inside the app on first use.

See [`docs/PUBLISHING.md`](docs/PUBLISHING.md) for the complete Pages, release, sidecar, SEO, and troubleshooting guide.

## Privacy

- Temporary WAV files are deleted after each transcription (including on errors).
- No telemetry. Text-only transcription history is opt-in, disabled by default, and stored in the app data directory; audio is never retained in history. Turning history off stops new entries without deleting existing ones, which remain available for explicit deletion. In local mode, imported media and microphone recordings stay on the device. In cloud mode, HotYap extracts imported audio locally, sends bounded audio chunks only to the selected speech-to-text provider, and sends the resulting text to the selected post-processing provider only when that optional stage is enabled. Temporary normalized audio and chunks are deleted after success, cancellation, and errors.


### Microphone, preview, and desktop behavior

Settings includes independent controls for live decoding and its preview. **Show dictation preview** hides/shows provisional text in both the main window and overlay; hiding it does not disable processing during speech.

- **Microphone:** Automatic resolves the current system input at the start of each recording. An explicit input is persisted using CPAL's device ID. A disconnected selected input produces an error instead of silently switching sources. Refresh the list after connecting a device; returning to the window also refreshes it.
- **Test microphone:** a local level meter using the same capture path as dictation, with a native 15-second deadline. Stop, change input, close settings, or start dictation to release the test microphone. The sample buffer is discarded and is never sent for transcription.
- **System sound:** Do nothing (default), mute, or lower to 20% of the current output level during recording. Windows uses Core Audio endpoint controls, macOS uses Core Audio HAL controls, and Linux uses `pactl` with PulseAudio or PipeWire's PulseAudio server. The original output device and channel levels are retained. Normal stop, input failure, worker failure, and graceful quit restore the changed setting unless the user has changed that setting meanwhile. An unavailable output control warns without preventing dictation. Device removal and forced process termination can prevent restoration. Some external outputs lack software volume/mute controls.
- **Launch at system startup:** opt-in, applied on Save using the official Tauri autostart plugin (Windows user startup registry, macOS LaunchAgent, Linux desktop autostart). The switch reads the OS registration when opening Settings. Registration errors are shown; failed settings persistence rolls the registration back. Enable in the installed app so the registered executable has a stable location.
- **Close window to system tray:** on by default, preserving existing behavior. Turn it off to make the window close button stop active capture/transcription and quit. Quit in the tray menu always exits.

Native microphone access, device changes, OS startup, and sound restoration require hardware/platform smoke tests in addition to the automated logic and browser UI checks.
