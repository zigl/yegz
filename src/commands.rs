//! Command Palette (⌘⇧P) for quick keyboard-driven action execution.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCommand {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub shortcut: Option<&'static str>,
    pub icon: &'static str,
}

#[derive(Debug, Clone)]
pub struct CommandMatch {
    pub command: EditorCommand,
    pub score: i64,
}

pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<CommandMatch>,
}

impl CommandPalette {
    pub fn new() -> Self {
        let mut cp = Self {
            open: false,
            query: String::new(),
            selected: 0,
            results: Vec::new(),
        };
        cp.update_search();
        cp
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
        let commands = all_commands();

        if q.is_empty() {
            self.results = commands
                .into_iter()
                .map(|command| CommandMatch { command, score: 0 })
                .collect();
            self.selected = 0;
            return;
        }

        let mut scored: Vec<CommandMatch> = commands
            .into_iter()
            .filter_map(|command| {
                fuzzy_score_command(&q, &command).map(|score| CommandMatch { command, score })
            })
            .collect();

        scored.sort_by(|a, b| b.score.cmp(&a.score));
        self.results = scored;
        self.selected = 0;
    }
}

pub fn all_commands() -> Vec<EditorCommand> {
    vec![
        // File
        EditorCommand { id: "file.new", title: "New File / Tab", category: "File", shortcut: Some("⌘N"), icon: "📄" },
        EditorCommand { id: "file.open", title: "Open File…", category: "File", shortcut: Some("⌘O"), icon: "📂" },
        EditorCommand { id: "file.open_folder", title: "Open Folder…", category: "File", shortcut: Some("⌘⇧O"), icon: "📁" },
        EditorCommand { id: "file.save", title: "Save File", category: "File", shortcut: Some("⌘S"), icon: "💾" },
        EditorCommand { id: "file.save_as", title: "Save File As…", category: "File", shortcut: Some("⌘⇧S"), icon: "💾" },
        EditorCommand { id: "file.close_tab", title: "Close Tab", category: "File", shortcut: Some("⌘W"), icon: "×" },
        EditorCommand { id: "file.close_other_tabs", title: "Close Other Tabs", category: "File", shortcut: None, icon: "×" },
        EditorCommand { id: "file.reveal_finder", title: "Reveal Active File in Finder", category: "File", shortcut: None, icon: "🔍" },
        EditorCommand { id: "file.copy_path", title: "Copy Full Path", category: "File", shortcut: None, icon: "📋" },

        // Search & Navigate
        EditorCommand { id: "find.quick_open", title: "Quick Open File…", category: "Navigate", shortcut: Some("⌘P"), icon: "🎯" },
        EditorCommand { id: "find.in_file", title: "Find & Replace in File", category: "Find", shortcut: Some("⌘F"), icon: "🔍" },
        EditorCommand { id: "find.in_project", title: "Search in Project Files", category: "Find", shortcut: Some("⌘⇧F"), icon: "🔎" },
        EditorCommand { id: "navigate.goto_line", title: "Go to Line…", category: "Navigate", shortcut: Some("Ctrl+G"), icon: "📍" },

        // Code Editing
        EditorCommand { id: "edit.toggle_comment", title: "Toggle Line Comment", category: "Edit", shortcut: Some("⌘/"), icon: "💬" },
        EditorCommand { id: "edit.duplicate_line", title: "Duplicate Line", category: "Edit", shortcut: Some("⌘⇧D"), icon: "📑" },
        EditorCommand { id: "edit.move_line_up", title: "Move Line Up", category: "Edit", shortcut: Some("⌥↑"), icon: "⬆" },
        EditorCommand { id: "edit.move_line_down", title: "Move Line Down", category: "Edit", shortcut: Some("⌥↓"), icon: "⬇" },
        EditorCommand { id: "edit.indent", title: "Indent Line / Selection", category: "Edit", shortcut: Some("Tab"), icon: "⇥" },
        EditorCommand { id: "edit.outdent", title: "Outdent Line / Selection", category: "Edit", shortcut: Some("⇧Tab"), icon: "⇤" },

        // View & Layout
        EditorCommand { id: "view.toggle_sidebar", title: "Toggle File Tree Sidebar", category: "View", shortcut: Some("⌘B"), icon: "◧" },
        EditorCommand { id: "view.toggle_split", title: "Toggle Split View (Side-by-Side)", category: "View", shortcut: None, icon: "◫" },
        EditorCommand { id: "view.toggle_word_wrap", title: "Toggle Word Wrap", category: "View", shortcut: None, icon: "↩" },
        EditorCommand { id: "view.toggle_line_numbers", title: "Toggle Line Numbers", category: "View", shortcut: None, icon: "#" },

        // Themes
        EditorCommand { id: "theme.onedark", title: "Theme: One Dark", category: "Theme", shortcut: None, icon: "🎨" },
        EditorCommand { id: "theme.onelight", title: "Theme: One Light", category: "Theme", shortcut: None, icon: "🎨" },
        EditorCommand { id: "theme.githubdark", title: "Theme: GitHub Dark", category: "Theme", shortcut: None, icon: "🎨" },
        EditorCommand { id: "theme.monokai", title: "Theme: Monokai", category: "Theme", shortcut: None, icon: "🎨" },

        // Syntax
        EditorCommand { id: "syntax.autodetect", title: "Syntax: Auto-detect", category: "Syntax", shortcut: None, icon: "⚡" },

        // Settings & Help
        EditorCommand { id: "preferences.open", title: "Open Settings & Preferences…", category: "Preferences", shortcut: Some("⌘,"), icon: "⚙" },
        EditorCommand { id: "help.about", title: "About Yegz…", category: "Help", shortcut: None, icon: "ℹ" },
    ]
}

pub fn fuzzy_score_command(query: &str, cmd: &EditorCommand) -> Option<i64> {
    let target = format!("{} {}", cmd.category, cmd.title).to_lowercase();
    let q_chars: Vec<char> = query.chars().collect();
    let t_chars: Vec<char> = target.chars().collect();

    let mut q_idx = 0;
    let mut score = 0i64;
    let mut consecutive = 0;

    for (t_idx, &tc) in t_chars.iter().enumerate() {
        if q_idx < q_chars.len() && tc == q_chars[q_idx] {
            q_idx += 1;
            score += 10;
            score += consecutive * 5;
            consecutive += 1;

            // Bonus if start of word
            if t_idx == 0 || t_chars[t_idx - 1] == ' ' || t_chars[t_idx - 1] == ':' {
                score += 20;
            }
        } else {
            consecutive = 0;
        }
    }

    if q_idx == q_chars.len() {
        // Shorter titles with same matches score higher
        score -= target.len() as i64;
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_search() {
        let mut palette = CommandPalette::new();
        palette.query = "wrap".into();
        palette.update_search();
        assert!(!palette.results.is_empty());
        assert_eq!(palette.results[0].command.id, "view.toggle_word_wrap");
    }

    #[test]
    fn test_theme_search() {
        let mut palette = CommandPalette::new();
        palette.query = "monokai".into();
        palette.update_search();
        assert_eq!(palette.results[0].command.id, "theme.monokai");
    }
}
