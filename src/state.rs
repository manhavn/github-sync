use serde::{Serialize, Deserialize};
use std::collections::{VecDeque, HashMap};
use chrono::{DateTime, Utc};
use crate::config::{Config, SyncProfile};

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProfileSyncState {
    pub status: String, // "Idle", "Syncing", "Error"
    pub last_sync_time: Option<DateTime<Utc>>,
    pub repos: Vec<RepoStatus>,
    pub logs: VecDeque<LogMessage>,
    pub auto_sync: bool,
}

impl ProfileSyncState {
    pub fn new() -> Self {
        Self {
            status: "Idle".to_string(),
            last_sync_time: None,
            repos: Vec::new(),
            logs: VecDeque::with_capacity(100),
            auto_sync: false,
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum SyncMode {
    Full,
    MissingOnly,
    UpdatesOnly,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncState {
    pub profiles: Vec<SyncProfile>,
    pub active_profile_id: String,
    pub profile_states: HashMap<String, ProfileSyncState>,
    pub next_sync_mode: SyncMode,
    pub web_host: String,
    pub web_port: u16,
}

impl SyncState {
    pub fn new(config: Config) -> Self {
        let mut profile_states = HashMap::new();
        for p in &config.profiles {
            profile_states.insert(p.id.clone(), ProfileSyncState::new());
        }
        
        Self {
            profiles: config.profiles,
            active_profile_id: config.active_profile_id,
            profile_states,
            next_sync_mode: SyncMode::Full,
            web_host: config.web_host,
            web_port: config.web_port,
        }
    }

    pub fn get_active_profile(&self) -> Option<&SyncProfile> {
        self.profiles.iter().find(|p| p.id == self.active_profile_id)
    }

    pub fn add_log_to_active(&mut self, level: &str, msg: &str) {
        let active_id = self.active_profile_id.clone();
        if !active_id.is_empty() {
            let state = self.profile_states.entry(active_id).or_insert_with(ProfileSyncState::new);
            state.add_log(level, msg);
        } else {
            println!("[{}] [{}] {}", Utc::now().to_rfc3339(), level, msg);
        }
    }
}
