//! Session and configuration persistence.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeName {
    #[default]
    OneDark,
    OneLight,
    GitHubDark,
    Monokai,
}

impl ThemeName {
    pub const ALL: &[ThemeName] = &[
        ThemeName::OneDark,
        ThemeName::OneLight,
        ThemeName::GitHubDark,
        ThemeName::Monokai,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::OneDark => "One Dark",
            Self::OneLight => "One Light",
            Self::GitHubDark => "GitHub Dark",
            Self::Monokai => "Monokai",
        }
    }

    pub fn is_dark(&self) -> bool {
        !matches!(self, Self::OneLight)
    }

    #[allow(dead_code)]
    pub fn syntect_theme(&self) -> &'static str {
        match self {
            Self::OneDark => "base16-ocean.dark",
            Self::OneLight => "base16-ocean.light",
            Self::GitHubDark => "base16-eighties.dark",
            Self::Monokai => "base16-mocha.dark",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: ThemeName,
    pub font_size: f32,
    pub tab_size: usize,
    pub word_wrap: bool,
    pub show_line_numbers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeName::OneDark,
            font_size: 14.0,
            tab_size: 4,
            word_wrap: false,
            show_line_numbers: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub root: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub sidebar_width: f32,
    pub window_size: [f32; 2],
    pub settings: Settings,
}

fn config_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join("Library/Application Support/lite");
        return Some(p);
    }
    None
}

fn session_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("session.json"))
}

pub fn load_session() -> SessionState {
    let Some(path) = session_file() else { return SessionState::default() };
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<SessionState>(&data) {
            return state;
        }
    }
    SessionState::default()
}

pub fn save_session(state: &SessionState) {
    let Some(dir) = config_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("session.json");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_roundtrip() {
        let state = SessionState {
            root: Some(PathBuf::from("/test")),
            open_files: vec![PathBuf::from("/test/a.rs")],
            active_file: Some(PathBuf::from("/test/a.rs")),
            sidebar_width: 250.0,
            window_size: [1200.0, 800.0],
            settings: Settings {
                theme: ThemeName::OneLight,
                font_size: 16.0,
                tab_size: 2,
                word_wrap: true,
                show_line_numbers: true,
            },
        };
        let s = serde_json::to_string(&state).unwrap();
        let loaded: SessionState = serde_json::from_str(&s).unwrap();
        assert_eq!(loaded.sidebar_width, 250.0);
        assert_eq!(loaded.settings.theme, ThemeName::OneLight);
        assert_eq!(loaded.settings.tab_size, 2);
    }
}
