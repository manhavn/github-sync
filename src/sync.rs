use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use chrono::Utc;
use serde::Deserialize;
use crate::state::{SyncState, RepoStatus, ProfileSyncState};

#[derive(Deserialize, Debug, Clone)]
struct GithubRepo {
    name: String,
    full_name: String,
    clone_url: String,
    private: bool,
}

pub struct SyncWorker {
    state: Arc<RwLock<SyncState>>,
    trigger: Arc<Notify>,
}

impl SyncWorker {
    pub fn new(state: Arc<RwLock<SyncState>>, trigger: Arc<Notify>) -> Self {
        Self { state, trigger }
    }

    pub async fn run_loop(self) {
        loop {
            // Get active profile sync interval
            let interval_secs = {
                let s = self.state.read().await;
                if let Some(profile) = s.get_active_profile() {
                    profile.sync_interval_secs
                } else {
                    3600 // Default fallback if no active profile
                }
            };

            // Wait for interval or trigger
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)) => {
                    // Normal timeout sync
                }
                _ = self.trigger.notified() => {
                    // Triggered sync (manual or config update)
                }
            }

            if let Err(e) = self.perform_sync().await {
                let mut s = self.state.write().await;
                let active_id = s.active_profile_id.clone();
                if !active_id.is_empty() {
                    let p_state = s.profile_states.entry(active_id).or_insert_with(ProfileSyncState::new);
                    p_state.status = "Error".to_string();
                    p_state.add_log("ERROR", &format!("Sync failed: {}", e));
                }
            }
        }
    }

    pub async fn perform_sync(&self) -> Result<(), String> {
        let (profile, sync_mode) = {
            let mut s = self.state.write().await;
            let mode = s.next_sync_mode;
            s.next_sync_mode = crate::state::SyncMode::Full; // Reset
            (s.get_active_profile().cloned(), mode)
        };

        let profile = match profile {
            Some(p) => p,
            None => {
                let mut s = self.state.write().await;
                s.add_log_to_active("WARN", "No active sync profile configured. Sync skipped.");
                return Ok(());
            }
        };

        if profile.username.is_empty() || profile.token.is_empty() {
            let mut s = self.state.write().await;
            s.add_log_to_active("WARN", &format!("Credentials not configured for profile '{}'. Sync skipped.", profile.name));
            return Ok(());
        }

        if profile.local_path.is_empty() {
            let mut s = self.state.write().await;
            s.add_log_to_active("WARN", &format!("Local storage path not configured for profile '{}'. Sync skipped.", profile.name));
            return Ok(());
        }

        let profile_id = profile.id.clone();
        let provider = profile.provider.clone();
        let domain = profile.domain.clone();
        let username = profile.username.clone();
        let token = profile.token.clone();
        let local_path = profile.local_path.clone();

        {
            let mut s = self.state.write().await;
            let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
            p_state.status = "Syncing".to_string();
            p_state.add_log("INFO", &format!("Starting sync for profile '{}' ({}) (Mode: {:?})...", profile.name, provider, sync_mode));
        }

        // Resolve repository list based on sync mode
        let repos = if sync_mode == crate::state::SyncMode::UpdatesOnly {
            discover_local_repos(&local_path).await
        } else {
            let fetch_result = if provider.to_lowercase() == "gitlab" {
                fetch_all_repos_gitlab(&domain, &token).await
            } else {
                fetch_all_repos_github(&token).await
            };

            match fetch_result {
                Ok(r) => r,
                Err(e) => {
                    let mut s = self.state.write().await;
                    let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
                    p_state.status = "Error".to_string();
                    p_state.add_log("ERROR", &format!("Failed to fetch repository list: {}", e));
                    return Err(e);
                }
            }
        };

        {
            let mut s = self.state.write().await;
            let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
            p_state.add_log("INFO", &format!("Found {} repositories to process.", repos.len()));
            
            // Sync internal list in profile state
            for gr in &repos {
                if !p_state.repos.iter().any(|r| r.full_name == gr.full_name) {
                    p_state.repos.push(RepoStatus {
                        name: gr.name.clone(),
                        full_name: gr.full_name.clone(),
                        status: "Idle".to_string(),
                        error: None,
                        last_sync: None,
                        is_private: gr.private,
                    });
                }
            }
        }

        let local_base_path = PathBuf::from(&local_path);
        if !local_base_path.exists() {
            tokio::fs::create_dir_all(&local_base_path)
                .await
                .map_err(|e| format!("Failed to create local path dir {}: {}", local_path, e))?;
        }

        for (idx, gr) in repos.iter().enumerate() {
            let full_name = &gr.full_name;
            
            // local path: base_path / full_name (natively handles nested groups)
            let repo_dir = local_base_path.join(full_name);

            // If mode is MissingOnly, skip if the directory already exists
            if repo_dir.exists() && sync_mode == crate::state::SyncMode::MissingOnly {
                let mut s = self.state.write().await;
                let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
                p_state.add_log("INFO", &format!("[{}/{}] Skipping {} (already exists locally).", idx + 1, repos.len(), full_name));
                if let Some(r) = p_state.repos.iter_mut().find(|r| r.full_name == *full_name) {
                    r.status = "Success".to_string();
                }
                continue;
            }

            {
                let mut s = self.state.write().await;
                let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
                if let Some(r) = p_state.repos.iter_mut().find(|r| r.full_name == *full_name) {
                    r.status = if repo_dir.exists() { "Pulling".to_string() } else { "Cloning".to_string() };
                }
                p_state.add_log("INFO", &format!("[{}/{}] Syncing {} (Private: {})...", idx + 1, repos.len(), full_name, gr.private));
            }

            let sync_result = sync_repository(&repo_dir, &gr.clone_url, &username, &token).await;

            {
                let mut s = self.state.write().await;
                let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
                if let Some(r) = p_state.repos.iter_mut().find(|r| r.full_name == *full_name) {
                    r.last_sync = Some(Utc::now());
                    match &sync_result {
                        Ok(_) => {
                            r.status = "Success".to_string();
                            r.error = None;
                            p_state.add_log("INFO", &format!("Successfully synced {}", full_name));
                        }
                        Err(err_msg) => {
                            r.status = "Failed".to_string();
                            r.error = Some(err_msg.clone());
                            p_state.add_log("ERROR", &format!("Failed to sync {}: {}", full_name, err_msg));
                        }
                    }
                }
            }
        }

        {
            let mut s = self.state.write().await;
            let p_state = s.profile_states.entry(profile_id.clone()).or_insert_with(ProfileSyncState::new);
            p_state.status = "Idle".to_string();
            p_state.last_sync_time = Some(Utc::now());
            p_state.add_log("INFO", "Sync cycle completed.");
        }

        Ok(())
    }
}

async fn fetch_all_repos_github(token: &str) -> Result<Vec<GithubRepo>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut repos = Vec::new();
    let mut page = 1;
    
    loop {
        let url = format!("https://api.github.com/user/repos?per_page=100&page={}", page);
        
        let response = client.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "gitsync-daemon")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| format!("Request error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("GitHub API returned status code {}: {}", status, body));
        }

        let page_repos: Vec<GithubRepo> = response.json()
            .await
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        if page_repos.is_empty() {
            break;
        }

        let count = page_repos.len();
        repos.extend(page_repos);
        
        if count < 100 {
            break;
        }
        page += 1;
    }

    Ok(repos)
}

async fn fetch_all_repos_gitlab(domain: &str, token: &str) -> Result<Vec<GithubRepo>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut repos = Vec::new();
    let mut page = 1;
    
    let base_url = if domain.starts_with("http://") || domain.starts_with("https://") {
        domain.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", domain.trim_end_matches('/'))
    };

    loop {
        let url = format!("{}/api/v4/projects?membership=true&per_page=100&page={}", base_url, page);
        
        let response = client.get(&url)
            .header("PRIVATE-TOKEN", token)
            .header("User-Agent", "gitsync-daemon")
            .send()
            .await
            .map_err(|e| format!("Request error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("GitLab API returned status code {}: {}", status, body));
        }

        #[derive(Deserialize)]
        struct GitlabProject {
            name: String,
            path_with_namespace: String,
            http_url_to_repo: String,
            visibility: String,
        }

        let projects: Vec<GitlabProject> = response.json()
            .await
            .map_err(|e| format!("Failed to parse GitLab response JSON: {}", e))?;

        if projects.is_empty() {
            break;
        }

        let count = projects.len();
        for p in projects {
            repos.push(GithubRepo {
                name: p.name,
                full_name: p.path_with_namespace,
                clone_url: p.http_url_to_repo,
                private: p.visibility != "public",
            });
        }
        
        if count < 100 {
            break;
        }
        page += 1;
    }

    Ok(repos)
}

async fn discover_local_repos(local_path: &str) -> Vec<GithubRepo> {
    let mut repos = Vec::new();
    let base_path = Path::new(local_path);
    let mut owner_dirs = match tokio::fs::read_dir(base_path).await {
        Ok(dirs) => dirs,
        Err(_) => return repos,
    };

    while let Some(owner_entry) = owner_dirs.next_entry().await.ok().flatten() {
        let owner_path = owner_entry.path();
        if !owner_path.is_dir() {
            continue;
        }
        let owner_name = match owner_path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let mut repo_dirs = match tokio::fs::read_dir(&owner_path).await {
            Ok(dirs) => dirs,
            Err(_) => continue,
        };

        while let Some(repo_entry) = repo_dirs.next_entry().await.ok().flatten() {
            let repo_path = repo_entry.path();
            if !repo_path.is_dir() {
                continue;
            }
            let repo_name = match repo_path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            let mut git_dir = repo_path.clone();
            git_dir.push(".git");
            if git_dir.exists() {
                repos.push(GithubRepo {
                    name: repo_name.clone(),
                    full_name: format!("{}/{}", owner_name, repo_name),
                    clone_url: format!("https://github.com/{}/{}.git", owner_name, repo_name),
                    private: false,
                });
            }
        }
    }
    repos
}

async fn sync_repository(repo_dir: &Path, clone_url: &str, username: &str, token: &str) -> Result<(), String> {
    let helper_val = format!("!f() {{ echo username={}; echo password={}; }}; f", username, token);
    
    if repo_dir.exists() {
        // Run git fetch
        let fetch_output = tokio::process::Command::new("git")
            .arg("-c")
            .arg(format!("credential.helper={}", helper_val))
            .current_dir(repo_dir)
            .args(&["fetch", "--all"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .map_err(|e| format!("Failed to execute git fetch: {}", e))?;

        if !fetch_output.status.success() {
            let err = String::from_utf8_lossy(&fetch_output.stderr).into_owned();
            return Err(format!("git fetch failed: {}", err));
        }

        // Run git pull
        let pull_output = tokio::process::Command::new("git")
            .arg("-c")
            .arg(format!("credential.helper={}", helper_val))
            .current_dir(repo_dir)
            .args(&["pull"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .map_err(|e| format!("Failed to execute git pull: {}", e))?;

        if !pull_output.status.success() {
            let err = String::from_utf8_lossy(&pull_output.stderr).into_owned();
            return Err(format!("git pull failed: {}", err));
        }
    } else {
        // Ensure parent dir exists
        if let Some(parent) = repo_dir.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create repo parent dir: {}", e))?;
        }

        // Run git clone
        let clone_output = tokio::process::Command::new("git")
            .arg("-c")
            .arg(format!("credential.helper={}", helper_val))
            .args(&["clone", clone_url, repo_dir.to_str().ok_or("Invalid path string")?])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .await
            .map_err(|e| format!("Failed to execute git clone: {}", e))?;

        if !clone_output.status.success() {
            let err = String::from_utf8_lossy(&clone_output.stderr).into_owned();
            return Err(format!("git clone failed: {}", err));
        }
    }

    Ok(())
}
