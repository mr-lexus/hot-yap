# HotYap / Горячий Лай — Local RU/EN Voice Dictation

[![Verify](https://github.com/mr-lexus/hot-yap/actions/workflows/ci.yml/badge.svg)](https://github.com/mr-lexus/hot-yap/actions/workflows/ci.yml)
[![Landing](https://github.com/mr-lexus/hot-yap/actions/workflows/pages.yml/badge.svg)](https://github.com/mr-lexus/hot-yap/actions/workflows/pages.yml)

Website: [mr-lexus.github.io/hot-yap](https://mr-lexus.github.io/hot-yap/)

Alpha builds: [GitHub Releases](https://github.com/mr-lexus/hot-yap/releases)

Tauri v2 desktop app for speech-to-text dictation in Russian with embedded English technical terms. It uses local faster-whisper + CTranslate2 by default and can optionally use a configured cloud provider. The result is copied to the system clipboard; the app never simulates keyboard input — you paste it yourself.

The current release channel is **`0.1.0-alpha.1`**. Linux is the primary tested platform; Windows and macOS packages are automated but should be treated as early alpha builds.

## What it does

```
hold Ctrl+Shift+Space → RECORD → release → TRANSCRIBE LOCALLY → COPY TO CLIPBOARD → DONE
```

- Models are downloaded from Hugging Face inside the app (one-time). The built-in catalog contains the verified `Russian First` Int8/Float16 and Int16 variants, `Code Switch` Int8/Float16, and the general `RuEn` line from `tiny` through `large-v3`.
- The Models panel keeps model management in a modal. Each model is stored in its own directory and can be downloaded, loaded, or deleted independently. `Update catalog` performs a manual Hugging Face search, accepts only repositories with a compatible CTranslate2 `model.bin`, persists the result locally, and never downloads a discovered model automatically.
- Local mode has no cloud, telemetry, or history. Audio leaves the device only when an external speech-to-text provider is explicitly selected.
- Russian speech + embedded English terms (e.g. `useEffect`, `git rebase`, `TypeScript`, `Docker`) are transcribed as-is and copied as UTF-8 text.

## Architecture

```
Tauri v2 / React / TS / Vite (UI + state)
        │  Tauri commands + events
        ▼
Rust backend (cpal recording → WAV via hound,
             clipboard-manager plugin,
             global-shortcut plugin)
        │
        ├─ local: JSONL → persistent Python worker → faster-whisper
        └─ external: HTTPS → selected speech-to-text provider
                            → optional text post-processing provider
```

- The Python worker is a **single persistent child process** — the model loads once and is reused.
- `worker.py` speaks JSON Lines on stdout only; all diagnostics go to stderr.
- Commands: `status`, `download_model`, `load_model`, `transcribe`, `shutdown`.
- Device auto-detection: CUDA (`int8_float16`) if available, otherwise CPU (`int8`) — the UI shows `Engine: CUDA` / `Engine: CPU`.

## Development requirements

- Linux (X11 recommended; hotkey needs X11 — Wayland will lack the global shortcut but the UI button still works)
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
~/.local/share/com.voxshift.app/models/ru-en-codeswitch/ct2_int8_float16/
```

The legacy application identifier is intentionally preserved so existing downloaded models are not lost after the HotYap rename.

## Interface languages

The interface is available in English (`HotYap`) and Russian (`Горячий Лай`). Use the `EN / RU` switch in the header. Translations live in `src/locales/en.json` and `src/locales/ru.json` and are loaded through `i18next` / `react-i18next`.

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

## GPU / CPU

- CUDA is used automatically when CTranslate2 sees a CUDA device: `device=cuda`, `compute_type=int8_float16`.
- If the CUDA load fails (missing runtime libs `nvidia-cublas-cu12` / `nvidia-cudnn-cu12`, old driver, OOM), the app **falls back to CPU** (`int8`) and reports it in the UI. No crash.
- Note: modern CTranslate2 CUDA may require CUDA 12 + cuDNN 9. We do not modify your GPU driver. Optional Python deps for CUDA (add to `backend/requirements.txt` if you want them bundled):

```
nvidia-cublas-cu12
nvidia-cudnn-cu12
```

## Known limitations

- Recording uses the system default input device (no selector in this MVP).
- No auto-paste by design — the app only writes the system clipboard.
- Whisper-large-v3-turbo on a CPU without AVX2 is slow (see Troubleshooting); on CUDA or a modern AVX2 CPU, a short dictation takes seconds.
- Alpha installers are currently unsigned. Windows SmartScreen and macOS Gatekeeper warnings are expected.
- Linux packages are the primary tested output. Windows and macOS need broader hardware and permission testing.

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
- No history or telemetry. In local mode, network access is used only for model catalog/download operations. In cloud mode, the temporary recording is sent only to the selected speech-to-text provider; the resulting text is also sent to the selected post-processing provider when that optional stage is enabled.
