//! Core editing operations: auto-indent, comments, line moving, duplicate, brackets.

/// Returns the comment prefix for a given syntax name.
pub fn comment_prefix_for_syntax(syntax: &str) -> Option<(&'static str, Option<&'static str>)> {
    match syntax.to_lowercase().as_str() {
        "rust" | "c" | "c++" | "c#" | "javascript" | "typescript" | "typescriptreact" |
        "java" | "kotlin" | "swift" | "go" | "scala" | "dart" | "zig" | "glsl" => Some(("// ", None)),
        "python" | "ruby" | "bourne again shell (bash)" | "yaml" | "toml" | "dockerfile" |
        "makefile" | "perl" | "r" | "elixir" | "crystal" | "nim" => Some(("# ", None)),
        "sql" | "lua" | "haskell" | "ada" | "vhdl" => Some(("-- ", None)),
        "html" | "xml" | "markdown" => Some(("<!-- ", Some(" -->"))),
        "css" | "scss" | "less" => Some(("/* ", Some(" */"))),
        "ini" | "git config" => Some(("; ", None)),
        _ => Some(("// ", None)),
    }
}

/// Toggles line comments on the lines spanning `[start_char, end_char]`.
/// Returns the updated text and new char range.
pub fn toggle_comment(
    text: &str,
    start_char: usize,
    end_char: usize,
    syntax: &str,
) -> (String, usize, usize) {
    let (prefix, suffix) = comment_prefix_for_syntax(syntax).unwrap_or(("// ", None));
    let s_char = start_char.min(end_char);
    let e_char = start_char.max(end_char);

    // Find line boundaries
    let lines: Vec<&str> = text.split('\n').collect();
    let mut current_char = 0;
    let mut target_line_indices = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_len = line.chars().count();
        let line_end = current_char + line_len;
        if (s_char <= line_end && e_char >= current_char) || (s_char == e_char && current_char <= s_char && s_char <= line_end) {
            target_line_indices.push(i);
        }
        current_char = line_end + 1; // +1 for '\n'
    }

    if target_line_indices.is_empty() {
        return (text.to_string(), start_char, end_char);
    }

    // Determine if all target lines are already commented
    let prefix_trim = prefix.trim_end();
    let all_commented = target_line_indices.iter().all(|&idx| {
        let trimmed = lines[idx].trim_start();
        trimmed.starts_with(prefix_trim)
    });

    let mut new_lines = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if target_line_indices.contains(&i) {
            if all_commented {
                // Uncomment
                let leading_ws_len = line.chars().take_while(|c| c.is_whitespace()).count();
                let ws: String = line.chars().take(leading_ws_len).collect();
                let rest: String = line.chars().skip(leading_ws_len).collect();
                let mut uncommented = if rest.starts_with(prefix) {
                    rest[prefix.len()..].to_string()
                } else if rest.starts_with(prefix_trim) {
                    rest[prefix_trim.len()..].to_string()
                } else {
                    rest
                };
                if let Some(suf) = suffix {
                    let suf_trim = suf.trim();
                    if uncommented.trim_end().ends_with(suf_trim) {
                        if let Some(idx) = uncommented.rfind(suf_trim) {
                            uncommented.truncate(idx);
                        }
                    }
                }
                new_lines.push(format!("{ws}{uncommented}"));
            } else {
                // Comment
                let leading_ws_len = line.chars().take_while(|c| c.is_whitespace()).count();
                let ws: String = line.chars().take(leading_ws_len).collect();
                let rest: String = line.chars().skip(leading_ws_len).collect();
                if let Some(suf) = suffix {
                    new_lines.push(format!("{ws}{prefix}{rest}{suf}"));
                } else {
                    new_lines.push(format!("{ws}{prefix}{rest}"));
                }
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    let result = new_lines.join("\n");
    (result, start_char, end_char)
}

/// Computes the auto-indent string when pressing Enter at `cursor_char`.
pub fn compute_auto_indent(text: &str, cursor_char: usize, tab_size: usize) -> String {
    let before: String = text.chars().take(cursor_char).collect();
    let last_line = before.rsplit('\n').next().unwrap_or("");
    let leading_ws: String = last_line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();

    let trimmed = last_line.trim_end();
    let extra_indent = if trimmed.ends_with('{') || trimmed.ends_with('(') || trimmed.ends_with('[') || trimmed.ends_with(':') {
        " ".repeat(tab_size)
    } else {
        String::new()
    };

    format!("\n{leading_ws}{extra_indent}")
}

/// Duplicates the current line or selection.
pub fn duplicate_line(text: &str, cursor_char: usize) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut line_start = cursor_char.min(chars.len());
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }
    let mut line_end = cursor_char.min(chars.len());
    while line_end < chars.len() && chars[line_end] != '\n' {
        line_end += 1;
    }

    let line_str: String = chars[line_start..line_end].iter().collect();
    let mut new_text = String::with_capacity(text.len() + line_str.len() + 1);

    let prefix: String = chars[..line_end].iter().collect();
    let suffix: String = chars[line_end..].iter().collect();

    new_text.push_str(&prefix);
    new_text.push('\n');
    new_text.push_str(&line_str);
    new_text.push_str(&suffix);

    let new_cursor = line_end + 1 + (cursor_char - line_start);
    (new_text, new_cursor)
}

/// Moves the current line up or down.
pub fn move_line(text: &str, cursor_char: usize, up: bool) -> (String, usize) {
    let mut lines: Vec<&str> = text.split('\n').collect();

    let mut current_char = 0;
    let mut active_line = 0;
    let mut col = 0;

    for (i, line) in lines.iter().enumerate() {
        let line_len = line.chars().count();
        if cursor_char >= current_char && cursor_char <= current_char + line_len {
            active_line = i;
            col = cursor_char - current_char;
            break;
        }
        current_char += line_len + 1;
    }

    if up {
        if active_line == 0 {
            return (text.to_string(), cursor_char);
        }
        lines.swap(active_line, active_line - 1);
        active_line -= 1;
    } else {
        if active_line + 1 >= lines.len() {
            return (text.to_string(), cursor_char);
        }
        lines.swap(active_line, active_line + 1);
        active_line += 1;
    }

    // Recompute cursor
    let mut new_cursor = 0;
    for (i, line) in lines.iter().enumerate() {
        if i == active_line {
            new_cursor += col.min(line.chars().count());
            break;
        }
        new_cursor += line.chars().count() + 1;
    }

    (lines.join("\n"), new_cursor)
}

/// Indents or dedents the selection (or current line) by `tab_size` spaces.
pub fn indent_selection(
    text: &str,
    start_char: usize,
    end_char: usize,
    dedent: bool,
    tab_size: usize,
) -> (String, usize, usize) {
    let s_char = start_char.min(end_char);
    let e_char = start_char.max(end_char);
    let indent_str = " ".repeat(tab_size);

    let lines: Vec<&str> = text.split('\n').collect();
    let mut current_char = 0;
    let mut target_indices = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_len = line.chars().count();
        let line_end = current_char + line_len;
        if (s_char <= line_end && e_char >= current_char) || (s_char == e_char && current_char <= s_char && s_char <= line_end) {
            target_indices.push(i);
        }
        current_char = line_end + 1;
    }

    let mut new_lines = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if target_indices.contains(&i) {
            if dedent {
                let mut removed = 0;
                let mut chars = line.chars();
                while removed < tab_size {
                    match chars.clone().next() {
                        Some(' ') => {
                            chars.next();
                            removed += 1;
                        }
                        Some('\t') => {
                            chars.next();
                            break;
                        }
                        _ => break,
                    }
                }
                new_lines.push(chars.collect::<String>());
            } else {
                new_lines.push(format!("{indent_str}{line}"));
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    (new_lines.join("\n"), start_char, end_char)
}

/// Finds matching bracket for cursor if cursor is adjacent to `()`, `[]`, `{}`.
pub fn find_matching_bracket(text: &str, cursor_char: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    // Check character right before cursor or at cursor
    let pos = if cursor_char > 0 && is_bracket(chars[cursor_char - 1]) {
        cursor_char - 1
    } else if cursor_char < chars.len() && is_bracket(chars[cursor_char]) {
        cursor_char
    } else {
        return None;
    };

    let target = chars[pos];
    let (match_char, forward) = match target {
        '(' => (')', true),
        ')' => ('(', false),
        '[' => (']', true),
        ']' => ('[', false),
        '{' => ('}', true),
        '}' => ('{', false),
        _ => return None,
    };

    let mut depth = 0;
    if forward {
        for i in pos + 1..chars.len() {
            if chars[i] == target {
                depth += 1;
            } else if chars[i] == match_char {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
        }
    } else {
        for i in (0..pos).rev() {
            if chars[i] == target {
                depth += 1;
            } else if chars[i] == match_char {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
        }
    }
    None
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_comment() {
        let (commented, _, _) = toggle_comment("let x = 1;", 0, 0, "rust");
        assert_eq!(commented, "// let x = 1;");
        let (uncommented, _, _) = toggle_comment("// let x = 1;", 0, 0, "rust");
        assert_eq!(uncommented, "let x = 1;");
    }

    #[test]
    fn test_auto_indent() {
        let indent = compute_auto_indent("fn foo() {", 10, 4);
        assert_eq!(indent, "\n    ");
    }

    #[test]
    fn test_bracket_match() {
        let text = "fn foo(x: i32) {}";
        let m = find_matching_bracket(text, 7); // after '('
        assert_eq!(m, Some(13)); // index of ')'
    }
}
