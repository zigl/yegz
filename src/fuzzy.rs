//! Atom-style Fuzzy File Finder (⌘P).

use std::fs;
use std::path::{Path, PathBuf};

const IGNORED_DIRS: &[&str] = &[
    ".git", "target", "node_modules", ".idea", ".vscode", "build", "dist",
    ".DS_Store", "DerivedData", "vendor",
];

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub path: PathBuf,
    pub relative: String,
    pub filename: String,
    pub score: i64,
}

pub struct FuzzyFinder {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub files: Vec<PathBuf>,
    pub results: Vec<MatchResult>,
    root: Option<PathBuf>,
}

impl FuzzyFinder {
    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            files: Vec::new(),
            results: Vec::new(),
            root: None,
        }
    }

    pub fn set_root(&mut self, root: Option<&Path>) {
        if self.root.as_deref() != root {
            self.root = root.map(|p| p.to_path_buf());
            self.refresh();
        }
    }

    pub fn refresh(&mut self) {
        self.files.clear();
        if let Some(root) = &self.root {
            let mut list = Vec::new();
            collect_files(root, &mut list, 0);
            self.files = list;
        }
        self.update_search();
    }

    pub fn open_palette(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.update_search();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
    }

    pub fn update_search(&mut self) {
        let q = self.query.trim().to_lowercase();
        let Some(root) = &self.root else {
            self.results.clear();
            return;
        };

        if q.is_empty() {
            // Show recent/top files
            self.results = self.files
                .iter()
                .take(50)
                .map(|p| {
                    let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().to_string();
                    let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    MatchResult {
                        path: p.clone(),
                        relative: rel,
                        filename: fname,
                        score: 0,
                    }
                })
                .collect();
            self.selected = 0;
            return;
        }

        let mut scored: Vec<MatchResult> = self.files
            .iter()
            .filter_map(|p| {
                let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().to_string();
                let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let score = fuzzy_score(&q, &rel, &fname)?;
                Some(MatchResult {
                    path: p.clone(),
                    relative: rel,
                    filename: fname,
                    score,
                })
            })
            .collect();

        scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.relative.len().cmp(&b.relative.len())));
        scored.truncate(50);
        self.results = scored;
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }
}

fn collect_files(dir: &Path, list: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 || list.len() > 10000 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if IGNORED_DIRS.contains(&name.as_str()) || (name.starts_with('.') && name != ".env") {
            continue;
        }
        if let Ok(file_type) = entry.file_type() {
            if file_type.is_dir() {
                collect_files(&path, list, depth + 1);
            } else if file_type.is_file() {
                list.push(path);
            }
        }
    }
}

/// Computes a fuzzy matching score. Returns None if the query is not a subsequence.
pub fn fuzzy_score(query: &str, target_path: &str, target_filename: &str) -> Option<i64> {
    let q_chars: Vec<char> = query.chars().collect();
    let t_chars: Vec<char> = target_path.to_lowercase().chars().collect();
    let fname_lower: Vec<char> = target_filename.to_lowercase().chars().collect();

    // Check if filename directly matches first for huge bonus
    let in_filename = is_subsequence(&q_chars, &fname_lower);

    if !is_subsequence(&q_chars, &t_chars) {
        return None;
    }

    let mut score: i64 = 0;
    if in_filename {
        score += 80;
    }

    if target_filename.to_lowercase() == query {
        score += 200;
    }

    let mut t_idx = 0;
    let mut prev_matched_idx = None;

    for &qc in &q_chars {
        while t_idx < t_chars.len() && t_chars[t_idx] != qc {
            t_idx += 1;
        }
        if t_idx >= t_chars.len() {
            return None;
        }

        score += 10;

        // Consecutive match bonus
        if let Some(prev) = prev_matched_idx {
            if t_idx == prev + 1 {
                score += 25;
            }
        }

        // Word boundary bonus: match right after '/', '.', '_', '-'
        if t_idx == 0 || ['/', '.', '_', '-'].contains(&t_chars[t_idx - 1]) {
            score += 30;
        }

        prev_matched_idx = Some(t_idx);
        t_idx += 1;
    }

    // Penalty for length difference to favor tighter matches
    score -= (target_path.len() as i64) / 4;

    Some(score)
}

fn is_subsequence(query: &[char], target: &[char]) -> bool {
    let mut q_idx = 0;
    for &tc in target {
        if q_idx < query.len() && query[q_idx] == tc {
            q_idx += 1;
        }
    }
    q_idx == query.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_score() {
        assert!(fuzzy_score("main", "src/main.rs", "main.rs").is_some());
        assert!(fuzzy_score("mrs", "src/main.rs", "main.rs").is_some());
        assert!(fuzzy_score("xyz", "src/main.rs", "main.rs").is_none());

        let s1 = fuzzy_score("main", "src/main.rs", "main.rs").unwrap();
        let s2 = fuzzy_score("main", "long/path/to/main_test.rs", "main_test.rs").unwrap();
        assert!(s1 > s2, "Closer match should score higher");
    }
}
