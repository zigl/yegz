//! Terminal Drawer & Project Tasks Runner.
//! Supports running shell commands, npm scripts (from package.json), cargo tasks, and Makefiles.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTask {
    pub name: String,
    pub command: String,
    pub source: &'static str,
}

#[derive(Debug, Clone)]
pub struct TerminalLine {
    pub text: String,
    pub kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Stdout,
    Stderr,
    Info,
    Success,
    Error,
}

pub enum ProcessMessage {
    Line(TerminalLine),
    Finished(Option<i32>),
}

pub struct TerminalDrawer {
    pub open: bool,
    pub height: f32,
    pub lines: Vec<TerminalLine>,
    pub input: String,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub autoscroll: bool,

    // Active task / running child
    pub running_command: Option<String>,
    kill_flag: Arc<AtomicBool>,
    child_handle: Option<Child>,
    rx: Receiver<ProcessMessage>,
    tx: Sender<ProcessMessage>,
}

impl TerminalDrawer {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            open: false,
            height: 220.0,
            lines: Vec::with_capacity(1000),
            input: String::new(),
            history: Vec::new(),
            history_idx: None,
            autoscroll: true,

            running_command: None,
            kill_flag: Arc::new(AtomicBool::new(false)),
            child_handle: None,
            rx,
            tx,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn is_running(&self) -> bool {
        self.running_command.is_some()
    }

    pub fn stop(&mut self) {
        self.kill_flag.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.child_handle.take() {
            let _ = child.kill();
        }
        if let Some(cmd) = self.running_command.take() {
            self.lines.push(TerminalLine {
                text: format!("[Process '{cmd}' stopped by user]"),
                kind: LineKind::Error,
            });
        }
    }

    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ProcessMessage::Line(line) => {
                    self.lines.push(line);
                    if self.lines.len() > 3000 {
                        self.lines.drain(0..500);
                    }
                }
                ProcessMessage::Finished(code) => {
                    self.running_command = None;
                    self.child_handle = None;
                    match code {
                        Some(0) => {
                            self.lines.push(TerminalLine {
                                text: "[Process exited successfully (code 0)]".into(),
                                kind: LineKind::Success,
                            });
                        }
                        Some(c) => {
                            self.lines.push(TerminalLine {
                                text: format!("[Process exited with error code {c}]"),
                                kind: LineKind::Error,
                            });
                        }
                        None => {
                            self.lines.push(TerminalLine {
                                text: "[Process terminated]".into(),
                                kind: LineKind::Info,
                            });
                        }
                    }
                }
            }
        }
    }

    pub fn execute(&mut self, cmd_str: &str, working_dir: Option<&Path>) {
        let trimmed = cmd_str.trim();
        if trimmed.is_empty() {
            return;
        }

        // If a task is currently running, terminate it before starting another
        if self.is_running() {
            self.stop();
        }

        // Add to history
        if self.history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
        }
        self.history_idx = None;

        self.running_command = Some(trimmed.to_string());
        self.open = true;

        let dir = working_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        self.lines.push(TerminalLine {
            text: format!("❯ {}  (in {})", trimmed, dir.display()),
            kind: LineKind::Info,
        });

        let kill_flag = Arc::new(AtomicBool::new(false));
        self.kill_flag = kill_flag.clone();
        let tx = self.tx.clone();

        // Spawn shell process
        let process = Command::new("sh")
            .arg("-c")
            .arg(trimmed)
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match process {
            Ok(c) => c,
            Err(e) => {
                self.lines.push(TerminalLine {
                    text: format!("Failed to spawn command: {e}"),
                    kind: LineKind::Error,
                });
                self.running_command = None;
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Stdout reader thread
        if let Some(out) = stdout {
            let tx_out = tx.clone();
            let kf = kill_flag.clone();
            thread::spawn(move || {
                let reader = BufReader::new(out);
                for line in reader.lines() {
                    if kf.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(l) = line {
                        let cleaned = strip_ansi(&l);
                        let _ = tx_out.send(ProcessMessage::Line(TerminalLine {
                            text: cleaned,
                            kind: LineKind::Stdout,
                        }));
                    }
                }
            });
        }

        // Stderr reader thread
        if let Some(err) = stderr {
            let tx_err = tx.clone();
            let kf = kill_flag.clone();
            thread::spawn(move || {
                let reader = BufReader::new(err);
                for line in reader.lines() {
                    if kf.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(l) = line {
                        let cleaned = strip_ansi(&l);
                        let _ = tx_err.send(ProcessMessage::Line(TerminalLine {
                            text: cleaned,
                            kind: LineKind::Stderr,
                        }));
                    }
                }
            });
        }

        // Wait thread
        thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| s.code());
            let _ = tx.send(ProcessMessage::Finished(code));
        });
    }
}

/// Detects runnable project tasks from `package.json`, `Cargo.toml`, or `Makefile`.
pub fn detect_project_tasks(root: Option<&Path>) -> Vec<ProjectTask> {
    let Some(root) = root else { return Vec::new() };
    let mut tasks = Vec::new();

    // 1. package.json scripts (npm / yarn / pnpm)
    let pkg_json_path = root.join("package.json");
    if pkg_json_path.exists() {
        if let Ok(contents) = fs::read_to_string(&pkg_json_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(scripts) = val.get("scripts").and_then(|s| s.as_object()) {
                    let mut sorted_scripts: BTreeMap<&str, &serde_json::Value> = scripts.iter().map(|(k, v)| (k.as_str(), v)).collect();

                    // Prioritize dev/start/build/test
                    let priority = ["dev", "start", "build", "test", "lint"];
                    for p in priority {
                        if let Some(_) = sorted_scripts.remove(p) {
                            tasks.push(ProjectTask {
                                name: format!("npm run {p}"),
                                command: format!("npm run {p}"),
                                source: "package.json",
                            });
                        }
                    }

                    for (name, _) in sorted_scripts {
                        tasks.push(ProjectTask {
                            name: format!("npm run {name}"),
                            command: format!("npm run {name}"),
                            source: "package.json",
                        });
                    }
                }
            }
        }
    }

    // 2. Cargo.toml tasks
    let cargo_toml_path = root.join("Cargo.toml");
    if cargo_toml_path.exists() {
        tasks.push(ProjectTask {
            name: "cargo check".into(),
            command: "cargo check".into(),
            source: "Cargo.toml",
        });
        tasks.push(ProjectTask {
            name: "cargo test".into(),
            command: "cargo test".into(),
            source: "Cargo.toml",
        });
        tasks.push(ProjectTask {
            name: "cargo run".into(),
            command: "cargo run".into(),
            source: "Cargo.toml",
        });
        tasks.push(ProjectTask {
            name: "cargo build".into(),
            command: "cargo build --release".into(),
            source: "Cargo.toml",
        });
    }

    // 3. Makefile targets
    let makefile_path = root.join("Makefile");
    if makefile_path.exists() {
        if let Ok(contents) = fs::read_to_string(&makefile_path) {
            for line in contents.lines() {
                if line.starts_with('\t') || line.starts_with('#') || line.starts_with('.') {
                    continue;
                }
                if let Some(target) = line.split(':').next() {
                    let t = target.trim();
                    if !t.is_empty() && !t.contains(' ') && !t.contains('=') {
                        tasks.push(ProjectTask {
                            name: format!("make {t}"),
                            command: format!("make {t}"),
                            source: "Makefile",
                        });
                    }
                }
            }
        }
    }

    tasks
}

/// Strips ANSI escape color codes from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;

    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            in_escape = true;
            i += 2;
            continue;
        }
        if in_escape {
            if chars[i].is_alphabetic() {
                in_escape = false;
            }
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let colored = "\x1b[32mBuild Success\x1b[0m in \x1b[1m200ms\x1b[0m";
        assert_eq!(strip_ansi(colored), "Build Success in 200ms");
    }

    #[test]
    fn test_detect_package_json() {
        let temp_dir = std::env::temp_dir().join("yegz_test_pkg");
        let _ = fs::create_dir_all(&temp_dir);
        let pkg_file = temp_dir.join("package.json");
        fs::write(
            &pkg_file,
            r#"{
            "name": "my-app",
            "scripts": {
                "dev": "vite",
                "build": "vite build",
                "test": "vitest"
            }
        }"#,
        )
        .unwrap();

        let tasks = detect_project_tasks(Some(&temp_dir));
        let task_names: Vec<String> = tasks.into_iter().map(|t| t.name).collect();
        assert!(task_names.contains(&"npm run dev".to_string()));
        assert!(task_names.contains(&"npm run build".to_string()));
        assert!(task_names.contains(&"npm run test".to_string()));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
