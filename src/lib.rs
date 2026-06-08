use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub mod audio;
pub mod db;
pub mod os_integration;
pub mod settings;
pub use settings::AppSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AppState {
    Idle,
    Recording,
    Transcribing,
    Refining,
    Success,
}

pub enum RecorderCommand {
    Start,
    Stop(crossbeam_channel::Sender<Result<Vec<f32>>>),
}


pub struct VoiceAssistantEngine {
    state: Arc<Mutex<AppState>>,
    whisper_ctx: Arc<Mutex<Option<Arc<WhisperContext>>>>,
    last_active: Arc<Mutex<Instant>>,
    whisper_model_path: PathBuf,
    gemma_model_path: PathBuf,
    refiner_bin_path: PathBuf,
    on_status_change: Arc<Mutex<Option<Box<dyn Fn(AppState) + Send + Sync + 'static>>>>,
    on_paste: Arc<Mutex<Option<Arc<dyn Fn(String) + Send + Sync + 'static>>>>,
    on_log: Arc<Mutex<Option<Box<dyn Fn(String) + Send + Sync + 'static>>>>,
    /// Shared pointer to the live settings; swapped atomically by the UI on save.
    pub settings: Arc<Mutex<Arc<AppSettings>>>,
    pub db: Arc<db::DbManager>,
    pub active_agent_id: Arc<Mutex<Option<String>>>,
}

impl VoiceAssistantEngine {
    /// Creates a new VoiceAssistantEngine with a shared settings reference.
    pub fn new(
        whisper_model: &str,
        gemma_model: &str,
        refiner_bin: &str,
        settings: Arc<Mutex<Arc<AppSettings>>>,
    ) -> Self {
        let db_path = AppSettings::config_path()
            .map(|p| p.parent().unwrap().join("history.db"))
            .unwrap_or_else(|_| PathBuf::from("history.db"));
        let db = Arc::new(db::DbManager::new(&db_path).expect("Failed to initialize SQLite history database"));

        // Load agents from SQLite to synchronize settings on startup
        if let Ok(db_agents) = db.get_all_agents() {
            if let Ok(mut settings_guard) = settings.lock() {
                let mut current_settings = (**settings_guard).clone();
                current_settings.agents = db_agents;
                *settings_guard = Arc::new(current_settings);
            }
        }

        Self {
            state: Arc::new(Mutex::new(AppState::Idle)),
            whisper_ctx: Arc::new(Mutex::new(None)),
            last_active: Arc::new(Mutex::new(Instant::now())),
            whisper_model_path: resolve_path(whisper_model),
            gemma_model_path: resolve_path(gemma_model),
            refiner_bin_path: resolve_path(refiner_bin),
            on_status_change: Arc::new(Mutex::new(None)),
            on_paste: Arc::new(Mutex::new(None)),
            on_log: Arc::new(Mutex::new(None)),
            settings,
            db,
            active_agent_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Sets a callback that triggers whenever the application state changes.
    pub fn set_status_callback(&self, cb: impl Fn(AppState) + Send + Sync + 'static) {
        *self.on_status_change.lock().unwrap() = Some(Box::new(cb));
    }

    /// Sets a callback that triggers when transcription and refinement are complete to paste/type text on the main thread.
    pub fn set_paste_callback(&self, cb: impl Fn(String) + Send + Sync + 'static) {
        *self.on_paste.lock().unwrap() = Some(Arc::new(cb));
    }

    /// Sets a callback that triggers when the engine logs info.
    pub fn set_log_callback(&self, cb: impl Fn(String) + Send + Sync + 'static) {
        *self.on_log.lock().unwrap() = Some(Box::new(cb));
    }

    /// Logs a message to stdout and triggers the log callback if registered.
    pub fn log(&self, msg: impl Into<String>) {
        let text = msg.into();
        println!("{}", text);
        if let Some(cb) = &*self.on_log.lock().unwrap() {
            cb(text);
        }
    }

    /// Retrieves the current application state.
    pub fn get_state(&self) -> AppState {
        *self.state.lock().unwrap()
    }

    /// Updates internal state and triggers the status callback if registered.
    fn update_state(&self, new_state: AppState) {
        *self.state.lock().unwrap() = new_state;
        *self.last_active.lock().unwrap() = Instant::now();
        if let Some(cb) = &*self.on_status_change.lock().unwrap() {
            cb(new_state);
        }
    }

    /// Starts a background inactivity checker that unloads Whisper's context after 1 minute.
    pub fn start_inactivity_timer(engine: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;

                let should_unload = {
                    let state_guard = engine.state.lock().unwrap();
                    let last_active_guard = engine.last_active.lock().unwrap();
                    let whisper_guard = engine.whisper_ctx.lock().unwrap();

                    *state_guard == AppState::Idle
                        && whisper_guard.is_some()
                        && last_active_guard.elapsed() >= Duration::from_secs(60) // 1 minute
                };

                if should_unload {
                    let mut whisper_guard = engine.whisper_ctx.lock().unwrap();
                    if whisper_guard.is_some() {
                        println!("\n🧹 Whisper model has been inactive for 1 minute. Unloading weights from memory (Zero-Memory Idle)...");
                        *whisper_guard = None;
                    }
                }
            }
        });
    }

    /// Dynamic Whisper context loader. Loads the model on demand if not already in memory.
    fn get_or_load_whisper(&self) -> Result<Arc<WhisperContext>> {
        let mut guard = self.whisper_ctx.lock().unwrap();
        if let Some(ctx) = &*guard {
            Ok(Arc::clone(ctx))
        } else {
            println!("⏳ Loading Whisper model from disk (dynamic lifecycle)...");
            let ctx = WhisperContext::new_with_params(
                self.whisper_model_path.to_str().context("Invalid whisper model path")?,
                WhisperContextParameters::default(),
            )
            .context("Failed to load Whisper model")?;
            let ctx_arc = Arc::new(ctx);
            *guard = Some(Arc::clone(&ctx_arc));
            Ok(ctx_arc)
        }
    }

    pub fn start_recording_for_agent(
        engine: Arc<Self>,
        recorder_tx: crossbeam_channel::Sender<RecorderCommand>,
        agent_id: String,
    ) -> Result<()> {
        engine.log(format!("VoiceAssistantEngine::start_recording_for_agent called: agent_id={}", agent_id));
        {
            let mut aid = engine.active_agent_id.lock().unwrap();
            *aid = Some(agent_id);
        }
        {
            let mut guard = engine.state.lock().unwrap();
            if *guard != AppState::Idle {
                return Ok(()); // Guard: ignore if already recording or processing
            }
            *guard = AppState::Recording;
            let enable_sounds = {
                let lock = engine.settings.lock().unwrap();
                lock.enable_sounds
            };
            if enable_sounds {
                os_integration::play_sound("start");
            }
        }
        if let Some(cb) = &*engine.on_status_change.lock().unwrap() {
            cb(AppState::Recording);
        }
        *engine.last_active.lock().unwrap() = Instant::now();

        // Pre-warm Whisper in parallel while user is speaking
        let engine_warm = Arc::clone(&engine);
        tokio::spawn(async move {
            let _ = engine_warm.get_or_load_whisper();
        });

        engine.log("\n🔴 Recording activated (Hold-to-Talk)! Speak...");
        if let Err(e) = recorder_tx.send(RecorderCommand::Start) {
            engine.log(format!("❌ Failed to start recording: {:?}", e));
            engine.update_state(AppState::Idle);
        }
        Ok(())
    }

    pub fn stop_and_process_for_agent(
        engine: Arc<Self>,
        recorder_tx: crossbeam_channel::Sender<RecorderCommand>,
        agent_id: String,
    ) -> Result<()> {
        engine.log(format!("VoiceAssistantEngine::stop_and_process_for_agent called: agent_id={}", agent_id));
        {
            let mut aid = engine.active_agent_id.lock().unwrap();
            *aid = Some(agent_id);
        }
        {
            let mut guard = engine.state.lock().unwrap();
            if *guard != AppState::Recording {
                return Ok(()); // Guard: only process if we were actually recording
            }
            *guard = AppState::Transcribing;
            let enable_sounds = {
                let lock = engine.settings.lock().unwrap();
                lock.enable_sounds
            };
            if enable_sounds {
                os_integration::play_sound("stop");
            }
        }
        if let Some(cb) = &*engine.on_status_change.lock().unwrap() {
            cb(AppState::Transcribing);
        }

        engine.log("\n🛑 Recording stopped (Hold-to-Talk released)! Processing...");
        let (res_tx, res_rx) = crossbeam_channel::bounded(1);
        if let Err(e) = recorder_tx.send(RecorderCommand::Stop(res_tx)) {
            engine.log(format!("❌ Failed to stop recording: {:?}", e));
            engine.update_state(AppState::Idle);
            return Ok(());
        }

        let engine_proc = Arc::clone(&engine);
        tokio::spawn(async move {
            let audio_res = tokio::task::spawn_blocking(move || {
                res_rx.recv().context("Failed to receive audio from recorder thread")
            }).await;
            match audio_res {
                Ok(Ok(Ok(pcm))) => engine_proc.run_processing_pipeline(pcm).await,
                _ => {
                    engine_proc.log("❌ Failed to retrieve audio buffer");
                    engine_proc.update_state(AppState::Idle);
                }
            }
        });
        Ok(())
    }

    async fn run_processing_pipeline(self: &Arc<Self>, audio_data: Vec<f32>) {
        self.log(format!("VoiceAssistantEngine::run_processing_pipeline called (audio data length={})", audio_data.len()));
        let engine_blocking = Arc::clone(self);

        // Snapshot settings atomically — lock held for nanoseconds
        let settings_snapshot = {
            let lock = self.settings.lock().unwrap();
            Arc::clone(&*lock)
        };

        let active_agent_id = {
            let aid = self.active_agent_id.lock().unwrap();
            aid.clone().unwrap_or_default()
        };

        let active_agent = settings_snapshot.agents.iter()
            .find(|a| a.id == active_agent_id)
            .cloned()
            .unwrap_or_else(|| {
                settings_snapshot.agents.first().cloned().unwrap_or_else(|| {
                    AppSettings::default().agents.remove(0)
                })
            });

        self.log(format!("🧠 Whisper speech recognition (STT lang: {})...", active_agent.stt_language));

        let settings_clone = Arc::clone(&settings_snapshot);
        let active_agent_id_clone = active_agent_id.clone();
        let process_res = tokio::task::spawn_blocking(move || -> Result<Option<(String, String)>> {
            let s = &settings_clone;
            let agent = s.agents.iter()
                .find(|a| a.id == active_agent_id_clone)
                .cloned()
                .unwrap_or_else(|| {
                    s.agents.first().cloned().unwrap_or_else(|| {
                        AppSettings::default().agents.remove(0)
                    })
                });

            // --- 1. Audio Energy (RMS) Check ---
            let rms = if audio_data.is_empty() {
                0.0
            } else {
                (audio_data.iter().map(|&x| x * x).sum::<f32>() / audio_data.len() as f32).sqrt()
            };
            if rms < 0.002 {
                engine_blocking.log(format!(
                    "🔇 [speech-filter] Audio energy too low (RMS = {:.6}, threshold = 0.002). Aborting.",
                    rms
                ));
                return Ok(None);
            }
            // -----------------------------------

            let whisper_ctx = engine_blocking.get_or_load_whisper()?;
            let mut whisper_state = whisper_ctx
                .create_state()
                .context("Failed to create Whisper state")?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
            let lang = agent.stt_language.as_str();
            params.set_language(if lang == "auto" { None } else { Some(lang) });
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);

            let prompt_for_speak = settings::load_prompt_for_speak();
            let mut final_whisper_prompt = agent.whisper_prompt.trim().to_string();
            if !prompt_for_speak.trim().is_empty() {
                if !final_whisper_prompt.is_empty() {
                    final_whisper_prompt.push_str(", ");
                }
                final_whisper_prompt.push_str(&prompt_for_speak);
            }
            if !final_whisper_prompt.trim().is_empty() {
                params.set_initial_prompt(&final_whisper_prompt);
            }

            whisper_state
                .full(params, &audio_data)
                .context("Error processing Whisper audio")?;

            let num_segments = whisper_state
                .full_n_segments()
                .context("Error getting Whisper segments")?;

            let mut raw_transcript = String::new();
            for i in 0..num_segments {
                let text = whisper_state
                    .full_get_segment_text(i)
                    .context("Error reading Whisper segment")?;
                raw_transcript.push_str(text.trim());
                raw_transcript.push(' ');
            }

            let raw_transcript = raw_transcript.trim().to_string();
            let raw_transcript = strip_audio_annotations(&raw_transcript);
            if raw_transcript.is_empty() {
                return Ok(None);
            }

            // --- 2. Transcript Validation ---
            if !is_valid_speech_transcript(&raw_transcript) {
                engine_blocking.log(format!(
                    "⚠️ [speech-filter] Discarded invalid speech/music transcript: {:?}",
                    raw_transcript
                ));
                return Ok(None);
            }
            // ---------------------------------

            let refiner_path = &engine_blocking.refiner_bin_path;
            if !refiner_path.exists() {
                engine_blocking.log("⏳ Compiling sidecar llm_refiner...");
                let build_status = std::process::Command::new("cargo")
                    .args(["build", "--bin", "llm_refiner"])
                    .status()
                    .context("Failed to build llm_refiner")?;
                if !build_status.success() {
                    anyhow::bail!("Failed to compile llm_refiner");
                }
                if let Some(parent) = refiner_path.parent() {
                    std::fs::create_dir_all(parent).context("Failed to create bin directory")?;
                }
                std::fs::copy("target/debug/llm_refiner", refiner_path)
                    .context("Failed to copy compiled llm_refiner to bin/llm_refiner")?;
            }

            let system_prompt = build_llm_prompt(&agent);
            let model_name = engine_blocking.gemma_model_path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Gemma 4".to_string());
            engine_blocking.log(format!("🔮 Prompt for LLM (Model: {}):\n{}", model_name, system_prompt));

            engine_blocking.update_state(AppState::Refining);
            engine_blocking.log(format!("🔮 Refining with local Gemma 4 (target lang: {})...", agent.target_language));
            engine_blocking.log(format!("🔮 Launching llm_refiner sidecar using model: {}", model_name));
            let mut child = std::process::Command::new(refiner_path)
                .arg(&engine_blocking.gemma_model_path)
                .arg(&system_prompt)          // argv[2]: dynamic system prompt
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .context("Failed to launch llm_refiner")?;

            {
                use std::io::Write;
                let mut stdin = child
                    .stdin
                    .take()
                    .context("Failed to open subprocess stdin")?;
                stdin
                    .write_all(raw_transcript.as_bytes())
                    .context("Failed to write to stdin")?;
            }

            let output = child
                .wait_with_output()
                .context("Error waiting for Gemma 4 subprocess")?;
            let refined = String::from_utf8_lossy(&output.stdout);
            let refined = strip_quotes(&refined);

            Ok(Some((raw_transcript, refined)))
        })
        .await;
        match process_res {
            Ok(Ok(Some((raw_text, refined_text)))) => {
                self.update_state(AppState::Success);
                let enable_sounds = {
                    let lock = self.settings.lock().unwrap();
                    lock.enable_sounds
                };
                if enable_sounds {
                    os_integration::play_sound("complete");
                }
                // Write successful translation/refinement to SQLite history log
                let active_id = {
                    let aid = self.active_agent_id.lock().unwrap();
                    aid.clone().unwrap_or_default()
                };
                if let Err(e) = self.db.insert_log(&raw_text, &refined_text, &active_id) {
                    self.log(format!("❌ Failed to write record to history database: {:?}", e));
                }

                self.log("\n📋 Processing results:");
                self.log("-----------------------------------");
                self.log(format!("📝 Original: \"{}\"", raw_text));
                self.log(format!("✨ Refined: \"{}\"", refined_text));
                self.log("-----------------------------------");

                let auto_paste = settings_snapshot.enable_auto_paste;
                if auto_paste {
                    let cb_opt = {
                        let guard = self.on_paste.lock().unwrap();
                        guard.clone()
                    };

                    if let Some(cb) = cb_opt {
                        self.log("⌨️ Launching input emulation on main thread...");
                        cb(refined_text.clone());
                    } else {
                        self.log("⌨️ Text input emulation...");
                        if let Err(e) = os_integration::inject_text(&refined_text) {
                            self.log(format!("❌ Input emulation error: {:?}", e));
                        }
                    }
                } else {
                    // Copy to clipboard fallback without keyboard simulation
                    match arboard::Clipboard::new() {
                        Ok(mut clipboard) => {
                            self.log("✏️ Writing refined text to clipboard...");
                            if let Err(e) = clipboard.set_text(refined_text.clone()) {
                                self.log(format!("❌ Clipboard write error: {:?}", e));
                            } else {
                                self.log("✨ Text successfully saved to clipboard! Paste it manually (Cmd+V).");
                            }
                        }
                        Err(e) => {
                            self.log(format!("❌ Failed to initialize clipboard manager: {:?}", e));
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
            Ok(Ok(None)) => {
                self.log("⚠️ Speech was not recognized or the recognized buffer is empty.");
            }
            Ok(Err(e)) => {
                self.log(format!("❌ Error during transcription/refinement: {:?}", e));
            }
            Err(e) => {
                self.log(format!("❌ Error executing background task: {:?}", e));
            }
        }

        self.update_state(AppState::Idle);
        self.log("\n🟢 App ready! Hold hotkey to record.");
    }
}

fn strip_audio_annotations(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_bracket = 0;
    let mut last_was_space = true; // Prevents leading spaces

    for c in text.chars() {
        match c {
            '[' => in_bracket += 1,
            ']' => {
                if in_bracket > 0 {
                    in_bracket -= 1;
                }
            }
            _ => {
                if in_bracket == 0 {
                    if c.is_whitespace() {
                        if !last_was_space {
                            result.push(' ');
                            last_was_space = true;
                        }
                    } else {
                        result.push(c);
                        last_was_space = false;
                    }
                }
            }
        }
    }
    
    // Trim trailing space if any
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

fn clean_transcript(text: &str) -> String {
    let mut cleaned = String::new();
    let mut in_bracket = 0;
    for c in text.chars() {
        match c {
            '[' | '(' | '{' => in_bracket += 1,
            ']' | ')' | '}' => {
                if in_bracket > 0 {
                    in_bracket -= 1;
                }
            }
            _ => {
                if in_bracket == 0 {
                    cleaned.push(c);
                }
            }
        }
    }
    cleaned
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn is_hallucination(cleaned: &str) -> bool {
    matches!(
        cleaned,
        "thankyou"
            | "thankyouforwatching"
            | "thanksforwatching"
            | "subtitlesby"
            | "amaraorg"
            | "you"
            | "bye"
            | "goodbye"
            | "m"
            | "h"
            | "s"
    )
}

fn is_valid_speech_transcript(text: &str) -> bool {
    let cleaned = clean_transcript(text);
    if cleaned.is_empty() {
        return false;
    }
    if is_hallucination(&cleaned) {
        return false;
    }
    true
}

/// Assembles the full Gemma 4 turn-template prompt from the active agent profile.
fn build_llm_prompt(agent: &settings::AgentProfile) -> String {
    let config = settings::load_gemma_prompts();
    
    let preset_prompt = if agent.preset_id == "None" || agent.preset_id.trim().is_empty() {
        "".to_string()
    } else {
        config.presets.iter()
            .find(|p| p.id == agent.preset_id)
            .map(|p| p.prompt.clone())
            .unwrap_or_default()
    };
    
    let target_lang = if agent.target_language.trim().is_empty() || agent.target_language == "No Translation" {
        "the original language".to_string()
    } else {
        agent.target_language.trim().to_string()
    };

    let base_prompt = config.default_prompt.replace("{{TARGET_LANGUAGE}}", &target_lang);
    
    let mut prompt_parts = Vec::new();
    prompt_parts.push(base_prompt);
    
    if !preset_prompt.trim().is_empty() {
        prompt_parts.push(preset_prompt.trim().to_string());
    }
    
    if !agent.custom_prompt.trim().is_empty() {
        prompt_parts.push(agent.custom_prompt.trim().to_string());
    }
    
    prompt_parts.join("\n")
}

/// Resolves model and helper binary paths dynamically across various execution directory structures.
fn resolve_path(relative: &str) -> PathBuf {
    // 1. Check relative to current working directory
    let cwd_path = PathBuf::from(relative);
    if cwd_path.exists() {
        return cwd_path;
    }

    // 2. Check one level up (if running inside src-tauri folder context)
    let parent_relative = format!("../{}", relative);
    let parent_path = PathBuf::from(&parent_relative);
    if parent_path.exists() {
        return parent_path;
    }

    // 3. Check relative to current executable directory
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // Direct check relative to executable
            let exe_rel = exe_dir.join(relative);
            if exe_rel.exists() {
                return exe_rel;
            }
            if let Some(exe_parent) = exe_dir.parent() {
                let exe_parent_rel = exe_parent.join(relative);
                if exe_parent_rel.exists() {
                    return exe_parent_rel;
                }
            }

            // 4. Check Tauri dev mode resources (target/debug/resources/...)
            let dev_resources = exe_dir.join("resources").join(relative);
            if dev_resources.exists() {
                return dev_resources;
            }

            // 5. Check macOS App Bundle resources (Contents/Resources/...)
            if let Some(exe_parent) = exe_dir.parent() {
                // If Tauri copied as _up_/...
                let bundle_resources_up = exe_parent.join("Resources").join("_up_").join(relative);
                if bundle_resources_up.exists() {
                    return bundle_resources_up;
                }

                // If Tauri copied directly to Resources/
                let bundle_resources_direct = exe_parent.join("Resources").join(relative);
                if bundle_resources_direct.exists() {
                    return bundle_resources_direct;
                }

                // If Tauri copied and flattened the path (e.g. just the filename)
                if let Some(filename) = PathBuf::from(relative).file_name() {
                    let bundle_resources_flat = exe_parent.join("Resources").join(filename);
                    if bundle_resources_flat.exists() {
                        return bundle_resources_flat;
                    }
                }
            }
        }
    }

    // Fallback to direct relative path
    cwd_path
}

/// Helper to recursively strip matching quotes added by LLMs
pub fn strip_quotes(s: &str) -> String {
    let mut current = s.trim().to_string();
    loop {
        let trimmed = current.trim();
        if trimmed.is_empty() {
            break;
        }

        let first = trimmed.chars().next().unwrap();
        let last = trimmed.chars().last().unwrap();

        let is_quote = match (first, last) {
            ('"', '"') => true,
            ('\'', '\'') => true,
            ('“', '”') => true,
            ('‘', '’') => true,
            ('«', '»') => true,
            _ => false,
        };

        if is_quote {
            let mut chars = trimmed.chars();
            chars.next();
            chars.next_back();
            current = chars.as_str().to_string();
        } else {
            break;
        }
    }
    current.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("\"Hello\""), "Hello");
        assert_eq!(strip_quotes("\"\"Hello\"\""), "Hello");
        assert_eq!(strip_quotes("“Hello”"), "Hello");
        assert_eq!(strip_quotes("«Hello»"), "Hello");
        assert_eq!(strip_quotes("'Hello'"), "Hello");
        assert_eq!(strip_quotes("\"'Hello'\""), "Hello");
        assert_eq!(strip_quotes("  \"Hello\"  "), "Hello");
        assert_eq!(strip_quotes("Hello"), "Hello");
        assert_eq!(strip_quotes("\"Hello"), "\"Hello");
        assert_eq!(strip_quotes("Hello\""), "Hello\"");
    }

    #[test]
    fn test_resolve_path() {
        // Fallback for non-existent path
        let non_existent = resolve_path("non_existent_file_xyz.bin");
        assert_eq!(non_existent, PathBuf::from("non_existent_file_xyz.bin"));

        // Resolve existing file in working directory
        let cargo_toml = resolve_path("Cargo.toml");
        assert!(cargo_toml.exists());
        assert_eq!(cargo_toml, PathBuf::from("Cargo.toml"));
    }

    #[test]
    fn test_clean_transcript() {
        assert_eq!(clean_transcript("Hello [music] world!"), "helloworld");
        assert_eq!(clean_transcript("[music]"), "");
        assert_eq!(clean_transcript("(sigh)"), "");
        assert_eq!(clean_transcript("..."), "");
        assert_eq!(clean_transcript("♪ ♪ ♪"), "");
    }

    #[test]
    fn test_is_valid_speech_transcript() {
        assert!(is_valid_speech_transcript("Hello there!"));
        assert!(!is_valid_speech_transcript("[music]"));
        assert!(!is_valid_speech_transcript("Thank you."));
        assert!(!is_valid_speech_transcript("you"));
        assert!(!is_valid_speech_transcript("..."));
        assert!(!is_valid_speech_transcript("♪"));
    }

    #[test]
    fn test_strip_audio_annotations() {
        assert_eq!(strip_audio_annotations("hello [music] world"), "hello world");
        assert_eq!(strip_audio_annotations("[laughter] hello"), "hello");
        assert_eq!(strip_audio_annotations("[music]"), "");
        assert_eq!(strip_audio_annotations("hello [music] [laughter] world"), "hello world");
        assert_eq!(strip_audio_annotations("hello world"), "hello world");
        assert_eq!(strip_audio_annotations("  hello   [sigh]   world  "), "hello world");
    }
}
