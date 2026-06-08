# Design Spec: Unified Voice Recording and Text Refinement

This specification outlines the architecture and implementation details for separating the voice recording path (transcription via Whisper only) from the text refinement path (processing via Gemma).

---

## 1. Goal

1. Provide **one global voice recording path** that records audio, transcribes it with Whisper, and inserts the raw text directly at the cursor of any focused OS input field (or copies to the clipboard if no field is focused).
2. Allow configuring the **STT Language**, **STT prompt bias/hints**, and the **Global Voice Recorder Hotkey** from a dedicated global section in the app settings, rather than per agent.
3. Keep the **Gemma LLM (Jemma) processing** as separate, agent-specific refinement actions. Pressing an agent's configured hotkey (e.g. `F9`) will process selected text or last-deleted text using that agent's prompts and settings, inserting the output back.

---

## 2. Key Changes & Architecture

### A. Data Settings Overhaul

#### AppSettings Updates (`src/settings.rs`)
We will add global recorder settings directly to `AppSettings`, and remove `stt_language` and `whisper_prompt` from `AgentProfile`:

```rust
pub struct AppSettings {
    pub agents: Vec<AgentProfile>,
    #[serde(default = "default_true")]
    pub enable_auto_paste: bool,
    #[serde(default = "default_true")]
    pub enable_sounds: bool,

    // Global Recording settings
    #[serde(default = "default_voice_hotkey_type")]
    pub voice_hotkey_type: String, // "Keyboard" or "Mouse"
    #[serde(default = "default_voice_hotkey_value")]
    pub voice_hotkey_value: String,
    #[serde(default = "default_voice_stt_language")]
    pub voice_stt_language: String, // e.g. "auto", "uk", "en"
    #[serde(default = "default_voice_whisper_prompt")]
    pub voice_whisper_prompt: String,
}

pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub target_language: String,
    pub preset_id: String,
    pub custom_prompt: String,
    pub hotkey_type: String, // "Keyboard" or "Mouse"
    pub hotkey_value: String, // e.g. "F9"
    #[serde(default = "default_true")]
    pub is_active: bool,
}
```

#### SQLite Schema Updates (`src/db.rs`)
Modify the `DbManager` to store only the simplified `AgentProfile` fields:
* Remove `stt_language` and `whisper_prompt` columns or queries inside `get_all_agents` and `save_agents`.

---

### B. Core Execution Engine Workflow (`src/lib.rs`)

1. **Global Voice Dictation Loop:**
   * Triggered by the global `voice_hotkey_value` (e.g. `MouseButton4` or `F13`).
   * Records audio via `AudioRecorder`.
   * On release, Whisper transcribes the audio using the global `voice_stt_language` and `voice_whisper_prompt`.
   * Gemma processing is bypassed completely.
   * Inserts raw transcribed text into the active cursor position in the OS. If no focused field is active, it copies the text to the system clipboard.

2. **Agent Text Refinement Loop:**
   * Triggered by an agent profile's hotkey (e.g. `F9`).
   * The app checks for selected text in the focused application.
     * If text is selected: Extracted, processed via `Gemma` sidecar using the agent profile instructions, and pasted back.
     * If no text is selected: Reads the last deleted text segment from the editor textarea buffer, processes it via `Gemma`, and inserts it back.

---

### C. GUI Settings & Interface Overhaul (`ui/index.html`)

1. **New Editor Tab:**
   * A clean tab pane featuring a large `<textarea>` for text editing.
   * Tracks character deletion to maintain the last-deleted text buffer in memory.
   * A dropdown menu to select a preset prompt to run Gemma on the deleted buffer.
2. **Settings Tab Panel Overhaul:**
   * Adds a dedicated **Voice Recorder Settings** card at the top of the tab for configuring the global hotkey, STT language, and prompt hints.
   * Simplifies the **Agents Config** forms to only expose name, preset prompt, target language, and refinement hotkey (e.g., F9).

---

## 3. Verification Plan

### Automated Tests
* Run `cargo test` to verify SQLite and settings serialization compile and pass successfully.
* Add unit tests verifying `AppSettings` loads global fields correctly.

### Manual Verification
* Press the global recording hotkey, speak in Ukrainian, release, and check that raw transcribed text is inserted into any active text input field without Gemma edits.
* Select text, press `F9`, and confirm the selection is refined by Gemma and pasted back.
* Delete a word, press `F9`, and confirm the deleted word is processed by Gemma and inserted.
