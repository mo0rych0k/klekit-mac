# KleKit 🎙️

> **Offline, privacy-first voice dictation and text refinement assistant exclusively for macOS (Apple Silicon)**

KleKit lets you dictate text using your voice or refine selected text on your machine — no internet, no cloud, no data leaving your device. Press a hotkey, speak/select, and the processed text is pasted instantly wherever your cursor is.

![KleKit Screenshot](resources/screenshot.png)

---

## ✨ Features

- 🎙️ **Voice Dictation** — Record voice input with a global hotkey, transcribe offline via Whisper STT, and paste the transcription directly at your cursor.
- ✍️ **Text Refinement & Agents** — Highlight any text in any app, trigger a custom Agent hotkey, and have Gemma automatically refine, edit, format, or translate it in place.
- 🔒 **100% Offline** — All processing happens locally on-device, ensuring complete privacy.
- ⚡ **macOS Native & Accelerated** — Dynamic compilation and execution tailored to Apple Silicon (Metal/XNNPACK) on macOS.
- 🧠 **Smart Gemma 4 Refiner** — Google AI Edge **Gemma 4 E2B** model fixes grammar, formats technical terms, and cleans up speech artifacts.
- 🎯 **Works Anywhere** — Emulates keystrokes to paste text into any active input field (IDE, browser, Slack, Notes…).
- 💤 **Zero Idle RAM** — Models automatically unload after 60 seconds of inactivity to keep a tiny memory footprint (~20-30 MB).

---

## 🏗️ Architecture

KleKit operates via two distinct pipelines:

1. **Voice Dictation (Whisper STT):**
```
Voice Hotkey (hold) ──> cpal Audio Recorder ──> whisper-rs STT (GGML) ──> (Optional Gemma 4 LLM) ──> OS Paste Inject
```

2. **Selected Text Refinement (Gemma LLM Agents):**
```
Agent Hotkey ──> OS Copy (Cmd+C) ──> Gemma 4 LLM (LiteRT-LM) ──> OS Paste Inject (Cmd+V)
```

| Module | Technology / Implementation | Role |
|---|---|---|
| Audio capture | `cpal` (see [src/audio.rs](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/audio.rs)) | 16 kHz mono PCM from mic |
| Speech-to-text | `whisper-rs` (whisper.cpp) | Offline transcription |
| Text refinement | [llm_refiner.rs](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/bin/llm_refiner.rs) (`litert-lm` CLI / Gemma 4 E2B) | Grammar fix, formatting |
| OS integration | [src/os_integration.rs](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/os_integration.rs) (`arboard` + CoreGraphics / API) | Hotkey & clipboard paste |
| App shell | `Tauri 2` (see [src-tauri/Cargo.toml](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src-tauri/Cargo.toml)) | Menu-bar tray app |

The core coordinator for this workflow is the [VoiceAssistantEngine](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/lib.rs#L28) struct.

### 🍏 New macOS Inference Engine
Unlike the old GGUF-based inference, KleKit now utilizes Google's official **AI Edge LiteRT-LM** engine. 
- The Rust sidecar dynamically locates the `litert-lm` command-line tool within your system `PATH`, with a fallback for macOS Python environments (`~/Library/Python/3.14/bin/litert-lm`).
- It executes the official Gemma 4 E2B model in `.litertlm` format.
- Computations are heavily accelerated via **XNNPACK** and macOS CPU vector instructions, producing highly accurate, real-time grammar corrections.

---

## 📋 Requirements

- **macOS** 11.0+ (Apple Silicon M1/M2/M3/M4 recommended)
- [Rust](https://rustup.rs/) 1.77+
- [Tauri CLI](https://tauri.app/start/prerequisites/) v2
- **Google AI Edge `litert-lm` CLI tool** (must be installed globally or at `~/Library/Python/3.14/bin/litert-lm` on macOS).
- Model files (stored locally, **not** included in this repo):
  - `models/ggml-large-v3-turbo-q5_0.bin` — Whisper large-v3-turbo Q5_0 model ([download](https://huggingface.co/ggerganov/whisper.cpp))
  - `models/gemma-4-E2B-it.litertlm` — Gemma 4 E2B official Google AI Edge model

---

## 🚀 Getting Started

### 1. Clone the repo
```bash
git clone https://github.com/mo0rych0k/klekit-mac.git
cd klekit-mac
```

### 2. Download models
```bash
mkdir -p models

# Whisper large-v3-turbo-q5_0 (~550 MB)
curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin \
  -o models/ggml-large-v3-turbo-q5_0.bin

# Place the official gemma-4-E2B-it.litertlm model in your models directory
# (Download gemma-4-E2B-it.litertlm from Kaggle or Hugging Face Google AI Edge repository)
```

### 3. Build and run
```bash
cargo tauri dev
```

### 4. Grant permissions
On first launch, the OS will prompt for:
- **Microphone** — for audio capture
- **Accessibility / Input Monitoring** — for global hotkey and clipboard/paste emulation (see [inject_text](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/os_integration.rs#L38))

---

## ⚙️ Configuration

Settings are stored at the user's config directory (e.g., `~/Library/Application Support/klekit/settings.json` on macOS). The configurations are managed by [AppSettings](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/settings.rs#L24) and [AgentProfile](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/settings.rs#L8):

```json
{
  "hotkey": "F13",
  "stt_language": "en",
  "vocabulary_hint": "",
  "llm_enabled": true,
  "llm_translate_to_english": false,
  "whisper_model_path": "models/ggml-large-v3-turbo-q5_0.bin",
  "llm_model_path": "models/gemma-4-E2B-it.litertlm"
}
```

The system prompt presets and vocabulary defaults are loaded from the [resources/](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/resources) folder:
- **Voice Recognition Hints**: [prompt_for_speak.txt](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/resources/prompt_for_speak.txt) (loaded via [load_prompt_for_speak](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/settings.rs#L85)) is a plain text file containing comma-separated technical keywords and terms (e.g. JSON, HTML, Rust, Tauri). It is passed as the initial prompt to the Whisper STT model to prime its spelling and formatting.
- **LLM Refiner Presets**: [gemma_prompts.json](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/resources/gemma_prompts.json) (loaded via [load_gemma_prompts](file:///Users/sergeypylypyshko/Documents/Projects/rust/offline_voice_assistant/src/settings.rs#L80)) is a JSON file containing the base prompt configurations (`default_prompt`) and templates used by the Gemma 4 refiner.

---

## 🧠 Memory Management

KleKit utilizes a "Zero-Memory Idle" state:

| State | Memory usage |
|---|---|
| Idle (models unloaded) | ~20–30 MB |
| Transcribing (Whisper loaded) | ~500 MB |
| Refining (Gemma 4 running via LiteRT) | ~2–3 GB (during inference only) |

Models are **automatically unloaded** after **60 seconds** of inactivity to preserve system memory.

---

## 🛠️ Development

```bash
# Run with hot-reload
cargo tauri dev

# Build release bundle
cargo tauri build

# Run tests
cargo test
```

---

## 📄 License

MIT © [mo0rych0k](https://github.com/mo0rych0k) - [MIT License Terms](https://opensource.org/licenses/MIT)
