//! Comprehensive Git integration:
//! - Repository status (branch, staged & unstaged changes, untracked files)
//! - Gutter change detection (added, modified, deleted lines via `git diff -U0`)
//! - Source control actions (stage, unstage, commit, push, pull, discard)
//! - Branch management (list branches, checkout, create)
//! - Diff parser for the interactive diff viewer

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGitStatus {
    Modified,
    Untracked,
    Staged,
    Deleted,
    Renamed,
}

impl FileGitStatus {
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Untracked => "U",
            Self::Staged => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: PathBuf,
    pub rel_path: String,
    pub status: FileGitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterChangeType {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GutterChange {
    pub line_start: usize, // 1-indexed
    pub line_count: usize,
    pub change_type: GutterChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineType {
    Context,
    Added,
    Deleted,
    Header,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub line_type: DiffLineType,
    pub old_line_num: Option<usize>,
    pub new_line_num: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitRepoStatus {
    pub branch: Option<String>,
    pub file_statuses: HashMap<PathBuf, FileGitStatus>,
    pub dir_has_changes: HashMap<PathBuf, bool>,
    pub staged: Vec<GitFileChange>,
    pub unstaged: Vec<GitFileChange>,
}

impl GitRepoStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh(&mut self, root: Option<&Path>) {
        self.file_statuses.clear();
        self.dir_has_changes.clear();
        self.staged.clear();
        self.unstaged.clear();
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
                // e.g. ## main...origin/main or ## main
                let branch_part = line[3..].split("...").next().unwrap_or("").trim();
                if !branch_part.is_empty() {
                    self.branch = Some(branch_part.to_string());
                }
            } else if line.len() >= 4 {
                let bytes = line.as_bytes();
                let staged_code = bytes[0] as char;
                let unstaged_code = bytes[1] as char;
                let file_part = line[3..].trim();

                // If renamed: "old -> new", take the new path
                let file_part = if file_part.contains(" -> ") {
                    file_part.split(" -> ").nth(1).unwrap_or(file_part).trim()
                } else {
                    file_part
                };

                let path = root.join(file_part);
                let rel_path = file_part.to_string();

                // 1. Staged change (first column)
                if staged_code != ' ' && staged_code != '?' {
                    let status = match staged_code {
                        'M' => FileGitStatus::Modified,
                        'A' => FileGitStatus::Staged,
                        'D' => FileGitStatus::Deleted,
                        'R' => FileGitStatus::Renamed,
                        _ => FileGitStatus::Staged,
                    };
                    self.staged.push(GitFileChange {
                        path: path.clone(),
                        rel_path: rel_path.clone(),
                        status,
                    });
                }

                // 2. Unstaged change (second column)
                if unstaged_code != ' ' {
                    let status = match unstaged_code {
                        '?' => FileGitStatus::Untracked,
                        'M' => FileGitStatus::Modified,
                        'D' => FileGitStatus::Deleted,
                        _ => FileGitStatus::Modified,
                    };
                    self.unstaged.push(GitFileChange {
                        path: path.clone(),
                        rel_path: rel_path.clone(),
                        status,
                    });
                }

                // Overall map entry for tree highlighting
                let overall_status = if staged_code != ' ' && staged_code != '?' {
                    FileGitStatus::Staged
                } else if unstaged_code == '?' {
                    FileGitStatus::Untracked
                } else if unstaged_code == 'D' || staged_code == 'D' {
                    FileGitStatus::Deleted
                } else {
                    FileGitStatus::Modified
                };
                self.file_statuses.insert(path.clone(), overall_status);

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

    pub fn total_changes_count(&self) -> usize {
        self.staged.len() + self.unstaged.len()
    }
}

// ---------------------------------------------------------------------------
// Git Commands Execution
// ---------------------------------------------------------------------------

pub fn stage_file(root: &Path, rel_path: &str) -> Result<(), String> {
    let out = Command::new("git")
        .args(["add", "--", rel_path])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn unstage_file(root: &Path, rel_path: &str) -> Result<(), String> {
    let out = Command::new("git")
        .args(["reset", "HEAD", "--", rel_path])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn stage_all(root: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn unstage_all(root: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .args(["reset", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn discard_file_changes(root: &Path, rel_path: &str, is_untracked: bool) -> Result<(), String> {
    if is_untracked {
        let full = root.join(rel_path);
        if full.is_dir() {
            std::fs::remove_dir_all(&full).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(&full).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }

    let out = Command::new("git")
        .args(["checkout", "HEAD", "--", rel_path])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn commit_staged(root: &Path, message: &str) -> Result<String, String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("Commit message cannot be empty".into());
    }

    let out = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn push_to_remote(root: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["push"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{stdout}\n{stderr}").trim().to_string();
        Ok(if combined.is_empty() { "Everything up-to-date".into() } else { combined })
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn pull_from_remote(root: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["pull"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{stdout}\n{stderr}").trim().to_string();
        Ok(if combined.is_empty() { "Already up to date.".into() } else { combined })
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn list_branches(root: &Path) -> Result<(Vec<String>, String), String> {
    let out = Command::new("git")
        .args(["branch", "-a", "--format=%(refname:short)|%(HEAD)"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Err("Not a git repository".into());
    }

    let mut branches = Vec::new();
    let mut current = "main".to_string();

    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.is_empty() {
            continue;
        }
        let name = parts[0].trim();
        if name.starts_with("origin/HEAD") || name.is_empty() {
            continue;
        }
        if parts.get(1).map_or(false, |&h| h.trim() == "*") {
            current = name.to_string();
        }
        if !branches.contains(&name.to_string()) {
            branches.push(name.to_string());
        }
    }

    Ok((branches, current))
}

pub fn checkout_branch(root: &Path, branch: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args(["checkout", branch])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(format!("Switched to branch '{branch}'"))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn create_and_checkout_branch(root: &Path, new_branch: &str) -> Result<String, String> {
    let branch = new_branch.trim();
    if branch.is_empty() {
        return Err("Branch name cannot be empty".into());
    }

    let out = Command::new("git")
        .args(["checkout", "-b", branch])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;

    if out.status.success() {
        Ok(format!("Created and switched to new branch '{branch}'"))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// Gutter Diffs & Full Diff Viewer
// ---------------------------------------------------------------------------

/// Computes gutter change indicators by parsing `git diff -U0 -- <file>`.
pub fn compute_gutter_changes(root: &Path, rel_path: &str) -> Vec<GutterChange> {
    let out = Command::new("git")
        .args(["diff", "-U0", "--", rel_path])
        .current_dir(root)
        .output();

    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout);
    parse_gutter_hunks(&text)
}

/// Parses Unified Diff Hunks into `GutterChange` markers.
pub fn parse_gutter_hunks(diff_text: &str) -> Vec<GutterChange> {
    let mut changes = Vec::new();

    for line in diff_text.lines() {
        if !line.starts_with("@@") {
            continue;
        }

        // e.g. "@@ -10,3 +10,4 @@" or "@@ -5 +5,2 @@" or "@@ -15,2 +15,0 @@"
        let parts: Vec<&str> = line.split("@@").collect();
        if parts.len() < 2 {
            continue;
        }

        let header = parts[1].trim();
        let chunks: Vec<&str> = header.split_whitespace().collect();
        if chunks.len() < 2 {
            continue;
        }

        // old: "-10,3" or "-5"
        // new: "+10,4" or "+5,2"
        let old_spec = chunks[0].trim_start_matches('-');
        let new_spec = chunks[1].trim_start_matches('+');

        let old_count = old_spec.split(',').nth(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
        let mut new_parts = new_spec.split(',');
        let new_start = new_parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
        let new_count = new_parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);

        if old_count == 0 && new_count > 0 {
            // Addition
            changes.push(GutterChange {
                line_start: new_start,
                line_count: new_count,
                change_type: GutterChangeType::Added,
            });
        } else if new_count == 0 {
            // Deletion
            changes.push(GutterChange {
                line_start: new_start.max(1),
                line_count: 1,
                change_type: GutterChangeType::Deleted,
            });
        } else {
            // Modification
            changes.push(GutterChange {
                line_start: new_start,
                line_count: new_count,
                change_type: GutterChangeType::Modified,
            });
        }
    }

    changes
}

/// Runs `git diff HEAD -- <rel_path>` to obtain a line-by-line unified diff.
pub fn get_file_diff(root: &Path, rel_path: &str) -> Vec<DiffLine> {
    let out = Command::new("git")
        .args(["diff", "HEAD", "--", rel_path])
        .current_dir(root)
        .output();

    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    parse_full_diff(&text)
}

/// Parses full unified diff text into lines with line numbers and line types.
pub fn parse_full_diff(diff_text: &str) -> Vec<DiffLine> {
    let mut lines = Vec::new();
    let mut old_num = 0;
    let mut new_num = 0;

    for line in diff_text.lines() {
        if line.starts_with("diff --git") || line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }

        if line.starts_with("@@") {
            // Hunk header: e.g. @@ -1,5 +1,6 @@
            let parts: Vec<&str> = line.split("@@").collect();
            if parts.len() >= 2 {
                let header = parts[1].trim();
                let chunks: Vec<&str> = header.split_whitespace().collect();
                if chunks.len() >= 2 {
                    let old_spec = chunks[0].trim_start_matches('-');
                    let new_spec = chunks[1].trim_start_matches('+');

                    old_num = old_spec.split(',').next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
                    new_num = new_spec.split(',').next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
                }
            }
            lines.push(DiffLine {
                line_type: DiffLineType::Header,
                old_line_num: None,
                new_line_num: None,
                text: line.to_string(),
            });
            continue;
        }

        if line.starts_with('+') {
            lines.push(DiffLine {
                line_type: DiffLineType::Added,
                old_line_num: None,
                new_line_num: Some(new_num),
                text: line[1..].to_string(),
            });
            new_num += 1;
        } else if line.starts_with('-') {
            lines.push(DiffLine {
                line_type: DiffLineType::Deleted,
                old_line_num: Some(old_num),
                new_line_num: None,
                text: line[1..].to_string(),
            });
            old_num += 1;
        } else {
            let content = if line.starts_with(' ') { &line[1..] } else { line };
            lines.push(DiffLine {
                line_type: DiffLineType::Context,
                old_line_num: Some(old_num),
                new_line_num: Some(new_num),
                text: content.to_string(),
            });
            old_num += 1;
            new_num += 1;
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_git_status_badge() {
        assert_eq!(FileGitStatus::Modified.badge(), "M");
        assert_eq!(FileGitStatus::Untracked.badge(), "U");
        assert_eq!(FileGitStatus::Staged.badge(), "A");
        assert_eq!(FileGitStatus::Deleted.badge(), "D");
        assert_eq!(FileGitStatus::Renamed.badge(), "R");
    }

    #[test]
    fn test_repo_status_counts() {
        let mut status = GitRepoStatus::new();
        assert_eq!(status.total_changes_count(), 0);
        status.staged.push(GitFileChange {
            path: PathBuf::from("/a/b"),
            rel_path: "b".into(),
            status: FileGitStatus::Staged,
        });
        status.unstaged.push(GitFileChange {
            path: PathBuf::from("/a/c"),
            rel_path: "c".into(),
            status: FileGitStatus::Modified,
        });
        assert_eq!(status.total_changes_count(), 2);
    }

    #[test]
    fn test_parse_gutter_hunks() {
        let diff = r#"
@@ -58,0 +59,2 @@ pub struct Settings {
+    foo
+    bar
@@ -74,0 +77 @@ impl Default for Settings {
+    baz
@@ -10,3 +10,3 @@ mod tests {
-    old
+    new
@@ -20,2 +20,0 @@
-    del1
-    del2
"#;
        let changes = parse_gutter_hunks(diff);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0], GutterChange { line_start: 59, line_count: 2, change_type: GutterChangeType::Added });
        assert_eq!(changes[1], GutterChange { line_start: 77, line_count: 1, change_type: GutterChangeType::Added });
        assert_eq!(changes[2], GutterChange { line_start: 10, line_count: 3, change_type: GutterChangeType::Modified });
        assert_eq!(changes[3], GutterChange { line_start: 20, line_count: 1, change_type: GutterChangeType::Deleted });
    }

    #[test]
    fn test_parse_full_diff() {
        let diff = r#"diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,4 @@
 line1
-line2
+line2_mod
+line3_add
 line4
"#;
        let lines = parse_full_diff(diff);
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0].line_type, DiffLineType::Header);
        assert_eq!(lines[1].line_type, DiffLineType::Context);
        assert_eq!(lines[2].line_type, DiffLineType::Deleted);
        assert_eq!(lines[3].line_type, DiffLineType::Added);
        assert_eq!(lines[4].line_type, DiffLineType::Added);
        assert_eq!(lines[5].line_type, DiffLineType::Context);
    }
}
