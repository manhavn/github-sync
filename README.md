# GitSync Daemon

A secure, background synchronization service written in Rust that automatically mirrors and syncs all repositories (both public and private) from a GitHub account to your local machine. It comes with a built-in interactive Web UI dashboard.

🇻🇳 [Xem phiên bản Tiếng Việt tại đây (Vietnamese version)](README.vi.md)

---

## ✨ Features

- **Background Daemon Mode**: Runs silently in the background (`-b` / `--background`) with PID file tracking and stdout/stderr logging.
- **Glassmorphic Web UI**: A beautiful, modern, dark-mode dashboard (built with responsive Vanilla HTML/CSS/JS and embedded directly into the compiled binary).
- **Secure Authentication**: Uses an ephemeral Git credential helper rather than storing your Personal Access Token (PAT) inside `.git/config` files on disk.
- **Automatic Organization**: Saves repositories to a clean hierarchy of `[configured_path]/[owner_username]/[repo_name]`.
- **REST API Control**: Endpoints to inspect synchronization statuses, track errors, view log history, and trigger manual sync cycles.
- **Multiple Sync Modes**:
  - **Force Full Sync**: Connects to GitHub API and updates all repositories (clones missing, pulls existing).
  - **Sync Missing Only**: Connects to GitHub API but only clones missing repositories (skips existing ones to save time).
  - **Pull Updates Only (Offline API)**: Bypasses GitHub API rate limits by scanning your local directory and pulling updates for existing repos offline relative to the API.

---

## 🛠️ Requirements

- **Rust & Cargo** (v1.65+)
- **Git** installed on the local system
- **Build Tools** (`gcc`/`g++`, `pkg-config`, `openssl-dev`)

---

## 🚀 Installation & Compilation

1. Clone the repository and navigate into it.
2. Build the project in release mode:
   ```bash
   cargo build --release
   ```
3. Copy the compiled binary to your system path:
   ```bash
   cp target/release/gitsync /usr/local/bin/gitsync
   ```

---

## 💻 CLI Commands

The `gitsync` binary supports the following commands:

### 1. Configure settings
Set your GitHub credentials, destination path, check interval, and Web UI server port:
```bash
gitsync config --username <github_username> --token <github_pat> --path <local_sync_path> --interval 3600 --port 9090
```

### 2. Start the daemon
Start the server. Use `--background` or `-b` to run it as a detached background service:
```bash
gitsync start --background
```
*Once started, access the Web UI dashboard at `http://127.0.0.1:9090`.*

### 3. Check status
Verify if the background process is running, and see the latest logs and file paths:
```bash
gitsync status
```

### 4. Trigger manual synchronization
Force an immediate sync. If the background daemon is active, it handles it; otherwise, it runs a foreground sync:
```bash
gitsync sync
```

### 5. Stop the background daemon
```bash
gitsync stop
```

---

## 🎨 Web UI Dashboard Endpoints

When the daemon is running, it exposes the following endpoints:
- `GET /` - Serves the HTML dashboard.
- `GET /api/status` - Returns the JSON object of the shared state (repo tracking, logs).
- `POST /api/config` - Updates configuration and saves it to disk.
- `POST /api/sync` - Triggers an immediate manual sync cycle.

---

## 📂 Configuration Paths

All configuration, PID tracking, and logs are stored inside:
- **Config JSON**: `~/.config/gitsync/config.json`
- **PID File**: `~/.config/gitsync/gitsync.pid`
- **Log File**: `~/.config/gitsync/gitsync.log`

To monitor background activities:
```bash
tail -f ~/.config/gitsync/gitsync.log
```

---

## 📄 License

This project is licensed under the MIT License.
