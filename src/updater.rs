//! Auto-updater: checks https://yegz.com/version.json and installs updates.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

pub const VERSION_CHECK_URL: &str = "https://yegz.com/version.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub pub_date: String,
    pub notes: String,
    pub macos_dmg: String,
    pub macos_zip: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading,
    ReadyToRestart { new_app: PathBuf, dest_app: Option<PathBuf> },
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading,
    ReadyToRestart { new_app: PathBuf, dest_app: Option<PathBuf> },
    Error(String),
}

pub struct AutoUpdater {
    pub state: UpdateState,
    pub banner_dismissed: bool,
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
}

impl AutoUpdater {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            state: UpdateState::Idle,
            banner_dismissed: false,
            tx,
            rx,
        }
    }

    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UpdateEvent::Checking => {
                    self.state = UpdateState::Checking;
                }
                UpdateEvent::UpToDate => {
                    self.state = UpdateState::UpToDate;
                }
                UpdateEvent::Available(info) => {
                    self.banner_dismissed = false;
                    self.state = UpdateState::Available(info);
                }
                UpdateEvent::Downloading => {
                    self.state = UpdateState::Downloading;
                }
                UpdateEvent::ReadyToRestart { new_app, dest_app } => {
                    self.state = UpdateState::ReadyToRestart { new_app, dest_app };
                }
                UpdateEvent::Error(err) => {
                    self.state = UpdateState::Error(err);
                }
            }
        }
    }

    pub fn check_for_updates(&mut self, current_version: &'static str) {
        self.state = UpdateState::Checking;
        let tx = self.tx.clone();

        thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Checking);

            // Fetch version.json using curl
            let output = Command::new("curl")
                .arg("-s")
                .arg("-m")
                .arg("6")
                .arg("-H")
                .arg("Accept: application/json")
                .arg(VERSION_CHECK_URL)
                .output();

            let out = match output {
                Ok(o) if o.status.success() => o.stdout,
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).to_string();
                    let _ = tx.send(UpdateEvent::Error(format!("Network error checking for updates: {err}")));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(format!("Failed to run update check: {e}")));
                    return;
                }
            };

            let json_str = match String::from_utf8(out) {
                Ok(s) => s,
                Err(_) => {
                    let _ = tx.send(UpdateEvent::Error("Invalid response from update server".into()));
                    return;
                }
            };

            let info: UpdateInfo = match serde_json::from_str(&json_str) {
                Ok(i) => i,
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(format!("Failed to parse version manifest: {e}")));
                    return;
                }
            };

            if is_newer_version(current_version, &info.version) {
                let _ = tx.send(UpdateEvent::Available(info));
            } else {
                let _ = tx.send(UpdateEvent::UpToDate);
            }
        });
    }

    pub fn download_and_install(&mut self, info: UpdateInfo) {
        self.state = UpdateState::Downloading;
        let tx = self.tx.clone();

        thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Downloading);

            let dl_dir = PathBuf::from("/tmp/yegz_update");
            let _ = std::fs::remove_dir_all(&dl_dir);
            if let Err(e) = std::fs::create_dir_all(&dl_dir) {
                let _ = tx.send(UpdateEvent::Error(format!("Failed to create update directory: {e}")));
                return;
            }

            let zip_path = dl_dir.join("update.zip");
            let extract_dir = dl_dir.join("extracted");

            // Download ZIP
            let curl_res = Command::new("curl")
                .arg("-L")
                .arg("-s")
                .arg("-o")
                .arg(&zip_path)
                .arg(&info.macos_zip)
                .output();

            match curl_res {
                Ok(o) if o.status.success() => {}
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).to_string();
                    let _ = tx.send(UpdateEvent::Error(format!("Download failed: {err}")));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(format!("Failed to start download: {e}")));
                    return;
                }
            }

            // Extract with ditto (preserves macOS app bundle codesign and permissions)
            let _ = std::fs::create_dir_all(&extract_dir);
            let ditto_res = Command::new("ditto")
                .arg("-xk")
                .arg(&zip_path)
                .arg(&extract_dir)
                .output();

            match ditto_res {
                Ok(o) if o.status.success() => {}
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).to_string();
                    let _ = tx.send(UpdateEvent::Error(format!("Extraction failed: {err}")));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(format!("Failed to run ditto: {e}")));
                    return;
                }
            }

            let new_app = extract_dir.join("Yegz.app");
            if !new_app.exists() {
                let _ = tx.send(UpdateEvent::Error("Downloaded update archive was missing Yegz.app".into()));
                return;
            }

            let dest_app = current_app_bundle();
            let _ = tx.send(UpdateEvent::ReadyToRestart { new_app, dest_app });
        });
    }

    pub fn apply_and_restart(&mut self) -> Result<(), String> {
        let (new_app, dest_app) = match &self.state {
            UpdateState::ReadyToRestart { new_app, dest_app } => (new_app.clone(), dest_app.clone()),
            _ => return Err("No update ready to install".into()),
        };

        if let Some(dest) = dest_app {
            // macOS bundle replacement script:
            // Wait briefly for current process to terminate, swap bundle, codesign ad-hoc if needed, and relaunch.
            let script = format!(
                r#"
                sleep 0.5
                rm -rf "{dest_display}"
                mv "{new_display}" "{dest_display}"
                open -n "{dest_display}"
                "#,
                dest_display = dest.display(),
                new_display = new_app.display()
            );

            let _ = Command::new("sh")
                .arg("-c")
                .arg(&script)
                .spawn();

            std::process::exit(0);
        } else {
            // Running as standalone binary (e.g. `cargo run` or terminal): launch downloaded app
            let _ = Command::new("open")
                .arg("-n")
                .arg(&new_app)
                .spawn();

            std::process::exit(0);
        }
    }
}

/// Locates the enclosing `.app` bundle of the running binary, if any.
pub fn current_app_bundle() -> Option<PathBuf> {
    let mut cur = std::env::current_exe().ok()?;
    while let Some(parent) = cur.parent() {
        if cur.extension().map_or(false, |ext| ext == "app") {
            return Some(cur);
        }
        cur = parent.to_path_buf();
    }
    None
}

/// Compares two semver strings (e.g. "0.1.1" and "0.1.2").
/// Returns true if `remote` is strictly newer than `current`.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|part| part.parse().ok())
            .collect()
    };

    let c = parse(current);
    let r = parse(remote);

    for (c_num, r_num) in c.iter().zip(r.iter()) {
        if r_num > c_num {
            return true;
        }
        if r_num < c_num {
            return false;
        }
    }

    r.len() > c.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_comparisons() {
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(is_newer_version("0.1.1", "0.1.2"));
        assert!(is_newer_version("0.1.1", "0.2.0"));
        assert!(is_newer_version("0.1.1", "1.0.0"));
        assert!(is_newer_version("v0.1.1", "v0.1.2"));

        assert!(!is_newer_version("0.1.1", "0.1.1"));
        assert!(!is_newer_version("0.1.2", "0.1.1"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
    }

    #[test]
    fn test_parse_version_manifest() {
        let sample = r#"{
            "version": "0.1.1",
            "pub_date": "2026-10-24",
            "notes": "Test notes",
            "macos_dmg": "https://yegz.com/downloads/Yegz-0.1.1-macos-universal.dmg",
            "macos_zip": "https://yegz.com/downloads/Yegz-0.1.1-macos-universal.zip"
        }"#;
        let info: UpdateInfo = serde_json::from_str(sample).unwrap();
        assert_eq!(info.version, "0.1.1");
        assert_eq!(info.macos_dmg, "https://yegz.com/downloads/Yegz-0.1.1-macos-universal.dmg");
    }
}
