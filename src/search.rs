//! In-file find & replace and project-wide search (⌘⇧F).

use regex::RegexBuilder;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectMatch {
    pub path: PathBuf,
    pub relative: String,
    pub line_number: usize,
    pub line_text: String,
    #[allow(dead_code)]
    pub match_range: (usize, usize),
}

pub struct ProjectSearch {
    pub open: bool,
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub results: Vec<ProjectMatch>,
    pub status: String,
    pub selected: usize,
}

impl ProjectSearch {
    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            case_sensitive: false,
            use_regex: false,
            results: Vec::new(),
            status: String::new(),
            selected: 0,
        }
    }

    pub fn execute(&mut self, root: Option<&Path>) {
        self.results.clear();
        self.selected = 0;
        let query = self.query.trim();
        if query.is_empty() {
            self.status = "Type a query to search".into();
            return;
        }

        let Some(root) = root else {
            self.status = "No folder open".into();
            return;
        };

        let mut files = Vec::new();
        collect_text_files(root, &mut files, 0);

        let re = if self.use_regex {
            match RegexBuilder::new(query)
                .case_insensitive(!self.case_sensitive)
                .build()
            {
                Ok(r) => Some(r),
                Err(e) => {
                    self.status = format!("Regex error: {e}");
                    return;
                }
            }
        } else {
            None
        };

        let mut matches = Vec::new();
        let query_lower = query.to_lowercase();

        for file_path in files {
            let Ok(content) = fs::read_to_string(&file_path) else { continue };
            let rel = file_path.strip_prefix(root).unwrap_or(&file_path).to_string_lossy().to_string();

            for (line_idx, line) in content.lines().enumerate() {
                if let Some(ref r) = re {
                    for m in r.find_iter(line) {
                        matches.push(ProjectMatch {
                            path: file_path.clone(),
                            relative: rel.clone(),
                            line_number: line_idx + 1,
                            line_text: line.to_string(),
                            match_range: (m.start(), m.end()),
                        });
                        if matches.len() >= 1000 {
                            break;
                        }
                    }
                } else {
                    let hay = if self.case_sensitive { line.to_string() } else { line.to_lowercase() };
                    let needle = if self.case_sensitive { query } else { &query_lower };

                    let mut start = 0;
                    while let Some(found) = hay[start..].find(needle) {
                        let actual_start = start + found;
                        let actual_end = actual_start + needle.len();
                        matches.push(ProjectMatch {
                            path: file_path.clone(),
                            relative: rel.clone(),
                            line_number: line_idx + 1,
                            line_text: line.to_string(),
                            match_range: (actual_start, actual_end),
                        });
                        start = actual_end;
                        if matches.len() >= 1000 {
                            break;
                        }
                    }
                }
                if matches.len() >= 1000 {
                    break;
                }
            }
            if matches.len() >= 1000 {
                break;
            }
        }

        self.status = format!("{} match(es) across {} files", matches.len(), count_unique_files(&matches));
        self.results = matches;
    }
}

fn count_unique_files(matches: &[ProjectMatch]) -> usize {
    let mut files = std::collections::HashSet::new();
    for m in matches {
        files.insert(&m.path);
    }
    files.len()
}

fn collect_text_files(dir: &Path, list: &mut Vec<PathBuf>, depth: usize) {
    if depth > 10 || list.len() > 3000 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".env" {
            continue;
        }
        if ["target", "node_modules", "build", "dist", ".git", "Music", "Pictures", "Movies", "Library", ".Trash"].contains(&name.as_str()) {
            continue;
        }
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                collect_text_files(&path, list, depth + 1);
            } else if ft.is_file() {
                // skip known media extensions
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !["png", "jpg", "jpeg", "gif", "webp", "pdf", "zip", "tar", "gz", "exe", "dylib", "so"].contains(&ext.as_str()) {
                    list.push(path);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// In-file Find & Replace

pub fn find_matches(haystack: &str, query: &str, case_sensitive: bool, use_regex: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    if use_regex {
        if let Ok(re) = RegexBuilder::new(query).case_insensitive(!case_sensitive).build() {
            return re.find_iter(haystack).map(|m| (m.start(), m.end())).collect();
        }
        return Vec::new();
    }

    let mut matches = Vec::new();
    if case_sensitive {
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(query) {
            let s = start + pos;
            let e = s + query.len();
            matches.push((s, e));
            start = e;
        }
    } else {
        let hay_lower = haystack.to_lowercase();
        let q_lower = query.to_lowercase();
        let mut start = 0;
        while let Some(pos) = hay_lower[start..].find(&q_lower) {
            let s = start + pos;
            let e = s + query.len();
            matches.push((s, e));
            start = e;
        }
    }
    matches
}

pub fn replace_next(
    text: &str,
    query: &str,
    replacement: &str,
    cursor_byte: usize,
    case_sensitive: bool,
    use_regex: bool,
) -> Option<(String, usize)> {
    let matches = find_matches(text, query, case_sensitive, use_regex);
    if matches.is_empty() {
        return None;
    }

    // Find the next match after cursor, or wrap to first
    let next_match = matches.iter().find(|(s, _)| *s >= cursor_byte).or_else(|| matches.first())?;

    let mut new_text = String::with_capacity(text.len() + replacement.len());
    new_text.push_str(&text[..next_match.0]);
    new_text.push_str(replacement);
    new_text.push_str(&text[next_match.1..]);

    let new_cursor = next_match.0 + replacement.len();
    Some((new_text, new_cursor))
}

pub fn replace_all(
    text: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    use_regex: bool,
) -> (String, usize) {
    if query.is_empty() {
        return (text.to_string(), 0);
    }
    if use_regex {
        if let Ok(re) = RegexBuilder::new(query).case_insensitive(!case_sensitive).build() {
            let count = re.find_iter(text).count();
            let replaced = re.replace_all(text, replacement).into_owned();
            return (replaced, count);
        }
        return (text.to_string(), 0);
    }

    let matches = find_matches(text, query, case_sensitive, false);
    let count = matches.len();
    if count == 0 {
        return (text.to_string(), 0);
    }

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    for (start, end) in matches {
        result.push_str(&text[last_end..start]);
        result.push_str(replacement);
        last_end = end;
    }
    result.push_str(&text[last_end..]);
    (result, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_and_replace() {
        let text = "Hello world, hello Rust";
        let matches = find_matches(text, "hello", false, false);
        assert_eq!(matches.len(), 2);

        let (replaced, count) = replace_all(text, "hello", "hi", false, false);
        assert_eq!(count, 2);
        assert_eq!(replaced, "hi world, hi Rust");
    }

    #[test]
    fn test_regex_replace() {
        let text = "count: 10, count: 20";
        let (replaced, count) = replace_all(text, r"\d+", "99", true, true);
        assert_eq!(count, 2);
        assert_eq!(replaced, "count: 99, count: 99");
    }
}
