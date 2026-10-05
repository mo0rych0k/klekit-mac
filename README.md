# KleKit 🎙️

> **Offline, privacy-first voice dictation and text refinement assistant exclusively for macOS (Apple Silicon)**

KleKit lets you dictate text using your voice or refine selected text on your machine — no internet, no cloud, no data leaving your device. Press a hotkey, speak/select, and the processed text is pasted instantly wherever your cursor is.

![KleKit Screenshot](resources/screenshot.png)

---

## ✨ Features

- 🎙️ **Voice Dictation** — Record voice input with a global hotkey, transcribe offline via Whisper STT, and paste the transcription directly at your cursor.
- ✍️ **Text Refinement & Agents** — Highlight any text in any app, trigger a custom Agent hotkey, and have the selected local LLM refine, edit, format, or translate it in place.
- 📁 **Local Models Manager** — Built-in GUI dashboard to scan, download, manage, and switch local Whisper and LiteRT models with one click.
- 🌍 **Multilingual Interface** — Fully localized in 20 languages (English, Ukrainian, Spanish, French, German, Japanese, Chinese, and more).
- 🔒 **100% Offline** — All processing happens locally on-device, ensuring complete privacy.
- ⚡ **macOS Native & Accelerated** — Dynamic execution tailored to Apple Silicon (Metal/XNNPACK) on macOS with native keystroke synthesis.
- 🧠 **Smart Local Refiner** — **Gemma 4 (E4B / E2B)** or **Qwen3 4B** can fix text, translate, format business messages, structure speech, or create AI-agent prompts.
- 🧩 **Five Prompt Presets** — Every agent configuration can use **No Preset**, **Fix Text**, **Business Style**, **Structured Text**, or **Create Prompt**; a Custom Prompt remains available for extra instructions.
- ⏱️ **Bounded Recording** — Choose a maximum recording length from 1 to 10 minutes in Settings to keep memory use predictable.
- 📜 **Optional Local History** — Keep voice history on-device, disable it entirely, or set automatic retention in days.
- 🎯 **Works Anywhere** — Emulates native keystrokes (`Cmd+V`) via macOS CoreGraphics to paste text into any active input field (IDE, browser, Slack, Notes…).
- 💤 **Zero Idle RAM** — Models automatically unload after 60 seconds of inactivity to keep a tiny memory footprint (~20-30 MB).

---

## 🏗️ Architecture

KleKit operates via two distinct pipelines:

1. **Voice Dictation (Whisper STT):**
```
Voice Hotkey (hold) ──> cpal Audio Recorder ──> whisper-rs STT (GGML) ──> (Optional active LLM) ──> OS Paste Inject
```

2. **Selected Text Refinement (Local LLM Agents):**
```
Agent Hotkey ──> OS Copy (Cmd+C) ──> Selected LLM (LiteRT-LM) ──> OS Paste Inject (Cmd+V)
```

| Module | Technology / Implementation | Role |
|---|---|---|
| Audio capture | `cpal` (see [src/audio.rs](src/audio.rs)) | 16 kHz mono PCM from mic |
| Speech-to-text | `whisper-rs` (whisper.cpp) | Offline transcription |
| Text refinement | [llm_refiner.rs](src/bin/llm_refiner.rs) (`litert-lm` CLI / Gemma 4 E4B or Qwen3 4B) | Preset-driven text transformation |
| OS integration | [src/os_integration.rs](src/os_integration.rs) (`arboard` + CoreGraphics) | Hotkey & clipboard paste |
| App shell | `Tauri 2` (see [src-tauri/Cargo.toml](src-tauri/Cargo.toml)) | Menu-bar tray app |

The core coordinator for this workflow is the [VoiceAssistantEngine](src/lib.rs) struct.

### 🍏 New macOS Inference Engine
Unlike the old GGUF-based inference, KleKit now utilizes Google's official **AI Edge LiteRT-LM** engine. 
- The Rust sidecar dynamically locates the `litert-lm` command-line tool within your system `PATH`, with a fallback for macOS Python environments (`~/Library/Python/3.14/bin/litert-lm`).
- It executes the selected compatible Gemma or Qwen model in `.litertlm` format.
- The sidecar uses the correct chat format for the selected model and disables the CLI's automatic template, so the prompt is applied exactly once. It is rebuilt automatically in development when its source changes.

---

## 📋 Requirements

- **macOS** 11.0+ (Apple Silicon M1/M2/M3/M4 recommended)
- [Rust](https://rustup.rs/) 1.77+
- [Tauri CLI](https://tauri.app/start/prerequisites/) v2
- **Google AI Edge `litert-lm` CLI tool** (must be installed globally or at `~/Library/Python/3.14/bin/litert-lm` on macOS).
- Model files stored anywhere on the local disk (**not bundled in the app**):
  - `models/ggml-large-v3-turbo.bin` — full Whisper large-v3-turbo model ([download](https://huggingface.co/ggerganov/whisper.cpp))
  - `models/gemma-4-E4B-it.litertlm` — Gemma 4 E4B official Google AI Edge model
  - `models/qwen3_4b_mixed_int4.litertlm` — Qwen3 4B mixed-int4 LiteRT Community model

---

## 🚀 Installation & Getting Started

### 1. Install `litert-lm` CLI
KleKit uses Google AI Edge LiteRT for LLM text refinement:
```bash
pip install litert-lm
# or ensure ~/Library/Python/3.14/bin/litert-lm or litert-lm in PATH is executable
```

### 2. Install via DMG (Recommended)
Open the generated DMG file and drag `KleKit.app` into your `Applications` folder:
- **DMG Path:** `target/release/bundle/dmg/KleKit_0.2.0_aarch64.dmg`
- To build the DMG yourself:
  ```bash
  ./scripts/build_dmg.sh
  ```

Alternatively, copy the built `.app` bundle directly:
```bash
cp -r target/release/bundle/macos/KleKit.app /Applications/
```
If prompted by macOS Gatekeeper on first launch (or if downloaded as an unsigned build):
```bash
xattr -cr /Applications/KleKit.app
```

### 3. Manage Models in the App (New!)
You can now download and manage models directly from KleKit's GUI:
1. Open KleKit and click **🧠 Local Models** (Локальні моделі) in the left sidebar.
2. Select your models directory (defaults to `~/Models`).
3. The app scans and displays all downloaded `.bin`, `.litertlm`, and `.gguf` weights, along with active statuses.
4. Click **⬇️ Download** for one-click downloading or **🤗 Hugging Face ↗** to open the model source directly.
5. Click **Set as Active** to activate a downloaded model immediately without manual path copying.

- **Whisper STT** (for voice dictation):
  ```bash
  mkdir -p ~/Models
  curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin \
    -o ~/Models/ggml-large-v3-turbo.bin
  ```
- **Local LLM** (for text refinement / agents):
  - Gemma 4 E4B (`.litertlm`) or Qwen3 4B (`.litertlm`).

### 4. Configure Models in KleKit
Launch **KleKit** from `/Applications` or Spotlight, click the menu bar tray icon, and open **Settings → Model Files**:
- Select your **Whisper / Vesper model** (`.bin`).
- Select your **Gemma** or **Qwen 3 model** (`.litertlm`) and choose your active LLM.

### 5. Grant macOS Permissions
On first launch, macOS will request:
- 🎙️ **Microphone** — for offline voice recording.
- ⌨️ **Accessibility** & **Input Monitoring** (`System Settings → Privacy & Security`) — required for global shortcuts (`F8`, `F10`, etc.) and typing/pasting text (`Cmd+C` / `Cmd+V` emulation into active apps).

> [!TIP]
> **Troubleshooting Keystroke Paste (`Cmd+V`):**
> If text is copied to clipboard but not automatically pasted into your active application, open **System Settings → Privacy & Security → Accessibility**, find **KleKit**, and toggle it off and on again. macOS re-validates accessibility permissions whenever an application binary or bundle signature is updated.


---

## ⚙️ Configuration

Settings are stored in `~/Library/Application Support/klekit/config.json` on macOS. They are managed by [AppSettings](src/settings.rs) and [AgentProfile](src/settings.rs):

```json
{
  "enable_auto_paste": true,
  "enable_sounds": true,
  "enable_gemma": false,
  "voice_hotkey_type": "Keyboard",
  "voice_hotkey_value": "",
  "voice_stt_language": "auto",
  "voice_whisper_prompt": "",
  "voice_max_recording_minutes": 2,
  "save_history": true,
  "history_retention_days": 0,
  "whisper_model_path": "/Users/you/Models/ggml-large-v3-turbo.bin",
  "gemma_model_path": "/Users/you/Models/gemma-4-E4B-it.litertlm",
  "qwen_model_path": "/Users/you/Models/qwen3.litertlm",
  "active_llm_model": "gemma",
  "agents": []
}
```

`history_retention_days: 0` keeps history until it is manually cleared. For each agent, choose an output language; selecting `English` makes the final refinement/translation English. With **No Preset**, an empty Custom Prompt leaves the text unchanged.

The system prompt presets and vocabulary defaults are loaded from the [resources/](resources/) folder:
- **Voice Recognition Hints**: [prompt_for_speak.txt](resources/prompt_for_speak.txt) is a plain-text list of technical keywords used as Whisper's initial prompt.
- **LLM Refiner Presets**: [gemma_prompts.json](resources/gemma_prompts.json) contains the base prompt and agent templates.

---

## 🧠 Memory Management

KleKit utilizes a "Zero-Memory Idle" state:

| State | Memory usage |
|---|---|
| Idle (models unloaded) | ~20–30 MB |
| Transcribing (Whisper loaded) | ~500 MB |
| Refining (active LLM running via LiteRT) | ~2–3 GB (during inference only) |

Whisper is automatically unloaded after **60 seconds** of inactivity. The active LLM runs only for the lifetime of its `litert-lm` subprocess.

---

## 🛠️ Development

```bash
# Run with hot-reload
cargo tauri dev
# or: npx @tauri-apps/cli dev

# Build release bundle
cargo tauri build
# or: npx @tauri-apps/cli build

# Run tests
cargo test
```

---

## 📄 License

MIT © [mo0rych0k](https://github.com/mo0rych0k) - [MIT License Terms](https://opensource.org/licenses/MIT)
