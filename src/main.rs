mod config;
mod daemon;
mod state;
mod sync;
mod web;

use clap::{Parser, Subcommand};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use std::path::PathBuf;

use config::{Config, SyncProfile, get_config_path, get_pid_path, get_log_path};
use daemon::{daemonize_process, stop_daemon, get_daemon_status};
use state::SyncState;
use sync::SyncWorker;
use web::start_web_server;

#[derive(Parser)]
#[command(name = "gitsync")]
#[command(about = "GitSync Daemon - Synchronize and mirror all GitHub/GitLab repos to local storage", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure GitHub/GitLab profiles and storage paths
    Config {
        /// ID of the profile to edit/create (default: active profile or 'default')
        #[arg(long)]
        profile: Option<String>,

        /// Profile display name (e.g. 'Personal GitHub', 'My Self-hosted GitLab')
        #[arg(long)]
        name: Option<String>,

        /// Git provider ('github' or 'gitlab')
        #[arg(long)]
        provider: Option<String>,

        /// Custom domain (default: 'github.com' for GitHub)
        #[arg(long)]
        domain: Option<String>,

        /// Git username
        #[arg(long)]
        username: Option<String>,

        /// Git Personal Access Token (PAT)
        #[arg(long)]
        token: Option<String>,

        /// Local storage path for syncing repositories
        #[arg(long)]
        path: Option<String>,

        /// Sync check interval in seconds (default: 3600)
        #[arg(long)]
        interval: Option<u64>,

        /// Set this profile as active
        #[arg(long)]
        activate: bool,

        /// Web UI server port (global setting, default: 9090)
        #[arg(long)]
        port: Option<u16>,
    },

    /// Start the sync service and Web UI
    Start {
        /// Run in the background as a daemon
        #[arg(long, short = 'b')]
        background: bool,
    },

    /// Stop the background daemon
    Stop,

    /// Check daemon status and configurations
    Status,

    /// Force immediate sync of the active repository profile
    Sync,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config {
            profile,
            name,
            provider,
            domain,
            username,
            token,
            path,
            interval,
            activate,
            port,
        } => {
            let mut cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            let mut updated = false;

            // Handle global port update
            if let Some(prt) = port {
                cfg.web_port = prt;
                updated = true;
            }

            // Resolve which profile ID we are editing
            let profile_id = profile.unwrap_or_else(|| {
                if !cfg.active_profile_id.is_empty() {
                    cfg.active_profile_id.clone()
                } else {
                    "default".to_string()
                }
            });

            // Check if profile fields are specified to edit
            let has_profile_updates = name.is_some()
                || provider.is_some()
                || domain.is_some()
                || username.is_some()
                || token.is_some()
                || path.is_some()
                || interval.is_some()
                || activate;

            if has_profile_updates {
                // Find or create profile
                let index = cfg.profiles.iter().position(|p| p.id == profile_id);
                
                let mut p = match index {
                    Some(idx) => cfg.profiles[idx].clone(),
                    None => SyncProfile {
                        id: profile_id.clone(),
                        name: profile_id.clone(),
                        provider: "github".to_string(),
                        domain: "github.com".to_string(),
                        username: String::new(),
                        token: String::new(),
                        local_path: String::new(),
                        sync_interval_secs: 3600,
                    },
                };

                if let Some(n) = name { p.name = n; }
                if let Some(prv) = provider { p.provider = prv; }
                if let Some(dom) = domain { p.domain = dom; }
                if let Some(u) = username { p.username = u; }
                if let Some(t) = token { p.token = t; }
                if let Some(pth) = path {
                    let path_buf = PathBuf::from(&pth);
                    p.local_path = if path_buf.is_absolute() {
                        pth
                    } else {
                        match std::env::current_dir() {
                            Ok(mut cwd) => {
                                cwd.push(path_buf);
                                cwd.to_string_lossy().into_owned()
                            }
                            Err(_) => pth,
                        }
                    };
                }
                if let Some(i) = interval { p.sync_interval_secs = i; }

                match index {
                    Some(idx) => {
                        cfg.profiles[idx] = p;
                    }
                    None => {
                        cfg.profiles.push(p);
                    }
                }

                if activate || cfg.active_profile_id.is_empty() {
                    cfg.active_profile_id = profile_id.clone();
                }

                updated = true;
                println!("Profile '{}' updated successfully.", profile_id);
            }

            if updated {
                if let Err(e) = cfg.save() {
                    eprintln!("Failed to save config: {}", e);
                    std::process::exit(1);
                }
            }

            // Display configuration
            println!("\nGlobal Web UI Settings ({}):", get_config_path().to_string_lossy());
            println!("  Web Host:          {}", cfg.web_host);
            println!("  Web Port:          {}", cfg.web_port);
            println!("  Active Profile ID: {}", if cfg.active_profile_id.is_empty() { "<none>".to_string() } else { cfg.active_profile_id.clone() });
            
            println!("\nConfigured Sync Profiles:");
            if cfg.profiles.is_empty() {
                println!("  <no profiles configured>");
            } else {
                for p in &cfg.profiles {
                    let active_tag = if p.id == cfg.active_profile_id { " [ACTIVE]" } else { "" };
                    println!("  - Profile ID: {}{}", p.id, active_tag);
                    println!("    Name:       {}", p.name);
                    println!("    Provider:   {} ({})", p.provider, p.domain);
                    println!("    Username:   {}", p.username);
                    println!("    Path:       {}", p.local_path);
                    println!("    Interval:   {} seconds", p.sync_interval_secs);
                    println!();
                }
            }
        }

        Commands::Start { background } => {
            match get_daemon_status() {
                Ok((ref status, Some(pid))) if status == "Running" => {
                    println!("GitSync daemon is already running (PID {}).", pid);
                    return;
                }
                _ => {}
            }

            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to load configuration: {}. Please configure at least one profile first.", e);
                    std::process::exit(1);
                }
            };

            if background {
                println!("Starting GitSync daemon in background...");
                let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/app"));
                if let Err(e) = daemonize_process(current_dir) {
                    eprintln!("Failed to run in background: {}", e);
                    std::process::exit(1);
                }
            } else {
                println!("Starting GitSync service in foreground...");
                println!("Press Ctrl+C to terminate.");
            }

            let web_host = config.web_host.clone();
            let web_port = config.web_port;
            
            let state = Arc::new(RwLock::new(SyncState::new(config)));
            let sync_trigger = Arc::new(Notify::new());

            let state_clone = Arc::clone(&state);
            let trigger_clone = Arc::clone(&sync_trigger);
            
            tokio::spawn(async move {
                let worker = SyncWorker::new(state_clone, trigger_clone);
                worker.run_loop().await;
            });

            println!("Starting Web UI server at http://{}:{}", web_host, web_port);
            if let Err(e) = start_web_server(state, sync_trigger, web_host, web_port).await {
                eprintln!("Web server error: {}", e);
                std::process::exit(1);
            }
        }

        Commands::Stop => {
            println!("Stopping GitSync daemon...");
            match stop_daemon() {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error stopping daemon: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Status => {
            match get_daemon_status() {
                Ok((status, pid_opt)) => {
                    println!("Daemon Status: {}", status);
                    if let Some(pid) = pid_opt {
                        println!("  PID: {}", pid);
                    }
                }
                Err(e) => {
                    println!("Daemon Status: Unknown (Error: {})", e);
                }
            }

            let config_path = get_config_path();
            let pid_path = get_pid_path();
            let log_path = get_log_path();

            println!("\nService Paths:");
            println!("  Config File: {}", config_path.to_string_lossy());
            println!("  PID File:    {}", pid_path.to_string_lossy());
            println!("  Log File:    {}", log_path.to_string_lossy());

            if log_path.exists() {
                println!("\nLast 10 Log Entries:");
                let log_cmd = std::process::Command::new("tail")
                    .arg("-n")
                    .arg("10")
                    .arg(&log_path)
                    .output();
                match log_cmd {
                    Ok(out) => {
                        print!("{}", String::from_utf8_lossy(&out.stdout));
                    }
                    Err(_) => {
                        println!("  <Unable to tail log file>");
                    }
                }
            }
        }

        Commands::Sync => {
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to load configuration: {}", e);
                    std::process::exit(1);
                }
            };

            if config.active_profile_id.is_empty() {
                eprintln!("No active profile is configured. Cannot sync.");
                std::process::exit(1);
            }

            match get_daemon_status() {
                Ok((ref status, _)) if status == "Running" => {
                    println!("Daemon is running. Triggering sync for active profile via API...");
                    let client = reqwest::Client::new();
                    let url = format!("http://{}:{}/api/sync", config.web_host, config.web_port);
                    
                    let res = client.post(&url).send().await;
                    match res {
                        Ok(response) => {
                            if response.status().is_success() {
                                println!("Successfully triggered synchronization for active profile in background daemon.");
                            } else {
                                eprintln!("Daemon returned error status: {}", response.status());
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to connect to daemon API: {}. Is it binding to a different port?", e);
                        }
                    }
                }
                _ => {
                    println!("Daemon is not running. Performing foreground sync on active profile...");
                    
                    let state = Arc::new(RwLock::new(SyncState::new(config)));
                    let sync_trigger = Arc::new(Notify::new());
                    let worker = SyncWorker::new(state, sync_trigger);
                    
                    if let Err(e) = worker.perform_sync().await {
                        eprintln!("Foreground sync failed: {}", e);
                        std::process::exit(1);
                    }
                    println!("Foreground sync finished successfully.");
                }
            }
        }
    }
}
