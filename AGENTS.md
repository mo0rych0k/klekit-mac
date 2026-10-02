# KleKit agent guide

KleKit is a macOS, offline Rust/Tauri voice-dictation and selected-text-refinement app.

## Current architecture

- `src/audio.rs`: `cpal` capture, 16 kHz mono PCM.
- `src/lib.rs`: `VoiceAssistantEngine`; owns the Whisper lifecycle and coordinates both pipelines.
- `src/bin/llm_refiner.rs`: invokes the local `litert-lm` CLI with Gemma 4 (E4B / E2B) or Qwen3 4B.
- `src/os_integration.rs`: macOS clipboard and Cmd+C/Cmd+V injection via `arboard` and CoreGraphics synthesized modifier key sequences.
- `src-tauri/src/lib.rs`: Tauri tray/UI, local models management, and global shortcuts via `tauri-plugin-global-shortcut`.

Flows:

1. Voice shortcut → capture → `whisper-rs` → optional LiteRT-LM refinement → paste (via main thread).
2. Agent shortcut → copy selection (Cmd+C) → LiteRT-LM refinement → paste (Cmd+V via main thread).

Whisper loads on demand and is released after 60 seconds idle. Gemma/Qwen runs in a short-lived `litert-lm` subprocess; do not add an in-process LLM cache without measuring a need.

## Change rules

- Keep the app macOS-only until platform support is implemented and tested.
- Keep model weights and generated binaries out of Git.
- Preserve the `WhisperContext`/per-call `WhisperState` lifetime pattern; never make a self-referential wrapper.
- Do not block Tauri's UI thread with audio, transcription, or inference.
- Return contextual errors at I/O, process, and OS boundaries. Do not introduce new production `unwrap()` or `expect()` calls.
- Validate with `cargo check --workspace`; run focused tests for changed non-trivial logic.
