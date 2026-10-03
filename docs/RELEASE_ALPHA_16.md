# HotYap v0.1.0-alpha.16

## New

- Native Apple Silicon inference with MLX/Metal on macOS 14+: Small, Large v3 Turbo and full Large v3. Intel Macs retain the CPU backend.
- General multilingual RuEn Large v3 Turbo for CUDA/CPU, complementing the existing Russian-focused models.
- Optional local transcription history, disabled by default: text only, favorites, search, filters and deletion.
- Audio/video file transcription with drag-and-drop, local audio extraction, cancellation, editable results and text export. Works with local models and configured external providers.
- Responsive window layouts, compact toolbar buttons and content-aware status wrapping.

## Reliability

- Reviewed model revisions remain pinned; discovery cannot overwrite the built-in catalog.
- Model discovery checks RU/EN support and required CT2 files. Partial downloads no longer appear ready when tokenizer/configuration files are missing.
- Download size estimates respect selected repository files.
- Recording, cancellation, worker lifecycle, provider configuration and atomic storage fixes, with regression tests.

## Installation

Windows: MSI or setup EXE. Linux: DEB, RPM or AppImage. macOS: the matching Intel or Apple Silicon DMG.

The local worker and model weights download separately on demand. Apple Silicon requires macOS 14 or later for MLX/Metal. Installers are unsigned alpha builds. Model-specific accuracy and speed depend on hardware and recordings; the model audit is not a comparative speech-quality benchmark.

Model sources and selection rationale: [MODEL_AUDIT.md](https://github.com/mr-lexus/hot-yap/blob/v0.1.0-alpha.16/docs/MODEL_AUDIT.md).
