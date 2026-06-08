# Unified Voice Recording and Text Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate the voice recording path (transcribing via Whisper only and pasting directly) from the agent prompt refinement path (refining selected or deleted text via Gemma and configured hotkeys).

**Architecture:** Update `AppSettings` and SQLite database to extract voice STT parameters globally. Add global hotkey handlers in Tauri. Add a dedicated "Editor" tab with deletion tracking and keyboard shortcuts in the UI.

**Tech Stack:** Rust, Tauri v2, SQLite (rusqlite), HTML/CSS/JavaScript (Vanilla).

---

### Task 1: Overhaul AppSettings Schema and SQLite database

**Files:**
- Modify: `src/settings.rs`
- Modify: `src/db.rs`
- Test: `src/settings.rs` (in mod tests)
- Test: `src/db.rs` (in mod tests)

- [ ] **Step 1: Update settings schemas and defaults**
  Modify `src/settings.rs` to add global voice settings to `AppSettings` and remove speech fields from `AgentProfile`.
  ```rust
  // In src/settings.rs:
  #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
  pub struct AgentProfile {
      pub id: String,
      pub name: String,
      pub target_language: String,
      pub preset_id: String,
      pub custom_prompt: String,
      pub hotkey_type: String, // "Keyboard" or "Mouse"
      pub hotkey_value: String,
      #[serde(default = "default_true")]
      pub is_active: bool,
  }

  #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
  pub struct AppSettings {
      pub agents: Vec<AgentProfile>,
      #[serde(default = "default_true")]
      pub enable_auto_paste: bool,
      #[serde(default = "default_true")]
      pub enable_sounds: bool,
      
      // Global Recording Settings
      #[serde(default = "default_voice_hotkey_type")]
      pub voice_hotkey_type: String,
      #[serde(default = "default_voice_hotkey_value")]
      pub voice_hotkey_value: String,
      #[serde(default = "default_voice_stt_language")]
      pub voice_stt_language: String,
      #[serde(default = "default_voice_whisper_prompt")]
      pub voice_whisper_prompt: String,
  }

  fn default_voice_hotkey_type() -> String { "Keyboard".to_string() }
  fn default_voice_hotkey_value() -> String { "".to_string() }
  fn default_voice_stt_language() -> String { "auto".to_string() }
  fn default_voice_whisper_prompt() -> String { "".to_string() }
  ```

- [ ] **Step 2: Update SQLite database schema and Agent queries**
  Modify `src/db.rs` to reflect the updated `AgentProfile` struct in SQLite schema, migrations, insertion, and retrieval methods.
  ```rust
  // In src/db.rs, update connection setup and queries:
  // Remove columns 'stt_language' and 'whisper_prompt' from SQL statements in DbManager.
  ```

- [ ] **Step 3: Run cargo tests to verify compilation and test suite passes**
  Run: `cargo test`
  Expected: All existing tests compile and pass.

- [ ] **Step 4: Commit changes**
  ```bash
  git add src/settings.rs src/db.rs
  git commit -m "feat: restructure settings and database schemas for global voice parameters"
  ```

---

### Task 2: Implement Backend Tauri Commands and Engine Orchestration

**Files:**
- Modify: `src/lib.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Update VoiceAssistantEngine to handle Whisper-only vs Refinement**
  Modify `run_processing_pipeline` in `src/lib.rs` to handle raw transcription without Gemma processing when triggered by the global voice recorder hotkey.
  Implement a new helper command to run LLM refiner on text directly:
  ```rust
  // In src/lib.rs:
  pub fn run_text_refinement(&self, text: &str, agent_id: &str) -> Result<String> {
      // Spawns llm_refiner sidecar and passes prompt/text via stdin
  }
  ```

- [ ] **Step 2: Add refine_text Tauri command**
  Add the command to `src-tauri/src/lib.rs` and register it in `tauri::generate_handler!`.
  ```rust
  #[tauri::command]
  pub fn refine_text(text: String, agent_id: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
      state.engine.run_text_refinement(&text, &agent_id).map_err(|e| e.to_string())
  }
  ```

- [ ] **Step 3: Reconcile global hotkey registrations**
  Update `src-tauri/src/lib.rs`'s setup logic to register the global `voice_hotkey_value` for Whisper-only recording, and register individual agent hotkeys (like `F9`) for triggering refinement on selection/deletion.

- [ ] **Step 4: Run cargo build to verify compilation**
  Run: `cargo build`
  Expected: Build succeeds without errors.

- [ ] **Step 5: Commit changes**
  ```bash
  git add src/lib.rs src-tauri/src/lib.rs
  git commit -m "feat: implement raw text refinement backend pipelines and tauri commands"
  ```

---

### Task 3: Build the UI Settings Overlay and Editor Layout

**Files:**
- Modify: `ui/index.html`

- [ ] **Step 1: Add the Editor Tab HTML Structure**
  Add the navigation menu button and the `#tab-editor` HTML workspace with a glassmorphic textarea, controls, and deletion buffer panel.

- [ ] **Step 2: Implement Deletion Tracking and F9 keypress listener in Javascript**
  Write JS handlers inside `ui/index.html` to capture deleted strings and trigger selection/deletion refinement on `F9` (or configuration specified keypress).

- [ ] **Step 3: Overhaul the Settings Tab UI**
  Split the UI Settings form to show the **Global Voice Recorder** configurations at the top, and remove voice parameters from the agent profiles.

- [ ] **Step 4: Test UI loaded in Tauri Dev Mode**
  Run: `cargo tauri dev`
  Expected: The app compiles and opens, displaying the new Editor tab and unified Settings card.

- [ ] **Step 5: Commit changes**
  ```bash
  git add ui/index.html
  git commit -m "feat: UI overhaul adding Editor tab, deletion processing, and global voice settings card"
  ```
