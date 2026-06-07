use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Serialize, Deserialize};
use std::path::Path;
use crate::settings::AgentProfile;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogRecord {
    pub id: i64,
    pub timestamp: String,
    pub raw_transcription: String,
    pub refined_text: String,
    pub agent_id: String,
}

pub struct DbManager {
    conn: std::sync::Mutex<Connection>,
}

impl DbManager {
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).context("Failed to open SQLite database")?;
        
        // Enable WAL mode & normal synchronization
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;"
        ).context("Failed to set WAL pragmas")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS voice_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                raw_transcription TEXT NOT NULL,
                refined_text TEXT NOT NULL,
                agent_id TEXT NOT NULL
            );",
            [],
        ).context("Failed to create voice_history table")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                stt_language TEXT NOT NULL,
                whisper_prompt TEXT NOT NULL DEFAULT '',
                target_language TEXT NOT NULL,
                preset_id TEXT NOT NULL,
                custom_prompt TEXT NOT NULL,
                hotkey_type TEXT NOT NULL,
                hotkey_value TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            );",
            [],
        ).context("Failed to create agents table")?;

        // Migration check for existing installations: add whisper_prompt column if missing
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN whisper_prompt TEXT NOT NULL DEFAULT '';", []);

        // Provision default agent if table is empty
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agents",
            [],
            |row| row.get(0),
        ).context("Failed to count agents")?;

        if count == 0 {
            conn.execute(
                "INSERT INTO agents (id, name, stt_language, whisper_prompt, target_language, preset_id, custom_prompt, hotkey_type, hotkey_value, is_active)
                 VALUES ('slack_refiner', 'Slack Refiner', 'auto', '', 'No Translation', 'workspace_sync', '', 'Keyboard', '', 1)",
                [],
            ).context("Failed to provision default agent")?;
        }

        Ok(Self { conn: std::sync::Mutex::new(conn) })
    }

    pub fn insert_log(&self, raw: &str, refined: &str, agent_id: &str) -> Result<()> {
         let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO voice_history (raw_transcription, refined_text, agent_id) VALUES (?1, ?2, ?3)",
            [raw, refined, agent_id],
        )?;
        Ok(())
    }

    pub fn get_logs_page(&self, limit: i64, offset: i64) -> Result<Vec<LogRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, strftime('%Y-%m-%d %H:%M:%S', timestamp), raw_transcription, refined_text, agent_id 
             FROM voice_history 
             ORDER BY timestamp DESC 
             LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map([limit, offset], |row| {
            Ok(LogRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                raw_transcription: row.get(2)?,
                refined_text: row.get(3)?,
                agent_id: row.get(4)?,
            })
        })?;
        
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    pub fn delete_log(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM voice_history WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM voice_history", [])?;
        Ok(())
    }

    pub fn get_all_agents(&self) -> Result<Vec<AgentProfile>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, stt_language, whisper_prompt, target_language, preset_id, custom_prompt, hotkey_type, hotkey_value, is_active 
             FROM agents"
        )?;
        let rows = stmt.query_map([], |row| {
            let is_active_int: i32 = row.get(9)?;
            Ok(AgentProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                stt_language: row.get(2)?,
                whisper_prompt: row.get(3)?,
                target_language: row.get(4)?,
                preset_id: row.get(5)?,
                custom_prompt: row.get(6)?,
                hotkey_type: row.get(7)?,
                hotkey_value: row.get(8)?,
                is_active: is_active_int != 0,
            })
        })?;
        
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    pub fn save_agents(&self, agents: &[AgentProfile]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM agents", [])?;
        for agent in agents {
            let is_active_int = if agent.is_active { 1 } else { 0 };
            tx.execute(
                "INSERT INTO agents (id, name, stt_language, whisper_prompt, target_language, preset_id, custom_prompt, hotkey_type, hotkey_value, is_active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    &agent.id,
                    &agent.name,
                    &agent.stt_language,
                    &agent.whisper_prompt,
                    &agent.target_language,
                    &agent.preset_id,
                    &agent.custom_prompt,
                    &agent.hotkey_type,
                    &agent.hotkey_value,
                    is_active_int,
                ),
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_db_operations() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let manager = DbManager::new(&db_path).unwrap();

        // Test insert
        manager.insert_log("raw text", "refined text", "agent_1").unwrap();

        // Test count/retrieval
        let logs = manager.get_logs_page(50, 0).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].raw_transcription, "raw text");
        assert_eq!(logs[0].refined_text, "refined text");
        assert_eq!(logs[0].agent_id, "agent_1");

        // Test delete
        manager.delete_log(logs[0].id).unwrap();
        assert_eq!(manager.get_logs_page(50, 0).unwrap().len(), 0);

        // Test clear
        manager.insert_log("raw 2", "refined 2", "agent_2").unwrap();
        manager.clear_history().unwrap();
        assert_eq!(manager.get_logs_page(50, 0).unwrap().len(), 0);

        // Test agents operations
        let default_agents = manager.get_all_agents().unwrap();
        assert_eq!(default_agents.len(), 1);
        assert_eq!(default_agents[0].id, "slack_refiner");
        assert_eq!(default_agents[0].is_active, true);

        let custom_agent = AgentProfile {
            id: "custom_agent".to_string(),
            name: "Custom".to_string(),
            stt_language: "en".to_string(),
            whisper_prompt: "".to_string(),
            target_language: "No Translation".to_string(),
            preset_id: "None".to_string(),
            custom_prompt: "Test".to_string(),
            hotkey_type: "Keyboard".to_string(),
            hotkey_value: "F9".to_string(),
            is_active: false,
        };

        manager.save_agents(&[default_agents[0].clone(), custom_agent]).unwrap();
        let loaded_agents = manager.get_all_agents().unwrap();
        assert_eq!(loaded_agents.len(), 2);
        assert_eq!(loaded_agents[1].id, "custom_agent");
        assert_eq!(loaded_agents[1].is_active, false);
    }
}
