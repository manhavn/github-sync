use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use chrono::Utc;
use serde::Deserialize;
use crate::state::{SyncState, RepoStatus};

#[derive(Deserialize, Debug, Clone)]
struct GithubRepo {
    name: String,
    full_name: String,
    clone_url: String,
    private: bool,
    owner: GithubOwner,
}

#[derive(Deserialize, Debug, Clone)]
struct GithubOwner {
    login: String,
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
            // Get sync interval
            let interval_secs = {
                let s = self.state.read().await;
                s.config.sync_interval_secs
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
                s.status = "Error".to_string();
                s.add_log("ERROR", &format!("Sync failed: {}", e));
            }
        }
    }

    pub async fn perform_sync(&self) -> Result<(), String> {
        let (username, token, local_path) = {
            let s = self.state.read().await;
            (
                s.config.github_username.clone(),
                s.config.github_token.clone(),
                s.config.local_path.clone(),
            )
        };

        if username.is_empty() || token.is_empty() {
            let mut s = self.state.write().await;
            s.add_log("WARN", "GitHub username or token is not configured. Sync skipped.");
            s.status = "Idle".to_string();
            return Ok(());
        }

        if local_path.is_empty() {
            let mut s = self.state.write().await;
            s.add_log("WARN", "Local destination path is not configured. Sync skipped.");
            s.status = "Idle".to_string();
            return Ok(());
        }

        {
            let mut s = self.state.write().await;
            s.status = "Syncing".to_string();
            s.add_log("INFO", "Starting GitHub repositories sync...");
        }

        // Fetch all repos from GitHub
        let repos = match fetch_all_repos(&username, &token).await {
            Ok(r) => r,
            Err(e) => {
                let mut s = self.state.write().await;
                s.status = "Error".to_string();
                s.add_log("ERROR", &format!("Failed to fetch repository list from GitHub: {}", e));
                return Err(e);
            }
        };

        {
            let mut s = self.state.write().await;
            s.add_log("INFO", &format!("Found {} repositories on GitHub.", repos.len()));
            // Sync internal list in state
            for gr in &repos {
                if !s.repos.iter().any(|r| r.full_name == gr.full_name) {
                    s.repos.push(RepoStatus {
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
            let repo_name = &gr.name;
            let owner_name = &gr.owner.login;
            let full_name = &gr.full_name;
            
            // local path: base_path / owner / repo
            let mut repo_dir = local_base_path.clone();
            repo_dir.push(owner_name);
            repo_dir.push(repo_name);

            {
                let mut s = self.state.write().await;
                if let Some(r) = s.repos.iter_mut().find(|r| r.full_name == *full_name) {
                    r.status = if repo_dir.exists() { "Pulling".to_string() } else { "Cloning".to_string() };
                }
                s.add_log("INFO", &format!("[{}/{}] Syncing {} (Private: {})...", idx + 1, repos.len(), full_name, gr.private));
            }

            let sync_result = sync_repository(&repo_dir, &gr.clone_url, &token).await;

            {
                let mut s = self.state.write().await;
                if let Some(r) = s.repos.iter_mut().find(|r| r.full_name == *full_name) {
                    r.last_sync = Some(Utc::now());
                    match &sync_result {
                        Ok(_) => {
                            r.status = "Success".to_string();
                            r.error = None;
                            s.add_log("INFO", &format!("Successfully synced {}", full_name));
                        }
                        Err(err_msg) => {
                            r.status = "Failed".to_string();
                            r.error = Some(err_msg.clone());
                            s.add_log("ERROR", &format!("Failed to sync {}: {}", full_name, err_msg));
                        }
                    }
                }
            }
        }

        {
            let mut s = self.state.write().await;
            s.status = "Idle".to_string();
            s.last_sync_time = Some(Utc::now());
            s.add_log("INFO", "All repositories sync cycle completed.");
        }

        Ok(())
    }
}

async fn fetch_all_repos(_username: &str, token: &str) -> Result<Vec<GithubRepo>, String> {
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

async fn sync_repository(repo_dir: &Path, clone_url: &str, token: &str) -> Result<(), String> {
    let helper_val = format!("!f() {{ echo username=x-access-token; echo password={}; }}; f", token);
    
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
            // Let's try git checkout default branch and reset if pull failed due to local conflicts/divergences,
            // but for now, we will return the pull error as is to let the user know.
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
