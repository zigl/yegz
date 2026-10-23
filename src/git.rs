//! Git status integration: branch name, file status colors (modified, untracked, staged).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGitStatus {
    Modified,
    Untracked,
    Staged,
    Deleted,
}

impl FileGitStatus {
    #[allow(dead_code)]
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Untracked => "U",
            Self::Staged => "A",
            Self::Deleted => "D",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitRepoStatus {
    pub branch: Option<String>,
    pub file_statuses: HashMap<PathBuf, FileGitStatus>,
    pub dir_has_changes: HashMap<PathBuf, bool>,
}

impl GitRepoStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh(&mut self, root: Option<&Path>) {
        self.file_statuses.clear();
        self.dir_has_changes.clear();
        self.branch = None;

        let Some(root) = root else { return };

        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain=v1")
            .arg("-b")
            .current_dir(root)
            .output();

        let Ok(output) = output else { return };
        if !output.status.success() {
            return;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.starts_with("## ") {
                // e.g. ## master...origin/master or ## main
                let branch_part = line[3..].split("...").next().unwrap_or("").trim();
                if !branch_part.is_empty() {
                    self.branch = Some(branch_part.to_string());
                }
            } else if line.len() >= 4 {
                let code = &line[..2];
                let file_part = line[3..].trim();
                let path = root.join(file_part);

                let status = if code == "??" {
                    FileGitStatus::Untracked
                } else if code.contains('M') {
                    FileGitStatus::Modified
                } else if code.contains('D') {
                    FileGitStatus::Deleted
                } else if code.contains('A') {
                    FileGitStatus::Staged
                } else {
                    FileGitStatus::Modified
                };

                self.file_statuses.insert(path.clone(), status);

                // Mark ancestor directories as containing changes
                let mut curr = path.parent();
                while let Some(parent) = curr {
                    if parent == root {
                        break;
                    }
                    self.dir_has_changes.insert(parent.to_path_buf(), true);
                    curr = parent.parent();
                }
            }
        }
    }

    pub fn status_for(&self, path: &Path) -> Option<FileGitStatus> {
        self.file_statuses.get(path).copied()
    }

    pub fn dir_modified(&self, path: &Path) -> bool {
        self.dir_has_changes.get(path).copied().unwrap_or(false)
    }
}
