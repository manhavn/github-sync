use std::fs::File;
use std::path::PathBuf;
use daemonize::Daemonize;
use crate::config::{get_pid_path, get_log_path};

pub fn daemonize_process(cwd: PathBuf) -> Result<(), String> {
    let pid_file = get_pid_path();
    let log_file_path = get_log_path();

    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create daemon config directory: {}", e))?;
    }

    let log_file = File::create(&log_file_path)
        .map_err(|e| format!("Failed to create daemon log file at {:?}: {}", log_file_path, e))?;

    let daemonize = Daemonize::new()
        .pid_file(pid_file)
        .chown_pid_file(true)
        .working_directory(cwd)
        .stdout(log_file.try_clone().map_err(|e| format!("Failed to clone log file handle: {}", e))?)
        .stderr(log_file);

    match daemonize.start() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Error daemonizing: {}", e)),
    }
}

pub fn stop_daemon() -> Result<(), String> {
    let pid_path = get_pid_path();
    if !pid_path.exists() {
        return Err("Daemon is not running (PID file not found).".to_string());
    }

    let pid_str = std::fs::read_to_string(&pid_path)
        .map_err(|e| format!("Failed to read PID file: {}", e))?;
    let pid: i32 = pid_str.trim().parse()
        .map_err(|e| format!("Invalid PID in PID file: {}", e))?;

    let output = std::process::Command::new("kill")
        .arg("-15") // SIGTERM
        .arg(pid.to_string())
        .output()
        .map_err(|e| format!("Failed to execute kill command: {}", e))?;

    if output.status.success() {
        println!("Sent termination signal to daemon process (PID {}).", pid);
        let mut attempts = 0;
        while attempts < 10 {
            let check = std::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .output();
            match check {
                Ok(out) if !out.status.success() => {
                    let _ = std::fs::remove_file(&pid_path);
                    println!("Daemon stopped successfully.");
                    return Ok(());
                }
                _ => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    attempts += 1;
                }
            }
        }
        
        let _ = std::process::Command::new("kill")
            .arg("-9") // SIGKILL
            .arg(pid.to_string())
            .output();
        let _ = std::fs::remove_file(&pid_path);
        println!("Daemon did not respond to SIGTERM. Sent SIGKILL (forced stop).");
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        if err.contains("No such process") {
            let _ = std::fs::remove_file(&pid_path);
            return Err("Daemon process was not found. Cleaned up stale PID file.".to_string());
        }
        Err(format!("Failed to terminate process: {}", err))
    }
}

pub fn get_daemon_status() -> Result<(String, Option<i32>), String> {
    let pid_path = get_pid_path();
    if !pid_path.exists() {
        return Ok(("Stopped".to_string(), None));
    }

    let pid_str = std::fs::read_to_string(&pid_path)
        .map_err(|e| format!("Failed to read PID file: {}", e))?;
    let pid: i32 = pid_str.trim().parse()
        .map_err(|e| format!("Invalid PID in PID file: {}", e))?;

    let check = std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output();

    match check {
        Ok(out) if out.status.success() => {
            Ok(("Running".to_string(), Some(pid)))
        }
        _ => {
            let _ = std::fs::remove_file(&pid_path);
            Ok(("Stopped (Stale PID file cleaned up)".to_string(), None))
        }
    }
}
