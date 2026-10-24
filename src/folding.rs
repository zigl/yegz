//! Code folding detection: brace blocks, indentation blocks, and markdown headings.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: usize, // 1-indexed
    pub end_line: usize,   // 1-indexed
    pub indent: usize,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFold {
    pub start_line: usize,
    pub original_end_line: usize,
    pub hidden_lines: String,
    pub placeholder: String,
}

/// Detects foldable code blocks in the provided text.
pub fn detect_fold_ranges(text: &str, syntax: &str) -> Vec<FoldRange> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return Vec::new();
    }

    let is_python_like = matches!(
        syntax.to_lowercase().as_str(),
        "python" | "yaml" | "nim" | "dockerfile"
    );
    let is_markdown = matches!(syntax.to_lowercase().as_str(), "markdown");

    let mut ranges = Vec::new();

    if is_markdown {
        detect_markdown_headings(&lines, &mut ranges);
    } else if is_python_like {
        detect_indent_folds(&lines, &mut ranges);
    } else {
        detect_brace_folds(&lines, &mut ranges);
        // Also detect indentation folds for blocks that don't use braces
        if ranges.is_empty() {
            detect_indent_folds(&lines, &mut ranges);
        }
    }

    // Sort by start_line ascending
    ranges.sort_by_key(|r| r.start_line);
    ranges
}

/// Detects curly-brace enclosed blocks `{ ... }`.
fn detect_brace_folds(lines: &[&str], ranges: &mut Vec<FoldRange>) {
    // Stack of (start_line 1-indexed, indent, label)
    let mut stack: Vec<(usize, usize, String)> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Skip single-line comments
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.chars().take_while(|c| c.is_whitespace()).count();

        // Count opening and closing braces on this line (simple scanner outside strings)
        let mut in_string = false;
        let mut string_char = ' ';
        let mut prev_char = ' ';

        for ch in line.chars() {
            if in_string {
                if ch == string_char && prev_char != '\\' {
                    in_string = false;
                }
            } else if ch == '"' || ch == '\'' {
                in_string = true;
                string_char = ch;
            } else if ch == '{' {
                let label = trimmed.split('{').next().unwrap_or(trimmed).trim().to_string();
                let label = if label.is_empty() { "block".to_string() } else { label };
                stack.push((line_num, indent, label));
            } else if ch == '}' {
                if let Some((start_line, start_indent, label)) = stack.pop() {
                    if line_num > start_line {
                        ranges.push(FoldRange {
                            start_line,
                            end_line: line_num,
                            indent: start_indent,
                            label,
                        });
                    }
                }
            }
            prev_char = ch;
        }
    }
}

/// Detects indentation-based fold ranges (Python, YAML, etc.).
fn detect_indent_folds(lines: &[&str], ranges: &mut Vec<FoldRange>) {
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let start_line = i + 1;
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();

        // Look for end of indented block
        let mut end_line = start_line;
        for (j, next_line) in lines.iter().enumerate().skip(i + 1) {
            let next_trimmed = next_line.trim();
            if next_trimmed.is_empty() || next_trimmed.starts_with('#') {
                continue;
            }
            let next_indent = next_line.chars().take_while(|c| c.is_whitespace()).count();
            if next_indent > indent {
                end_line = j + 1;
            } else {
                break;
            }
        }

        if end_line > start_line {
            let label = trimmed.to_string();
            ranges.push(FoldRange {
                start_line,
                end_line,
                indent,
                label,
            });
        }
    }
}

/// Detects Markdown headings (`# Heading`, `## Subheading`).
fn detect_markdown_headings(lines: &[&str], ranges: &mut Vec<FoldRange>) {
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }

        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if level == 0 || level > 6 {
            continue;
        }

        let start_line = i + 1;
        let mut end_line = lines.len();

        // Scan ahead for equal or shallower heading
        for (j, next_line) in lines.iter().enumerate().skip(i + 1) {
            let next_trimmed = next_line.trim_start();
            if next_trimmed.starts_with('#') {
                let next_level = next_trimmed.chars().take_while(|c| *c == '#').count();
                if next_level <= level {
                    end_line = j;
                    break;
                }
            }
        }

        if end_line > start_line {
            ranges.push(FoldRange {
                start_line,
                end_line,
                indent: level,
                label: trimmed.to_string(),
            });
        }
    }
}

/// Collapses the lines inside `range` into a single folded placeholder line.
/// Returns the updated text and the created `ActiveFold`.
pub fn fold_block(text: &str, range: &FoldRange) -> Option<(String, ActiveFold)> {
    if range.start_line >= range.end_line {
        return None;
    }

    let ends_with_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    if range.start_line > lines.len() || range.end_line > lines.len() {
        return None;
    }

    // Determine the range of lines to hide.
    // For brace blocks where range.end_line has closing brace, hide lines `start_line` to `end_line - 1` (0-indexed)
    let hide_start_idx = range.start_line; // line after header
    let hide_end_idx = if range.end_line > range.start_line + 1 {
        range.end_line - 1
    } else {
        range.end_line
    };

    if hide_start_idx >= hide_end_idx {
        return None;
    }

    let hidden_slice = &lines[hide_start_idx..hide_end_idx];
    let hidden_lines = hidden_slice.join("\n");
    let count = hidden_slice.len();

    let indent_str = " ".repeat(range.indent + 4);
    let placeholder = format!("{indent_str}/* ⋯ {count} lines folded ⋯ */");

    let mut new_lines = Vec::new();
    new_lines.extend_from_slice(&lines[..hide_start_idx]);
    new_lines.push(&placeholder);
    new_lines.extend_from_slice(&lines[hide_end_idx..]);

    let mut result = new_lines.join("\n");
    if ends_with_newline {
        result.push('\n');
    }

    let active = ActiveFold {
        start_line: range.start_line,
        original_end_line: range.end_line,
        hidden_lines,
        placeholder,
    };

    Some((result, active))
}

/// Restores a previously folded block back to its original lines.
pub fn unfold_block(text: &str, fold: &ActiveFold) -> String {
    text.replace(&fold.placeholder, &fold.hidden_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fold_and_unfold() {
        let original = "fn main() {\n    let a = 1;\n    let b = 2;\n}\n";
        let range = FoldRange {
            start_line: 1,
            end_line: 4,
            indent: 0,
            label: "fn main()".into(),
        };
        let (folded, active) = fold_block(original, &range).unwrap();
        assert!(folded.contains("folded"));
        assert!(!folded.contains("let a = 1;"));
        let restored = unfold_block(&folded, &active);
        assert_eq!(restored, original);
    }

    #[test]
    fn test_brace_folding() {
        let code = r#"fn main() {
    let a = 1;
    let b = 2;
}"#;
        let folds = detect_fold_ranges(code, "Rust");
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].start_line, 1);
        assert_eq!(folds[0].end_line, 4);
    }

    #[test]
    fn test_nested_brace_folding() {
        let code = r#"struct Foo {
    fn bar() {
        if true {
            run();
        }
    }
}"#;
        let folds = detect_fold_ranges(code, "Rust");
        assert_eq!(folds.len(), 3);
        assert_eq!(folds[0].start_line, 1);
        assert_eq!(folds[0].end_line, 7);
        assert_eq!(folds[1].start_line, 2);
        assert_eq!(folds[1].end_line, 6);
        assert_eq!(folds[2].start_line, 3);
        assert_eq!(folds[2].end_line, 5);
    }

    #[test]
    fn test_python_indent_folding() {
        let code = "def hello():\n    print('a')\n    print('b')\n\nprint('done')\n";
        let folds = detect_fold_ranges(code, "Python");
        assert!(!folds.is_empty());
        assert_eq!(folds[0].start_line, 1);
        assert_eq!(folds[0].end_line, 3);
    }

    #[test]
    fn test_markdown_heading_folding() {
        let md = "# Title\nline 1\nline 2\n# Next\nline 3";
        let folds = detect_fold_ranges(md, "Markdown");
        assert_eq!(folds.len(), 2);
        assert_eq!(folds[0].start_line, 1);
        assert_eq!(folds[0].end_line, 3);
    }
}
