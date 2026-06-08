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
    #[serde(default = "default_voice_hotkey_type")]
    pub voice_hotkey_type: String,
    #[serde(default = "default_voice_hotkey_value")]
    pub voice_hotkey_value: String,
    #[serde(default = "default_voice_stt_language")]
    pub voice_stt_language: String,
    #[serde(default = "default_voice_whisper_prompt")]
    pub voice_whisper_prompt: String,
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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            agents: vec![AgentProfile {
                id: "slack_refiner".to_string(),
                name: "Slack Refiner".to_string(),
                target_language: "No Translation".to_string(),
                preset_id: "workspace_sync".to_string(),
                custom_prompt: "".to_string(),
                hotkey_type: "Keyboard".to_string(),
                hotkey_value: "".to_string(),
                is_active: true,
            }],
            enable_auto_paste: true,
            enable_sounds: true,
            enable_gemma: default_false(),
            voice_hotkey_type: default_voice_hotkey_type(),
            voice_hotkey_value: default_voice_hotkey_value(),
            voice_stt_language: default_voice_stt_language(),
            voice_whisper_prompt: default_voice_whisper_prompt(),
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
    serde_json::from_str::<GemmaPromptsConfig>(DEFAULT_GEMMA_PROMPTS_JSON)
        .expect("Embedded gemma_prompts.json must be valid JSON")
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
        assert_eq!(settings.agents.len(), 1); // slack default
        assert_eq!(settings.agents[0].name, "Slack Refiner");
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
    }

    #[test]
    fn test_load_prompt_for_speak() {
        let terms = load_prompt_for_speak();
        assert!(terms.contains("JSON"));
        assert!(terms.contains("HTML"));
    }
}
