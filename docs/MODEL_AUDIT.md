# Model catalog audit — 2026-10-03

HotYap focuses on local Russian dictation with embedded English technical terms.
Admission requires RU/EN support, downloadable weights, a documented source,
a compatible packaged runtime, and a distinct reason to choose the model.
Popularity alone does not establish recognition quality.

## Added for alpha.16

| Model | Purpose | Download | Runtime | Pinned revision |
|---|---|---|---|---|
| [RuEn Large v3 Turbo](https://huggingface.co/mobiuslabsgmbh/faster-whisper-large-v3-turbo) | General multilingual Turbo complementing Russian fine-tunes; fewer decoder layers than full Large v3 | 1.622 GB | CTranslate2: CUDA or CPU | `0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf` |
| [Apple Silicon Large v3](https://huggingface.co/mlx-community/whisper-large-v3-mlx) | Full Large v3 as a quality-oriented alternative to Small/Turbo; more memory and decoding time | 3.084 GB | MLX/Metal, native Apple Silicon, macOS 14+ | `49e6aa286ad60c14352c404340ded53710378a11` |

Both repositories declare MIT. Download sizes come from repository file metadata,
not runtime RAM/VRAM estimates. CTranslate2 weights are FP16; CPU inference uses
Int8 and CUDA chooses a supported compute type. Labels distinguish the stored
weight format from runtime compute type.

The Turbo repository is the mapping used by upstream
[faster-whisper](https://github.com/SYSTRAN/faster-whisper/blob/master/faster_whisper/utils.py).
The MLX repository documents direct use with `mlx-whisper`, the existing backend.
Both support the existing prompt and dictation/file-transcription paths.
No additional runtime or remote code is needed.

## Considered, not included

- [NVIDIA Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3):
  supports RU/EN and is promising, but FastConformer/TDT needs a separate inference
  adapter and packaging/cancellation/long-file validation.
- [Distil Large v3.5](https://huggingface.co/distil-whisper/distil-large-v3.5-ct2):
  English-only does not match the current Russian-first inference path.
- [Antony66 Russian Large v3](https://huggingface.co/antony66/whisper-large-v3-russian):
  the original repository contains Transformers weights, not a ready CT2/MLX
  package. Another Russian model needs mixed-language evaluation before admission.
- [MLX Turbo q4](https://huggingface.co/mlx-community/whisper-large-v3-turbo-q4):
  deferred pending on-device comparison of quantization quality and runtime memory
  against the already supported Small/Turbo variants.

## Reliability fixes

- CT2 readiness requires tokenizer, configuration and vocabulary as well as weights.
- Python and Rust use the same supported MLX weight filenames.
- Download totals count allowed files, excluding unrelated formats in a repository.
- Discovery requires Russian and English language IDs in the pinned CT2
  configuration and a complete file layout; English-only models are rejected.
- Cached/discovered entries cannot replace reviewed built-in revisions or labels.
- Discovery paths are validated before constructing local model paths.

## Validation boundaries

Pinned files/configurations and runtime compatibility are audited against Hugging
Face and upstream documentation. Unit tests cover readiness, language filtering
and backend isolation. Release builds smoke-test packaged workers on all targets.
Apple CI checks MLX on CPU because hosted runners may not expose Metal.
This is not an on-device speech-quality benchmark: there is no claim that a new
model beats the Russian fine-tunes on every recording. Defaults are preserved.
