use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use chrono::{DateTime, Utc};
use crate::config::Config;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RepoStatus {
    pub name: String,
    pub full_name: String,
    pub status: String, // "Idle", "Cloning", "Pulling", "Success", "Failed"
    pub error: Option<String>,
    pub last_sync: Option<DateTime<Utc>>,
    pub is_private: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogMessage {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum SyncMode {
    Full,
    MissingOnly,
    UpdatesOnly,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncState {
    pub config: Config,
    pub status: String, // "Idle", "Syncing", "Error"
    pub last_sync_time: Option<DateTime<Utc>>,
    pub repos: Vec<RepoStatus>,
    pub logs: VecDeque<LogMessage>,
    pub next_sync_mode: SyncMode,
}

impl SyncState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            status: "Idle".to_string(),
            last_sync_time: None,
            repos: Vec::new(),
            logs: VecDeque::with_capacity(100),
            next_sync_mode: SyncMode::Full,
        }
    }

    pub fn add_log(&mut self, level: &str, msg: &str) {
        if self.logs.len() >= 100 {
            self.logs.pop_front();
        }
        self.logs.push_back(LogMessage {
            timestamp: Utc::now(),
            level: level.to_string(),
            message: msg.to_string(),
        });
        println!("[{}] [{}] {}", Utc::now().to_rfc3339(), level, msg);
    }
}
