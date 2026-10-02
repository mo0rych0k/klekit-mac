use std::fs::{self, File};
use std::io::{Write, Read};
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use anyhow::{Context, Result};

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
    #[serde(default = "default_false")]
    pub enable_gemma: bool,
    #[serde(default = "default_true")]
    pub save_history: bool,
    /// Number of days to retain history. Zero keeps history until the user clears it.
    #[serde(default)]
    pub history_retention_days: u32,
    #[serde(default = "default_voice_hotkey_type")]
    pub voice_hotkey_type: String,
    #[serde(default = "default_voice_hotkey_value")]
    pub voice_hotkey_value: String,
    #[serde(default = "default_voice_stt_language")]
    pub voice_stt_language: String,
    #[serde(default = "default_voice_whisper_prompt")]
    pub voice_whisper_prompt: String,
    #[serde(default = "default_voice_max_recording_minutes")]
    pub voice_max_recording_minutes: u8,
    #[serde(default)]
    pub whisper_model_path: String,
    #[serde(default)]
    pub gemma_model_path: String,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_voice_hotkey_type() -> String {
    "Keyboard".to_string()
}

fn default_voice_hotkey_value() -> String {
    "".to_string()
}

fn default_voice_stt_language() -> String {
    "auto".to_string()
}

fn default_voice_whisper_prompt() -> String {
    "".to_string()
}

fn default_voice_max_recording_minutes() -> u8 { 2 }

pub fn default_agents() -> Vec<AgentProfile> {
    vec![AgentProfile {
        id: "default_refiner".to_string(),
        name: "Text Refiner".to_string(),
        target_language: "No Translation".to_string(),
        preset_id: "None".to_string(),
        custom_prompt: String::new(),
        hotkey_type: "Keyboard".to_string(),
        hotkey_value: String::new(),
        is_active: true,
    }]
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            agents: default_agents(),
            enable_auto_paste: true,
            enable_sounds: true,
            enable_gemma: default_false(),
            save_history: default_true(),
            history_retention_days: 0,
            voice_hotkey_type: default_voice_hotkey_type(),
            voice_hotkey_value: default_voice_hotkey_value(),
            voice_stt_language: default_voice_stt_language(),
            voice_whisper_prompt: default_voice_whisper_prompt(),
            voice_max_recording_minutes: default_voice_max_recording_minutes(),
            whisper_model_path: String::new(),
            gemma_model_path: String::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Translation {
    pub title: String,
    pub subtitle: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PresetBlueprint {
    pub id: String,
    pub icon: String,
    pub translations: std::collections::HashMap<String, Translation>,
    pub prompt: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct GemmaPromptsConfig {
    pub default_prompt: String,
    pub presets: Vec<PresetBlueprint>,
}

const DEFAULT_GEMMA_PROMPTS_JSON: &str = include_str!("../resources/gemma_prompts.json");
const DEFAULT_PROMPT_FOR_SPEAK_TXT: &str = include_str!("../resources/prompt_for_speak.txt");

pub fn load_gemma_prompts() -> GemmaPromptsConfig {
    let mut config = serde_json::from_str::<GemmaPromptsConfig>(DEFAULT_GEMMA_PROMPTS_JSON)
        .expect("Embedded gemma_prompts.json must be valid JSON");
    config.presets.retain(|preset| preset.id != "workspace_sync");
    config
}

pub fn load_prompt_for_speak() -> String {
    DEFAULT_PROMPT_FOR_SPEAK_TXT.trim().to_string()
}


impl AppSettings {
    pub fn config_path() -> Result<PathBuf> {
        let mut path = if cfg!(target_os = "windows") {
            let appdata = std::env::var("APPDATA").context("Failed to resolve APPDATA directory")?;
            PathBuf::from(appdata)
        } else {
            let home = std::env::var("HOME").context("Failed to resolve HOME directory")?;
            PathBuf::from(home).join("Library").join("Application Support")
        };
        path.push("klekit");
        fs::create_dir_all(&path).context("Failed to create application directories")?;
        path.push("config.json");
        Ok(path)
    }

    pub fn load() -> Self {
        match Self::config_path() {
            Ok(path) => {
                if path.exists() {
                    if let Ok(mut file) = File::open(&path) {
                        let mut contents = String::new();
                        if file.read_to_string(&mut contents).is_ok() {
                            if let Ok(settings) = serde_json::from_str::<AppSettings>(&contents) {
                                return settings;
                            }
                        }
                    }
                }
                let default = Self::default();
                let _ = default.save();
                default
            }
            Err(_) => {
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let serialized = serde_json::to_string_pretty(self).context("Failed to serialize AppSettings")?;
        let mut file = File::create(&path).context("Failed to write/create config file")?;
        file.write_all(serialized.as_bytes()).context("Failed to write to file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_agent_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.agents.len(), 1);
        assert_eq!(settings.agents[0].preset_id, "None");
        assert!(settings.save_history);
        assert_eq!(settings.history_retention_days, 0);
        assert_eq!(settings.voice_max_recording_minutes, 2);
        assert!(settings.whisper_model_path.is_empty());
        assert!(settings.gemma_model_path.is_empty());
    }

    #[test]
    fn test_load_gemma_prompts() {
        let prompts = load_gemma_prompts();
        assert!(!prompts.default_prompt.is_empty());
        assert!(!prompts.presets.is_empty());
        
        // Ensure "None" and "fix_errors_only" are present
        let has_none = prompts.presets.iter().any(|p| p.id == "None");
        let has_fix = prompts.presets.iter().any(|p| p.id == "fix_errors_only");
        assert!(has_none);
        assert!(has_fix);
        assert_eq!(prompts.presets.len(), 5);
    }

    #[test]
    fn test_load_prompt_for_speak() {
        let terms = load_prompt_for_speak();
        assert!(terms.contains("JSON"));
        assert!(terms.contains("HTML"));
    }
}
