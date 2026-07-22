use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub github_username: String,
    #[serde(default)]
    pub github_token: String,
    #[serde(default)]
    pub local_path: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u64,
    #[serde(default = "default_web_host")]
    pub web_host: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
}

fn default_sync_interval() -> u64 {
    3600
}

fn default_web_host() -> String {
    "127.0.0.1".to_string()
}

fn default_web_port() -> u16 {
    9090
}

impl Default for Config {
    fn default() -> Self {
        Self {
            github_username: String::new(),
            github_token: String::new(),
            local_path: String::new(),
            sync_interval_secs: default_sync_interval(),
            web_host: default_web_host(),
            web_port: default_web_port(),
        }
    }
}

pub fn get_config_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/root/.config"));
    path.push("gitsync");
    path
}

pub fn get_config_path() -> PathBuf {
    let mut path = get_config_dir();
    path.push("config.json");
    path
}

pub fn get_pid_path() -> PathBuf {
    let mut path = get_config_dir();
    path.push("gitsync.pid");
    path
}

pub fn get_log_path() -> PathBuf {
    let mut path = get_config_dir();
    path.push("gitsync.log");
    path
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = get_config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = get_config_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
        let path = get_config_path();
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(&path, data)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        Ok(())
    }
}
