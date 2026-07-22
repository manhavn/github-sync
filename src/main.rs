mod config;
mod daemon;
mod state;
mod sync;
mod web;

use clap::{Parser, Subcommand};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use std::path::PathBuf;

use config::{Config, get_config_path, get_pid_path, get_log_path};
use daemon::{daemonize_process, stop_daemon, get_daemon_status};
use state::SyncState;
use sync::SyncWorker;
use web::start_web_server;

#[derive(Parser)]
#[command(name = "gitsync")]
#[command(about = "GitSync Daemon - Synchronize and mirror all GitHub repos to local storage", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure GitHub credentials and storage paths
    Config {
        /// GitHub username
        #[arg(long)]
        username: Option<String>,

        /// GitHub Personal Access Token (PAT)
        #[arg(long)]
        token: Option<String>,

        /// Local path where repositories will be synced: [path]/[username]/[repo_name]
        #[arg(long)]
        path: Option<String>,

        /// Sync check interval in seconds (default: 3600)
        #[arg(long)]
        interval: Option<u64>,

        /// Web UI server port (default: 9090)
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

    /// Force immediate sync of all repositories
    Sync,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Config { username, token, path, interval, port } => {
            let mut cfg = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            let mut updated = false;

            if let Some(u) = username {
                cfg.github_username = u;
                updated = true;
            }
            if let Some(t) = token {
                cfg.github_token = t;
                updated = true;
            }
            if let Some(p) = path {
                // Canonicalize if possible to store absolute path
                let path_buf = PathBuf::from(&p);
                cfg.local_path = if path_buf.is_absolute() {
                    p
                } else {
                    match std::env::current_dir() {
                        Ok(mut cwd) => {
                            cwd.push(path_buf);
                            cwd.to_string_lossy().into_owned()
                        }
                        Err(_) => p,
                    }
                };
                updated = true;
            }
            if let Some(i) = interval {
                cfg.sync_interval_secs = i;
                updated = true;
            }
            if let Some(prt) = port {
                cfg.web_port = prt;
                updated = true;
            }

            if updated {
                if let Err(e) = cfg.save() {
                    eprintln!("Failed to save config: {}", e);
                    std::process::exit(1);
                }
                println!("Configuration updated successfully.");
            }

            // Display configuration
            println!("Current Configuration ({}):", get_config_path().to_string_lossy());
            println!("  GitHub Username: {}", if cfg.github_username.is_empty() { "<not set>".to_string() } else { cfg.github_username });
            println!("  GitHub Token:    {}", if cfg.github_token.is_empty() { "<not set>".to_string() } else { "******** (configured)".to_string() });
            println!("  Local Sync Path: {}", if cfg.local_path.is_empty() { "<not set>".to_string() } else { cfg.local_path });
            println!("  Sync Interval:   {} seconds", cfg.sync_interval_secs);
            println!("  Web Port:        {}", cfg.web_port);
        }

        Commands::Start { background } => {
            // Check status first
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
                    eprintln!("Failed to load configuration: {}. Please run 'gitsync config' first.", e);
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
                // Beyond this point, we are in the background daemon process!
            } else {
                println!("Starting GitSync service in foreground...");
                println!("Press Ctrl+C to terminate.");
            }

            // Initialize State and Worker
            let web_host = config.web_host.clone();
            let web_port = config.web_port;
            
            let state = Arc::new(RwLock::new(SyncState::new(config)));
            let sync_trigger = Arc::new(Notify::new());

            // Run first sync in a separate task so it starts immediately
            let state_clone = Arc::clone(&state);
            let trigger_clone = Arc::clone(&sync_trigger);
            
            // Spawn worker
            tokio::spawn(async move {
                let worker = SyncWorker::new(state_clone, trigger_clone);
                // Trigger immediate initial sync
                let _ = worker.perform_sync().await;
                // Enter interval wait loop
                worker.run_loop().await;
            });

            // Start Axum web server
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

            // Print log location information
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

            // Check if daemon is running
            match get_daemon_status() {
                Ok((ref status, _)) if status == "Running" => {
                    // Send POST request to running daemon Web API
                    println!("Daemon is running. Triggering sync via API...");
                    let client = reqwest::Client::new();
                    let url = format!("http://{}:{}/api/sync", config.web_host, config.web_port);
                    
                    let res = client.post(&url).send().await;
                    match res {
                        Ok(response) => {
                            if response.status().is_success() {
                                println!("Successfully triggered synchronization in background daemon.");
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
                    println!("Daemon is not running. Performing foreground sync...");
                    
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
