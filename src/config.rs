use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncProfile {
    pub id: String,
    pub name: String,
    pub provider: String, // "github" | "gitlab"
    pub domain: String, // e.g. "github.com" or "gitlab.myvps.com"
    pub username: String,
    pub token: String,
    pub local_path: String,
    pub sync_interval_secs: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebToken {
    pub name: String,
    pub token: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub profiles: Vec<SyncProfile>,
    #[serde(default)]
    pub active_profile_id: String,
    #[serde(default = "default_web_host")]
    pub web_host: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
    #[serde(default)]
    pub web_tokens: Vec<WebToken>,
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
            profiles: Vec::new(),
            active_profile_id: String::new(),
            web_host: default_web_host(),
            web_port: default_web_port(),
            web_tokens: Vec::new(),
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
            
        let v: serde_json::Value = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))?;
            
        // Migrate old configuration format
        if v.get("profiles").is_none() {
            let github_username = v.get("github_username").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let github_token = v.get("github_token").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let local_path = v.get("local_path").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let sync_interval_secs = v.get("sync_interval_secs").and_then(|x| x.as_u64()).unwrap_or(3600);
            let web_host = v.get("web_host").and_then(|x| x.as_str()).unwrap_or("127.0.0.1").to_string();
            let web_port = v.get("web_port").and_then(|x| x.as_u64()).unwrap_or(9090) as u16;
            
            let default_profile = SyncProfile {
                id: "default-github".to_string(),
                name: "Default GitHub".to_string(),
                provider: "github".to_string(),
                domain: "github.com".to_string(),
                username: github_username,
                token: github_token,
                local_path,
                sync_interval_secs,
            };
            
            let migrated = Self {
                profiles: vec![default_profile],
                active_profile_id: "default-github".to_string(),
                web_host,
                web_port,
                web_tokens: Vec::new(),
            };
            
            let _ = migrated.save();
            return Ok(migrated);
        }

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
