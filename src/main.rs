//! Yegz — an Atom-inspired lightweight code and file editor in Rust.

use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_extras::syntax_highlighting::{self, CodeTheme};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

mod commands;
mod editor_ops;
mod fuzzy;
mod git;
mod search;
mod session;
mod syntax;
mod terminal;
mod updater;
mod viewers;

use commands::CommandPalette;
use fuzzy::FuzzyFinder;
use git::{FileGitStatus, GitRepoStatus};
use search::ProjectSearch;
use session::{SessionState, Settings, ThemeName};
use terminal::{detect_project_tasks, render_native_terminal, ProjectTask, TerminalDrawer};
use updater::{AutoUpdater, UpdateState};
use viewers::{ImageView, PdfView};

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

fn main() -> eframe::Result {
    let arg = std::env::args().nth(1).map(PathBuf::from);
    let session = session::load_session();
    let initial_size = session.window_size;

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon_512.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Yegz")
        .with_inner_size(if initial_size[0] > 100.0 { initial_size } else { [1100.0, 720.0] })
        .with_min_inner_size([400.0, 300.0]);

    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Yegz",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx, &session.settings);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Editor::new(arg, session, cc.egui_ctx.clone())))
        }),
    );
    std::process::exit(0);
}

fn apply_theme(ctx: &egui::Context, settings: &Settings) {
    let mut style = (*ctx.global_style()).clone();
    let (bg, panel, text, select_bg, select_stroke, border) = match settings.theme {
        ThemeName::OneDark => (
            Color32::from_rgb(40, 44, 52),
            Color32::from_rgb(33, 37, 43),
            Color32::from_rgb(171, 178, 191),
            Color32::from_rgb(62, 68, 81),
            Color32::from_rgb(97, 175, 239),
            Color32::from_rgb(24, 26, 31),
        ),
        ThemeName::OneLight => (
            Color32::from_rgb(250, 250, 250),
            Color32::from_rgb(234, 234, 234),
            Color32::from_rgb(56, 58, 66),
            Color32::from_rgb(220, 222, 225),
            Color32::from_rgb(82, 111, 255),
            Color32::from_rgb(215, 215, 215),
        ),
        ThemeName::GitHubDark => (
            Color32::from_rgb(13, 17, 23),
            Color32::from_rgb(22, 27, 34),
            Color32::from_rgb(201, 209, 217),
            Color32::from_rgb(31, 111, 235),
            Color32::from_rgb(88, 166, 255),
            Color32::from_rgb(48, 54, 61),
        ),
        ThemeName::Monokai => (
            Color32::from_rgb(39, 40, 34),
            Color32::from_rgb(30, 31, 28),
            Color32::from_rgb(248, 248, 242),
            Color32::from_rgb(73, 72, 62),
            Color32::from_rgb(249, 38, 114),
            Color32::from_rgb(20, 20, 20),
        ),
    };

    if settings.theme.is_dark() {
        style.visuals = egui::Visuals::dark();
    } else {
        style.visuals = egui::Visuals::light();
    }

    style.visuals.panel_fill = panel;
    style.visuals.window_fill = panel;
    style.visuals.extreme_bg_color = bg;
    style.visuals.widgets.noninteractive.bg_fill = panel;

    let btn_bg = if settings.theme.is_dark() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 10)
    };
    let btn_hover = if settings.theme.is_dark() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 24)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 18)
    };
    let btn_active = if settings.theme.is_dark() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 38)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 28)
    };

    style.visuals.widgets.inactive.bg_fill = btn_bg;
    style.visuals.widgets.hovered.bg_fill = btn_hover;
    style.visuals.widgets.active.bg_fill = btn_active;
    style.visuals.widgets.open.bg_fill = btn_hover;

    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(4);

    style.visuals.widgets.noninteractive.fg_stroke.color = text;
    style.visuals.widgets.inactive.fg_stroke.color = text;
    style.visuals.selection.bg_fill = select_bg;
    style.visuals.selection.stroke.color = select_stroke;
    style.visuals.hyperlink_color = select_stroke;
    style.visuals.window_stroke.color = border;

    style.spacing.button_padding = egui::vec2(7.0, 3.5);
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);

    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::monospace(settings.font_size),
    );
    ctx.set_global_style(style);
}

// ---------------------------------------------------------------------------

enum View {
    Text,
    Image(ImageView),
    Pdf(PdfView),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MdMode {
    Source,
    Split,
    Preview,
}

impl MdMode {
    fn next(self) -> Self {
        match self {
            Self::Source => Self::Split,
            Self::Split => Self::Preview,
            Self::Preview => Self::Source,
        }
    }
}

struct Tab {
    path: Option<PathBuf>,
    view: View,
    md_mode: MdMode,
    md_cache: CommonMarkCache,
    text: String,
    saved: String,
    read_only: bool,
    syntax: String,
    last_modified: Option<SystemTime>,
    disk_changed: bool,
    cached_line_count: usize,
    cached_line_numbers: String,
}

impl Tab {
    fn untitled() -> Self {
        Self {
            path: None,
            view: View::Text,
            md_mode: MdMode::Source,
            md_cache: CommonMarkCache::default(),
            text: String::new(),
            saved: String::new(),
            read_only: false,
            syntax: syntax::PLAIN.into(),
            last_modified: None,
            disk_changed: false,
            cached_line_count: 1,
            cached_line_numbers: "  1".into(),
        }
    }

    fn is_markdown(&self) -> bool {
        matches!(self.view, View::Text) && self.syntax == "Markdown"
    }

    fn open(path: &Path, ctx: &egui::Context) -> Result<Self, String> {
        let meta = fs::metadata(path).map_err(|e| e.to_string())?;
        let ext = viewers::ext_of(path);
        let is_image = viewers::IMAGE_EXTS.contains(&ext.as_str());
        let is_pdf = viewers::PDF_EXTS.contains(&ext.as_str());

        if is_image || is_pdf {
            if meta.len() > MAX_MEDIA_BYTES {
                return Err(format!("file too large ({} MB)", meta.len() / 1024 / 1024));
            }
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            let view = if is_image {
                View::Image(ImageView::open(&bytes, path)?)
            } else {
                View::Pdf(PdfView::open(bytes, ctx)?)
            };
            let mut tab = Self::untitled();
            tab.path = Some(path.to_path_buf());
            tab.view = view;
            tab.read_only = true;
            tab.syntax = if is_image { "Image".into() } else { "PDF".into() };
            tab.last_modified = meta.modified().ok();
            return Ok(tab);
        }

        if meta.len() > MAX_FILE_BYTES {
            return Err(format!("file too large ({} MB)", meta.len() / 1024 / 1024));
        }

        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let (text, read_only) = match String::from_utf8(bytes) {
            Ok(s) => (s, false),
            Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
        };

        let mut tab = Self::untitled();
        tab.syntax = syntax::detect(Some(path), &text);
        if tab.syntax == "Markdown" {
            tab.md_mode = MdMode::Split;
        }
        tab.path = Some(path.to_path_buf());
        tab.saved = text.clone();
        tab.text = text;
        tab.read_only = read_only;
        tab.last_modified = meta.modified().ok();
        tab.recalculate_line_numbers();
        Ok(tab)
    }

    fn recalculate_line_numbers(&mut self) {
        let count = self.text.lines().count().max(1) + usize::from(self.text.ends_with('\n'));
        if count != self.cached_line_count || self.cached_line_numbers.is_empty() {
            self.cached_line_count = count;
            let digits = count.to_string().len().max(3);
            let mut s = String::with_capacity(count * (digits + 1));
            for n in 1..=count {
                use std::fmt::Write;
                let _ = writeln!(s, "{n:>digits$}");
            }
            self.cached_line_numbers = s;
        }
    }

    fn dirty(&self) -> bool {
        self.text != self.saved
    }

    fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into())
    }
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

enum Pending {
    CloseTab(usize),
    Quit,
}

#[derive(Clone)]
enum TreeModal {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    Delete(PathBuf),
}

struct Editor {
    root: Option<PathBuf>,
    dir_cache: HashMap<PathBuf, Vec<Entry>>,
    tabs: Vec<Tab>,
    active: usize,
    status: String,
    show_sidebar: bool,
    sidebar_width: f32,

    // Find & Replace
    find_open: bool,
    find_query: String,
    replace_query: String,
    find_case_sensitive: bool,
    find_use_regex: bool,
    find_focus: bool,
    goto_offset: Option<usize>,

    pending: Option<Pending>,
    allow_quit: bool,
    lang_filter: String,
    ctx: egui::Context,

    // Features 1 to 11
    fuzzy: FuzzyFinder,
    project_search: ProjectSearch,
    git: GitRepoStatus,
    settings: Settings,
    settings_open: bool,
    about_open: bool,

    // New Features
    command_palette: CommandPalette,
    goto_line_open: bool,
    goto_line_input: String,
    recent_files: Vec<PathBuf>,
    recent_projects: Vec<PathBuf>,
    split_view: bool,
    split_active: usize,
    updater: AutoUpdater,
    terminal: TerminalDrawer,
    project_tasks: Vec<ProjectTask>,

    tree_modal: Option<TreeModal>,
    modal_input: String,
}

impl Editor {
    fn new(arg: Option<PathBuf>, session: SessionState, ctx: egui::Context) -> Self {
        let mut updater = AutoUpdater::new();
        if session.settings.auto_check_updates {
            updater.check_for_updates(env!("CARGO_PKG_VERSION"));
        }

        let mut ed = Self {
            root: None,
            dir_cache: HashMap::new(),
            tabs: vec![],
            active: 0,
            status: String::new(),
            show_sidebar: true,
            sidebar_width: if session.sidebar_width > 50.0 { session.sidebar_width } else { 230.0 },

            find_open: false,
            find_query: String::new(),
            replace_query: String::new(),
            find_case_sensitive: false,
            find_use_regex: false,
            find_focus: false,
            goto_offset: None,

            pending: None,
            allow_quit: false,
            lang_filter: String::new(),
            ctx: ctx.clone(),

            fuzzy: FuzzyFinder::new(),
            project_search: ProjectSearch::new(),
            git: GitRepoStatus::new(),
            settings: session.settings,
            settings_open: false,
            about_open: false,

            command_palette: CommandPalette::new(),
            goto_line_open: false,
            goto_line_input: String::new(),
            recent_files: session.recent_files.clone(),
            recent_projects: session.recent_projects.clone(),
            split_view: false,
            split_active: 0,
            updater,
            terminal: TerminalDrawer::new(),
            project_tasks: Vec::new(),

            tree_modal: None,
            modal_input: String::new(),
        };

        let mut restored = false;

        match arg {
            Some(p) if p.is_dir() => {
                ed.set_root(fs::canonicalize(&p).ok().as_deref());
            }
            Some(p) if p.is_file() => {
                let canon = p.canonicalize().ok().unwrap_or(p);
                ed.set_root(canon.parent());
                ed.open_file(&canon);
                restored = true;
            }
            Some(p) => {
                let mut t = Tab::untitled();
                t.syntax = syntax::detect(Some(&p), "");
                t.path = Some(p);
                ed.tabs.push(t);
                restored = true;
            }
            None => {
                // Restore session from state
                if let Some(r) = session.root.filter(|p| p.is_dir()) {
                    ed.set_root(Some(&r));
                } else {
                    ed.set_root(std::env::current_dir().ok().as_deref());
                }

                for f in &session.open_files {
                    if f.exists() {
                        ed.open_file(f);
                        restored = true;
                    }
                }
                if let Some(active_f) = session.active_file {
                    if let Some(pos) = ed.tabs.iter().position(|t| t.path.as_deref() == Some(&active_f)) {
                        ed.active = pos;
                    }
                }
            }
        }

        if !restored || ed.tabs.is_empty() {
            ed.tabs.push(Tab::untitled());
        }

        ed
    }

    fn set_root(&mut self, root: Option<&Path>) {
        if let Some(r) = root {
            let canon = r.canonicalize().unwrap_or_else(|_| r.to_path_buf());
            self.recent_projects.retain(|p| p != &canon);
            self.recent_projects.insert(0, canon);
            self.recent_projects.truncate(10);
        }
        self.root = root.map(|p| p.to_path_buf());
        self.dir_cache.clear();
        self.fuzzy.set_root(root);
        self.git.refresh(root);
        self.project_tasks = detect_project_tasks(root);
    }

    fn save_session_state(&self) {
        let open_files: Vec<PathBuf> = self.tabs.iter().filter_map(|t| t.path.clone()).collect();
        let active_file = self.tabs.get(self.active).and_then(|t| t.path.clone());
        let window_size = self.ctx.input(|i| {
            let r = i.viewport().inner_rect.unwrap_or(egui::Rect::ZERO);
            [r.width(), r.height()]
        });

        let state = SessionState {
            root: self.root.clone(),
            open_files,
            active_file,
            sidebar_width: self.sidebar_width,
            window_size,
            settings: self.settings.clone(),
            recent_files: self.recent_files.clone(),
            recent_projects: self.recent_projects.clone(),
        };
        session::save_session(&state);
    }

    // ----- file operations -------------------------------------------------

    fn open_file(&mut self, path: &Path) {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent_files.retain(|p| p != &canon);
        self.recent_files.insert(0, canon.clone());
        self.recent_files.truncate(20);

        if let Some(i) = self.tabs.iter().position(|t| t.path.as_deref() == Some(&canon)) {
            self.active = i;
            return;
        }
        match Tab::open(&canon, &self.ctx) {
            Ok(tab) => {
                if tab.read_only && matches!(tab.view, View::Text) {
                    self.status = "Non-UTF-8 file opened read-only".into();
                }
                if self.tabs.len() == 1 && self.tabs[0].path.is_none() && self.tabs[0].text.is_empty() {
                    self.tabs.clear();
                }
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
                self.save_session_state();
            }
            Err(e) => self.status = format!("Cannot open {}: {e}", canon.display()),
        }
    }

    fn check_disk_modifications(&mut self) {
        let tab = &mut self.tabs[self.active];
        let Some(path) = &tab.path else { return };
        let Ok(meta) = fs::metadata(path) else { return };
        let Ok(mtime) = meta.modified() else { return };

        if let Some(last) = tab.last_modified {
            if mtime > last {
                if !tab.dirty() {
                    // Clean tab: reload automatically
                    if let Ok(bytes) = fs::read(path) {
                        if let Ok(s) = String::from_utf8(bytes) {
                            tab.text = s.clone();
                            tab.saved = s;
                            tab.last_modified = Some(mtime);
                            tab.recalculate_line_numbers();
                            self.status = format!("Reloaded {} from disk", tab.title());
                        }
                    }
                } else {
                    tab.disk_changed = true;
                }
            }
        } else {
            tab.last_modified = Some(mtime);
        }
    }

    fn save(&mut self, idx: usize, save_as: bool) -> bool {
        let Some(tab) = self.tabs.get_mut(idx) else { return false };
        if tab.read_only {
            self.status = "File is read-only".into();
            return false;
        }
        let path = match (&tab.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut dlg = rfd::FileDialog::new().set_file_name(tab.title());
                if let Some(dir) = tab.path.as_ref().and_then(|p| p.parent()).or(self.root.as_deref()) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };

        match fs::write(&path, &tab.text) {
            Ok(()) => {
                tab.saved = tab.text.clone();
                if tab.path.as_deref() != Some(path.as_path()) {
                    tab.syntax = syntax::detect(Some(&path), &tab.text);
                }
                tab.path = Some(path.clone());
                tab.last_modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
                tab.disk_changed = false;
                self.status = format!("Saved {}", path.display());
                if let Some(parent) = path.parent() {
                    self.dir_cache.remove(parent);
                }
                self.git.refresh(self.root.as_deref());
                self.save_session_state();
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
                false
            }
        }
    }

    fn request_close(&mut self, idx: usize) {
        if self.tabs.get(idx).is_some_and(Tab::dirty) {
            self.pending = Some(Pending::CloseTab(idx));
        } else {
            self.close_tab(idx);
        }
    }

    fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::untitled());
        }
        if self.active >= idx && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.save_session_state();
    }

    fn close_other_tabs(&mut self, keep_idx: usize) {
        if keep_idx >= self.tabs.len() {
            return;
        }
        let keep_tab = self.tabs.remove(keep_idx);
        let mut new_tabs = Vec::new();
        for t in self.tabs.drain(..) {
            if t.dirty() {
                new_tabs.push(t);
            }
        }
        new_tabs.insert(0, keep_tab);
        self.tabs = new_tabs;
        self.active = 0;
        self.save_session_state();
    }

    fn open_folder_dialog(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.set_root(Some(&dir));
            self.save_session_state();
        }
    }

    fn open_file_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(r) = &self.root {
            dlg = dlg.set_directory(r);
        }
        if let Some(files) = dlg.pick_files() {
            for f in files {
                self.open_file(&f);
            }
        }
    }

    // ----- Find & Replace --------------------------------------------------

    fn find_next(&mut self, backwards: bool) {
        if self.find_query.is_empty() {
            return;
        }
        let tab = &self.tabs[self.active];
        let matches = search::find_matches(
            &tab.text,
            &self.find_query,
            self.find_case_sensitive,
            self.find_use_regex,
        );
        if matches.is_empty() {
            self.status = format!("\"{}\" not found", self.find_query);
            return;
        }

        let id = self.editor_id();
        let cursor_char = egui::TextEdit::load_state(&self.ctx, id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| r.primary.index.0)
            .unwrap_or(0);
        let cursor_byte = char_to_byte(&tab.text, cursor_char);

        let next = if backwards {
            matches.iter().rev().find(|(s, _)| *s < cursor_byte).or_else(|| matches.last())
        } else {
            matches.iter().find(|(s, _)| *s > cursor_byte).or_else(|| matches.first())
        };

        if let Some((start, _)) = next {
            self.goto_offset = Some(*start);
            self.status.clear();
        }
    }

    fn replace_current(&mut self) {
        let id = self.editor_id();
        let cursor_char = egui::TextEdit::load_state(&self.ctx, id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| r.primary.index.0)
            .unwrap_or(0);
        let tab = &mut self.tabs[self.active];
        let cursor_byte = char_to_byte(&tab.text, cursor_char);

        if let Some((new_text, new_cursor_byte)) = search::replace_next(
            &tab.text,
            &self.find_query,
            &self.replace_query,
            cursor_byte,
            self.find_case_sensitive,
            self.find_use_regex,
        ) {
            tab.text = new_text;
            tab.recalculate_line_numbers();
            self.goto_offset = Some(new_cursor_byte);
        }
    }

    fn replace_all_matches(&mut self) {
        let tab = &mut self.tabs[self.active];
        let (new_text, count) = search::replace_all(
            &tab.text,
            &self.find_query,
            &self.replace_query,
            self.find_case_sensitive,
            self.find_use_regex,
        );
        tab.text = new_text;
        tab.recalculate_line_numbers();
        self.status = format!("Replaced {count} occurrence(s)");
    }

    fn editor_id(&self) -> egui::Id {
        egui::Id::unique(("editor", self.active, self.tabs[self.active].path.clone()))
    }

    // ----- UI & Shortcuts --------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let alt = Modifiers::ALT;
        let hit = |m, k| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        // Command Palette (⌘⇧P)
        if hit(cmd_shift, Key::P) {
            self.command_palette.open_palette();
        }

        // Go to Line (Ctrl+G)
        if hit(Modifiers::CTRL, Key::G) {
            self.goto_line_open = true;
            self.goto_line_input.clear();
        }

        // Toggle Split View (⌘⌥\)
        if hit(Modifiers::COMMAND | Modifiers::ALT, Key::Backslash) {
            self.split_view = !self.split_view;
            if self.split_view && self.tabs.len() >= 2 && self.split_active == self.active {
                self.split_active = (self.active + 1) % self.tabs.len();
            }
        }

        // Toggle Terminal Drawer (⌃` or ⌘`)
        if hit(Modifiers::CTRL, Key::Backtick) || hit(cmd, Key::Backtick) {
            self.terminal.toggle(self.root.as_deref());
        }

        // Fuzzy Finder (⌘P)
        if hit(cmd, Key::P) {
            self.fuzzy.open_palette();
        }

        // Project Search (⌘⇧F)
        if hit(cmd_shift, Key::F) {
            self.project_search.open = true;
        }

        // Settings (⌘,)
        if hit(cmd, Key::Comma) {
            self.settings_open = !self.settings_open;
        }

        // Save
        if hit(cmd_shift, Key::S) {
            self.save(self.active, true);
        }
        if hit(cmd, Key::S) {
            self.save(self.active, false);
        }

        // Open
        if hit(cmd_shift, Key::O) {
            self.open_folder_dialog();
        }
        if hit(cmd, Key::O) {
            self.open_file_dialog();
        }

        // Tabs
        if hit(cmd, Key::N) {
            self.tabs.push(Tab::untitled());
            self.active = self.tabs.len() - 1;
        }
        if hit(cmd, Key::W) {
            self.request_close(self.active);
        }
        if hit(cmd, Key::Q) {
            if self.tabs.iter().any(Tab::dirty) {
                self.pending = Some(Pending::Quit);
            } else {
                self.save_session_state();
                std::process::exit(0);
            }
        }

        // Sidebar
        if hit(cmd, Key::B) || hit(cmd, Key::Backslash) {
            self.show_sidebar = !self.show_sidebar;
        }

        // Find
        if hit(cmd, Key::F) {
            self.find_open = true;
            self.find_focus = true;
        }
        if hit(cmd_shift, Key::G) {
            self.find_next(true);
        }
        if hit(cmd, Key::G) {
            self.find_next(false);
        }
        if self.find_open && hit(Modifiers::NONE, Key::Escape) {
            self.find_open = false;
        }

        // Markdown mode cycle
        if hit(Modifiers::CTRL | Modifiers::SHIFT, Key::M) {
            let tab = &mut self.tabs[self.active];
            if tab.is_markdown() {
                tab.md_mode = tab.md_mode.next();
            }
        }

        // Tab switching
        let n = self.tabs.len();
        if hit(Modifiers::CTRL, Key::Tab) {
            self.active = (self.active + 1) % n;
        }
        if hit(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
            self.active = (self.active + n - 1) % n;
        }

        // Code editing shortcuts:
        let id = self.editor_id();
        let is_editor_focused = ctx.memory(|m| match m.focused() {
            Some(focused_id) => focused_id == id,
            None => true,
        });
        let state = egui::TextEdit::load_state(ctx, id);

        if let Some(mut state) = state {
            let char_range = state.cursor.char_range();
            let (cursor_start, cursor_end) = char_range
                .map(|r| (r.primary.index.0, r.secondary.index.0))
                .unwrap_or((0, 0));

            // Toggle comment (⌘/)
            if hit(cmd, Key::Slash) {
                let tab = &mut self.tabs[self.active];
                let (new_text, _, _) = editor_ops::toggle_comment(&tab.text, cursor_start, cursor_end, &tab.syntax);
                tab.text = new_text;
                tab.recalculate_line_numbers();
            }

            // Duplicate line (⌘⇧D)
            if hit(cmd_shift, Key::D) {
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::duplicate_line(&tab.text, cursor_start);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                state.clone().store(ctx, id);
            }

            // Move line up (⌥↑) / down (⌥↓)
            if hit(alt, Key::ArrowUp) {
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::move_line(&tab.text, cursor_start, true);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                state.clone().store(ctx, id);
            }
            if hit(alt, Key::ArrowDown) {
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::move_line(&tab.text, cursor_start, false);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                state.clone().store(ctx, id);
            }

            // Tab / Shift-Tab Indent & Dedent
            if is_editor_focused && hit(Modifiers::NONE, Key::Tab) {
                let tab = &mut self.tabs[self.active];
                if !tab.read_only {
                    if cursor_start != cursor_end {
                        let (new_text, s, e) = editor_ops::indent_selection(&tab.text, cursor_start, cursor_end, false, self.settings.tab_size);
                        tab.text = new_text;
                        tab.recalculate_line_numbers();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
                        state.clone().store(ctx, id);
                    } else {
                        let indent = " ".repeat(self.settings.tab_size);
                        let chars: Vec<char> = tab.text.chars().collect();
                        let idx = cursor_start.min(chars.len());
                        let mut new_text = String::with_capacity(tab.text.len() + indent.len());
                        new_text.extend(chars[..idx].iter());
                        new_text.push_str(&indent);
                        new_text.extend(chars[idx..].iter());
                        tab.text = new_text;
                        tab.recalculate_line_numbers();
                        let new_pos = idx + self.settings.tab_size;
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_pos))));
                        state.clone().store(ctx, id);
                    }
                }
            }
            if is_editor_focused && hit(Modifiers::SHIFT, Key::Tab) {
                let tab = &mut self.tabs[self.active];
                if !tab.read_only {
                    let (new_text, s, e) = editor_ops::indent_selection(&tab.text, cursor_start, cursor_end, true, self.settings.tab_size);
                    tab.text = new_text;
                    tab.recalculate_line_numbers();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
                    state.clone().store(ctx, id);
                }
            }

            // Auto-indent on Enter
            if hit(Modifiers::NONE, Key::Enter) {
                let tab = &mut self.tabs[self.active];
                let indent_insert = editor_ops::compute_auto_indent(&tab.text, cursor_start, self.settings.tab_size);
                let chars: Vec<char> = tab.text.chars().collect();
                let min_c = cursor_start.min(cursor_end);
                let max_c = cursor_start.max(cursor_end);
                let mut new_text = String::with_capacity(tab.text.len() + indent_insert.len());
                new_text.extend(chars[..min_c].iter());
                new_text.push_str(&indent_insert);
                new_text.extend(chars[max_c..].iter());
                tab.text = new_text;
                tab.recalculate_line_numbers();
                let new_pos = min_c + indent_insert.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_pos))));
                state.clone().store(ctx, id);
            }

            // Auto-close pairs & wrap selection
            if is_editor_focused && self.settings.auto_close_pairs {
                let mut char_to_insert = None;
                ctx.input_mut(|i| {
                    if let Some(pos) = i.events.iter().position(|e| match e {
                        egui::Event::Text(s) => matches!(s.as_str(), "(" | "[" | "{" | "\"" | "'" | "`" | ")" | "]" | "}"),
                        _ => false,
                    }) {
                        if let egui::Event::Text(s) = i.events.remove(pos) {
                            if let Some(ch) = s.chars().next() {
                                char_to_insert = Some(ch);
                            }
                        }
                    }
                });

                if let Some(ch) = char_to_insert {
                    let tab = &mut self.tabs[self.active];
                    if !tab.read_only {
                        if matches!(ch, ')' | ']' | '}') && editor_ops::should_skip_closing_char(&tab.text, cursor_start, ch) && cursor_start == cursor_end {
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_start + 1))));
                            state.clone().store(ctx, id);
                        } else if let Some((new_text, s, e)) = editor_ops::handle_auto_close_pair(&tab.text, cursor_start, cursor_end, ch) {
                            tab.text = new_text;
                            tab.recalculate_line_numbers();
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
                            state.clone().store(ctx, id);
                        } else {
                            let chars: Vec<char> = tab.text.chars().collect();
                            let min_c = cursor_start.min(cursor_end);
                            let max_c = cursor_start.max(cursor_end);
                            let mut new_text = String::with_capacity(tab.text.len() + 1);
                            new_text.extend(chars[..min_c].iter());
                            new_text.push(ch);
                            new_text.extend(chars[max_c..].iter());
                            tab.text = new_text;
                            tab.recalculate_line_numbers();
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(min_c + 1))));
                            state.clone().store(ctx, id);
                        }
                    }
                }

                // Auto-backspace pair deletion
                if cursor_start == cursor_end && ctx.input(|i| i.key_pressed(Key::Backspace)) {
                    let tab = &mut self.tabs[self.active];
                    if !tab.read_only {
                        if let Some((new_text, new_pos)) = editor_ops::handle_auto_backspace_pair(&tab.text, cursor_start) {
                            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace));
                            tab.text = new_text;
                            tab.recalculate_line_numbers();
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_pos))));
                            state.clone().store(ctx, id);
                        }
                    }
                }
            }
        }
    }

    fn entries(&mut self, dir: &Path) -> Vec<Entry> {
        self.dir_cache
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let mut v: Vec<Entry> = fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| Entry {
                        path: e.path(),
                        name: e.file_name().to_string_lossy().into_owned(),
                        is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                    })
                    .filter(|e| !HIDDEN.contains(&e.name.as_str()))
                    .collect();
                v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
                v
            })
            .clone()
    }

    fn tree(&mut self, ui: &mut egui::Ui, dir: &Path, to_open: &mut Option<PathBuf>, open_paths: &HashSet<PathBuf>) {
        for e in self.entries(dir) {
            let dim = e.name.starts_with('.');
            let git_status = self.git.status_for(&e.path);

            if e.is_dir {
                let mut label_text = format!("📁 {}", e.name);
                if self.git.dir_modified(&e.path) {
                    label_text.push_str(" ●");
                }
                let mut label = RichText::new(label_text);
                if dim {
                    label = label.color(Color32::GRAY);
                } else if self.git.dir_modified(&e.path) {
                    label = label.color(Color32::from_rgb(229, 192, 123));
                }

                let response = egui::CollapsingHeader::new(label)
                    .id_salt(&e.path)
                    .default_open(e.name == "src")
                    .show(ui, |ui| self.tree(ui, &e.path, to_open, open_paths));

                response.header_response.context_menu(|ui| {
                    self.context_menu_for_path(ui, &e.path, true);
                });
            } else {
                let active = open_paths.contains(&e.path);
                let bg = if active {
                    Color32::from_rgba_unmultiplied(97, 175, 239, 38)
                } else {
                    Color32::TRANSPARENT
                };

                let frame = egui::Frame::NONE
                    .fill(bg)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(4, 3));

                let frame_res = frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let text_color = if active {
                            ui.visuals().strong_text_color()
                        } else if dim {
                            Color32::GRAY
                        } else {
                            ui.visuals().text_color()
                        };
                        let mut text = RichText::new(format!("📄 {}", e.name)).color(text_color);
                        if active {
                            text = text.strong();
                        }
                        let label_resp = ui.label(text);

                        if let Some(status) = git_status {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (badge, color) = match status {
                                    FileGitStatus::Modified => ("M", Color32::from_rgb(209, 154, 102)),
                                    FileGitStatus::Untracked => ("U", Color32::from_rgb(152, 195, 121)),
                                    FileGitStatus::Staged => ("A", Color32::from_rgb(97, 175, 239)),
                                    FileGitStatus::Deleted => ("D", Color32::from_rgb(224, 108, 117)),
                                };
                                ui.label(RichText::new(badge).small().strong().color(color));
                            });
                        }
                        label_resp
                    });
                });

                let resp = frame_res.response.interact(egui::Sense::click());
                if resp.clicked() {
                    *to_open = Some(e.path.clone());
                }
                resp.context_menu(|ui| {
                    self.context_menu_for_path(ui, &e.path, false);
                });
            }
        }
    }

    fn context_menu_for_path(&mut self, ui: &mut egui::Ui, path: &Path, is_dir: bool) {
        let parent = if is_dir { path.to_path_buf() } else { path.parent().unwrap_or(path).to_path_buf() };

        if ui.button("📄 New File…").clicked() {
            self.tree_modal = Some(TreeModal::NewFile(parent.clone()));
            self.modal_input.clear();
            ui.close();
        }
        if ui.button("📁 New Folder…").clicked() {
            self.tree_modal = Some(TreeModal::NewFolder(parent));
            self.modal_input.clear();
            ui.close();
        }
        ui.separator();
        if ui.button("✏ Rename…").clicked() {
            self.tree_modal = Some(TreeModal::Rename(path.to_path_buf()));
            self.modal_input = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.close();
        }
        if ui.button("🗑 Delete").clicked() {
            self.tree_modal = Some(TreeModal::Delete(path.to_path_buf()));
            ui.close();
        }
        ui.separator();
        if ui.button("Reveal in Finder").clicked() {
            let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
            ui.close();
        }
        if ui.button("Copy Path").clicked() {
            ui.ctx().copy_text(path.display().to_string());
            ui.close();
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.root.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                if ui.button("📁 Open Folder…").clicked() {
                    self.open_folder_dialog();
                }
                ui.add_space(6.0);
                if ui.button("📄 Open File…").clicked() {
                    self.open_file_dialog();
                }
            });

            if !self.recent_projects.is_empty() {
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(
                    RichText::new("RECENT PROJECTS")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(130, 140, 155)),
                );
                ui.add_space(4.0);
                let mut open_proj = None;
                for p in self.recent_projects.iter().take(6) {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                    if ui.button(format!("📁 {name}")).on_hover_text(p.display().to_string()).clicked() {
                        open_proj = Some(p.clone());
                    }
                }
                if let Some(p) = open_proj {
                    self.set_root(Some(&p));
                }
            }

            if !self.recent_files.is_empty() {
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(
                    RichText::new("RECENT FILES")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(130, 140, 155)),
                );
                ui.add_space(4.0);
                let mut open_file = None;
                for p in self.recent_files.iter().take(8) {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                    if ui.button(format!("📄 {name}")).on_hover_text(p.display().to_string()).clicked() {
                        open_file = Some(p.clone());
                    }
                }
                if let Some(p) = open_file {
                    self.open_file(&p);
                }
            }
            return;
        };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("PROJECT EXPLORER")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(130, 140, 155)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("⟳").on_hover_text("Refresh").clicked() {
                    self.dir_cache.clear();
                    self.git.refresh(Some(&root));
                    self.fuzzy.refresh();
                }
            });
        });
        ui.add_space(3.0);
        ui.separator();
        ui.add_space(3.0);

        let active_path: HashSet<PathBuf> = self.tabs.get(self.active).and_then(|t| t.path.clone()).into_iter().collect();
        let mut to_open = None;
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            self.tree(ui, &root, &mut to_open, &active_path);

            if !self.recent_files.is_empty() {
                ui.add_space(12.0);
                ui.separator();
                ui.collapsing("Recent Files", |ui| {
                    let mut open_f = None;
                    for p in self.recent_files.iter().take(6) {
                        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
                        if ui.button(format!("📄 {name}")).on_hover_text(p.display().to_string()).clicked() {
                            open_f = Some(p.clone());
                        }
                    }
                    if let Some(p) = open_f {
                        to_open = Some(p);
                    }
                });
            }
        });

        if let Some(p) = to_open {
            self.open_file(&p);
        }
    }

    fn main_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            if ui.button("⚡ Commands").on_hover_text("Command Palette (⌘⇧P)").clicked() {
                self.command_palette.open_palette();
            }

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            if ui.button("📂 Open").on_hover_text("Open File (⌘O)").clicked() {
                self.open_file_dialog();
            }
            if ui.button("📁 Folder").on_hover_text("Open Project Folder (⌘⇧O)").clicked() {
                self.open_folder_dialog();
            }

            let can_save = self.tabs.get(self.active).map(|t| t.dirty() || t.path.is_none()).unwrap_or(false);
            if ui.add_enabled(can_save, egui::Button::new("💾 Save")).on_hover_text("Save File (⌘S)").clicked() {
                self.save(self.active, false);
            }

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            if ui.button("🔍 Find").on_hover_text("Find & Replace in File (⌘F)").clicked() {
                self.find_open = !self.find_open;
                self.find_focus = self.find_open;
            }
            if ui.button("🔎 Search").on_hover_text("Search across all files (⌘⇧F)").clicked() {
                self.project_search.open = !self.project_search.open;
            }
            if ui.button("🎯 Quick Open").on_hover_text("Fuzzy File Finder (⌘P)").clicked() {
                self.fuzzy.open_palette();
            }

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            let split_label = if self.split_view { "◫ Split [ON]" } else { "◫ Split" };
            if ui.selectable_label(self.split_view, split_label).on_hover_text("Toggle Side-by-Side Split View (⌘⌥\\)").clicked() {
                self.split_view = !self.split_view;
                if self.split_view && self.tabs.len() >= 2 && self.split_active == self.active {
                    self.split_active = (self.active + 1) % self.tabs.len();
                }
            }

            let term_label = if self.terminal.open { "💻 Terminal [ON]" } else { "💻 Terminal" };
            if ui.selectable_label(self.terminal.open, term_label).on_hover_text("Toggle Terminal & Tasks Drawer (⌃`)").clicked() {
                self.terminal.toggle(self.root.as_deref());
            }

            let sidebar_label = if self.show_sidebar { "◧ Sidebar" } else { "◨ Sidebar" };
            if ui.button(sidebar_label).on_hover_text("Toggle File Tree Sidebar (⌘B)").clicked() {
                self.show_sidebar = !self.show_sidebar;
            }

            self.language_picker(ui);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ Settings").on_hover_text("Preferences & Themes (⌘,)").clicked() {
                    self.settings_open = !self.settings_open;
                }
                if let Some(ref branch) = self.git.branch {
                    ui.label(RichText::new(format!(" {branch}")).small().color(Color32::from_rgb(97, 175, 239)));
                }
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        let mut close_others = None;
        let mut save_idx = None;
        let mut rename_idx = None;

        ui.spacing_mut().item_spacing.x = 4.0;

        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let is_active = i == self.active;
                        let bg = if is_active {
                            ui.visuals().extreme_bg_color
                        } else {
                            Color32::TRANSPARENT
                        };

                        let stroke = if is_active {
                            egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.8))
                        } else {
                            egui::Stroke::NONE
                        };

                        let frame = egui::Frame::NONE
                            .fill(bg)
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 })
                            .inner_margin(egui::Margin::symmetric(10, 5));

                        let frame_res = frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;

                                let mut title_text = RichText::new(tab.title());
                                if is_active {
                                    title_text = title_text.strong().color(ui.visuals().strong_text_color());
                                } else {
                                    title_text = title_text.color(ui.visuals().text_color().gamma_multiply(0.75));
                                }
                                ui.add(egui::Label::new(title_text).selectable(false));

                                if tab.dirty() {
                                    ui.colored_label(Color32::from_rgb(229, 192, 123), "●");
                                }

                                ui.add(
                                    egui::Button::new(RichText::new("×").size(13.0))
                                        .frame(false)
                                        .min_size(egui::vec2(16.0, 16.0)),
                                )
                                .on_hover_text("Close Tab (⌘W)")
                            })
                            .inner
                        });

                        let close_btn = frame_res.inner;
                        let tab_r = frame_res.response.interact(egui::Sense::click());
                        let tab_r = match &tab.path {
                            Some(p) => tab_r.on_hover_text(p.display().to_string()),
                            None => tab_r,
                        };

                        let click_pos = ui.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos()));
                        let clicked_on_close = click_pos.map_or(false, |pos| close_btn.rect.expand(2.0).contains(pos));
                        let close_clicked = close_btn.clicked()
                            || (tab_r.clicked() && clicked_on_close)
                            || (ui.input(|i| i.pointer.primary_clicked()) && clicked_on_close);

                        if close_clicked {
                            close = Some(i);
                        } else if tab_r.clicked() {
                            self.active = i;
                        }

                        if tab_r.double_clicked() && !clicked_on_close {
                            rename_idx = Some(i);
                        }
                        if tab_r.middle_clicked() {
                            close = Some(i);
                        }

                        tab_r.context_menu(|ui| {
                            if ui.button("✏ Rename…").clicked() {
                                rename_idx = Some(i);
                                ui.close();
                            }
                            if ui.button("💾 Save").clicked() {
                                save_idx = Some((i, false));
                                ui.close();
                            }
                            if ui.button("💾 Save As…").clicked() {
                                save_idx = Some((i, true));
                                ui.close();
                            }
                            if let Some(p) = &tab.path {
                                ui.separator();
                                if ui.button("Reveal in Finder").clicked() {
                                    let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
                                    ui.close();
                                }
                                if ui.button("Copy Full Path").clicked() {
                                    ui.ctx().copy_text(p.display().to_string());
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("Close Tab").clicked() {
                                close = Some(i);
                                ui.close();
                            }
                            if ui.button("Close Other Tabs").clicked() {
                                close_others = Some(i);
                                ui.close();
                            }
                        });
                    }

                    // "+" button to create a new tab file
                    ui.add_space(2.0);
                    let add_tab = ui.add(
                        egui::Button::new(RichText::new("+").size(15.0).strong())
                            .frame(false)
                            .min_size(egui::vec2(22.0, 20.0)),
                    )
                    .on_hover_text("New Tab (⌘N)");

                    if add_tab.clicked() {
                        self.tabs.push(Tab::untitled());
                        self.active = self.tabs.len() - 1;
                        self.save_session_state();
                    }
                });
            });

        if let Some((idx, save_as)) = save_idx {
            self.save(idx, save_as);
        }
        if let Some(idx) = rename_idx {
            if let Some(p) = self.tabs.get(idx).and_then(|t| t.path.clone()) {
                self.tree_modal = Some(TreeModal::Rename(p.clone()));
                self.modal_input = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            } else {
                self.save(idx, true);
            }
        }
        if let Some(keep_idx) = close_others {
            self.close_other_tabs(keep_idx);
        }
        if let Some(i) = close {
            self.request_close(i);
        }
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        if self.split_view && self.tabs.len() >= 2 {
            if self.split_active >= self.tabs.len() || self.split_active == self.active {
                self.split_active = (self.active + 1) % self.tabs.len();
            }
            let half = (ui.available_width() - 8.0) / 2.0;
            let mut close_split = false;
            let split_idx = self.split_active;

            egui::Panel::right("split_editor_right_panel")
                .resizable(true)
                .default_size(half)
                .size_range(180.0..=(ui.available_width() - 180.0).max(180.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let tab_title = self.tabs.get(split_idx).map(|t| t.title()).unwrap_or_else(|| "Tab".into());
                        let is_dirty = self.tabs.get(split_idx).map(|t| t.dirty()).unwrap_or(false);
                        ui.label(RichText::new(format!("◫ {}{tab_title}", if is_dirty { "● " } else { "" })).strong());

                        ui.menu_button("▾", |ui| {
                            let mut pick_idx = None;
                            for (i, t) in self.tabs.iter().enumerate() {
                                if i != self.active {
                                    if ui.selectable_label(i == split_idx, t.title()).clicked() {
                                        pick_idx = Some(i);
                                        ui.close();
                                    }
                                }
                            }
                            if let Some(i) = pick_idx {
                                self.split_active = i;
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Close split pane").clicked() {
                                close_split = true;
                            }
                        });
                    });
                    ui.separator();
                    self.render_tab_pane(ui, split_idx);
                });

            if close_split {
                self.split_view = false;
            }

            self.render_tab_pane(ui, self.active);
        } else {
            self.render_tab_pane(ui, self.active);
        }
    }

    fn render_tab_pane(&mut self, ui: &mut egui::Ui, tab_idx: usize) {
        if tab_idx >= self.tabs.len() {
            return;
        }

        let mut reload = false;
        let mut ignore = false;
        {
            let tab = &self.tabs[tab_idx];
            if tab.disk_changed {
                egui::Panel::top(egui::Id::unique(("disk_alert", tab_idx))).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::from_rgb(229, 192, 123), "⚠ File modified on disk by another program.");
                        if ui.button("Reload from disk").clicked() {
                            reload = true;
                        }
                        if ui.button("Ignore / Keep local changes").clicked() {
                            ignore = true;
                        }
                    });
                });
            }
        }

        if reload {
            let tab = &mut self.tabs[tab_idx];
            if let Some(p) = &tab.path {
                if let Ok(bytes) = fs::read(p) {
                    if let Ok(s) = String::from_utf8(bytes) {
                        tab.text = s.clone();
                        tab.saved = s;
                        tab.disk_changed = false;
                        tab.last_modified = fs::metadata(p).and_then(|m| m.modified()).ok();
                        tab.recalculate_line_numbers();
                    }
                }
            }
        }
        if ignore {
            let tab = &mut self.tabs[tab_idx];
            tab.disk_changed = false;
            if let Some(p) = &tab.path {
                tab.last_modified = fs::metadata(p).and_then(|m| m.modified()).ok();
            }
        }

        let tab = &mut self.tabs[tab_idx];
        match &mut tab.view {
            View::Image(v) => return v.ui(ui),
            View::Pdf(v) => return v.ui(ui),
            View::Text => {}
        }

        if !tab.is_markdown() {
            return self.render_text_editor(ui, tab_idx);
        }

        // Markdown: Source / Split / Preview
        egui::Panel::top(egui::Id::unique(("md_toolbar", tab_idx))).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (mode, label) in [(MdMode::Source, "Source"), (MdMode::Split, "Split"), (MdMode::Preview, "Preview")] {
                    if ui.selectable_label(tab.md_mode == mode, label).clicked() {
                        tab.md_mode = mode;
                    }
                }
                ui.label(RichText::new("Ctrl+Shift+M to cycle").small().color(Color32::GRAY));
            });
        });

        match tab.md_mode {
            MdMode::Source => self.render_text_editor(ui, tab_idx),
            MdMode::Preview => Self::md_preview(tab, ui),
            MdMode::Split => {
                let half = ui.available_width() / 2.0;
                egui::Panel::right(egui::Id::unique(("md_preview", tab_idx)))
                    .resizable(true)
                    .default_size(half)
                    .size_range(150.0..=ui.available_width() - 150.0)
                    .show(ui, |ui| Self::md_preview(tab, ui));
                self.render_text_editor(ui, tab_idx);
            }
        }
    }

    fn md_preview(tab: &mut Tab, ui: &mut egui::Ui) {
        let base = tab
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|d| format!("file://{}/", d.display()))
            .unwrap_or_else(|| "file://".into());

        egui::Frame::NONE.inner_margin(12.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("md_preview_scroll").auto_shrink(false).show(ui, |ui| {
                CommonMarkViewer::new()
                    .default_implicit_uri_scheme(base)
                    .max_image_width(Some(ui.available_width() as usize))
                    .show(ui, &mut tab.md_cache, &tab.text);
            });
        });
    }

    fn render_text_editor(&mut self, ui: &mut egui::Ui, tab_idx: usize) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        let is_active = tab_idx == self.active;
        let id = egui::Id::unique(("editor", tab_idx, self.tabs[tab_idx].path.clone()));
        let goto = if is_active { self.goto_offset.take() } else { None };
        let needle_len = self.find_query.chars().count();
        let tab = &mut self.tabs[tab_idx];
        let lang = tab.syntax.clone();
        let theme = CodeTheme::from_style(ui.style());
        let read_only = tab.read_only;
        let wrap = self.settings.word_wrap;
        let show_line_nums = self.settings.show_line_numbers;

        tab.recalculate_line_numbers();
        let line_count = tab.cached_line_count;
        let line_numbers_str = tab.cached_line_numbers.clone();

        // Optimized layouter: on huge files (> 1MB), use plain text highlighting for silky smooth typing
        let is_huge_file = tab.text.len() > 1024 * 1024;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let job = if is_huge_file {
                let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                egui::text::LayoutJob::simple(
                    buf.as_str().to_string(),
                    font_id,
                    ui.visuals().text_color(),
                    if wrap { wrap_width } else { f32::INFINITY },
                )
            } else {
                let mut j = syntax_highlighting::highlight_with(
                    ui.ctx(),
                    ui.style(),
                    &theme,
                    buf.as_str(),
                    &lang,
                    syntax::settings(),
                );
                j.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                j
            };
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let output = egui::ScrollArea::both().id_salt(id).auto_shrink(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                if show_line_nums {
                    ui.add(
                        egui::Label::new(
                            RichText::new(line_numbers_str.trim_end())
                                .monospace()
                                .color(Color32::from_rgb(99, 109, 131)),
                        )
                        .selectable(false),
                    );
                    ui.add_space(6.0);
                }

                egui::TextEdit::multiline(&mut tab.text)
                    .id(id)
                    .code_editor()
                    .interactive(!read_only)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .desired_rows(line_count.max(40))
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner
        });

        let mut out = output.inner;

        if out.response.clicked() || out.response.gained_focus() {
            if !is_active {
                self.active = tab_idx;
            }
        }

        if let Some(byte) = goto {
            let start = tab.text[..byte.min(tab.text.len())].chars().count();
            let range = egui::text::CCursorRange::two(
                egui::text::CCursor::new(start),
                egui::text::CCursor::new(start + needle_len.max(1)),
            );
            out.state.cursor.set_char_range(Some(range));
            out.state.clone().store(ui.ctx(), id);
            let rect = out.galley.pos_from_cursor(egui::text::CCursor::new(start)).translate(out.galley_pos.to_vec2());
            ui.scroll_to_rect(rect.expand(40.0), Some(egui::Align::Center));
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }

        if is_active {
            if let Some(r) = out.cursor_range {
                let idx: usize = r.primary.index.into();
                let before: String = tab.text.chars().take(idx).collect();
                let line = before.matches('\n').count() + 1;
                let col = before.rsplit('\n').next().map(|s| s.chars().count()).unwrap_or(0) + 1;
                let bracket = editor_ops::find_matching_bracket(&tab.text, idx);
                CURSOR.with(|c| *c.borrow_mut() = (line, col, bracket));
            }
        }
    }

    fn language_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.tabs[self.active].syntax.clone();
        let label = RichText::new(format!("{current} ▾")).strong().color(Color32::from_rgb(97, 175, 239));
        ui.menu_button(label, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.lang_filter).hint_text("Filter languages…").desired_width(200.0));
            r.request_focus();
            ui.separator();
            let filter = self.lang_filter.to_lowercase();
            let mut picked = None;
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                if ui.button("Auto-detect").clicked() {
                    let t = &self.tabs[self.active];
                    picked = Some(syntax::detect(t.path.as_deref(), &t.text));
                }
                for name in syntax::all_languages().iter().filter(|n| n.to_lowercase().contains(&filter)) {
                    if ui.selectable_label(*name == current, name).clicked() {
                        picked = Some(name.clone());
                    }
                }
            });
            if let Some(p) = picked {
                self.tabs[self.active].syntax = p;
                self.lang_filter.clear();
                ui.close();
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let tab = &self.tabs[self.active];
        let read_only = tab.read_only && matches!(tab.view, View::Text);
        let media_info = match &tab.view {
            View::Text => None,
            View::Image(v) => Some(v.info()),
            View::Pdf(v) => Some(v.info()),
        };

        let sep = RichText::new("|").color(ui.visuals().text_color().gamma_multiply(0.25));

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            let (l, c, bracket) = CURSOR.with(|c| *c.borrow());
            ui.label(RichText::new(format!("Ln {l}, Col {c}")).small());

            ui.label(sep.clone());
            ui.label(RichText::new("Spaces: 4").small());

            ui.label(sep.clone());
            ui.label(RichText::new("UTF-8").small());

            if read_only {
                ui.label(sep.clone());
                ui.label(RichText::new("read-only").small().color(Color32::from_rgb(224, 108, 117)));
            }

            if let Some(b) = bracket {
                ui.label(sep.clone());
                ui.label(RichText::new(format!("Bracket @ {b}")).small().color(Color32::from_rgb(97, 175, 239)));
            }

            if !self.status.is_empty() {
                ui.label(sep.clone());
                ui.label(RichText::new(&self.status).small().color(Color32::from_rgb(229, 192, 123)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                let term_text = if self.terminal.is_running() {
                    RichText::new("💻 Running task…").small().color(Color32::from_rgb(97, 175, 239))
                } else if self.terminal.open {
                    RichText::new("💻 Terminal: ON").small().color(Color32::from_rgb(152, 195, 121))
                } else {
                    RichText::new("💻 Terminal").small()
                };
                if ui.selectable_label(self.terminal.open, term_text).on_hover_text("Toggle Terminal & Tasks Drawer (⌃`)").clicked() {
                    self.terminal.toggle(self.root.as_deref());
                }
                ui.label(sep.clone());

                if let Some(info) = media_info {
                    ui.label(RichText::new(info).small());
                    return;
                }

                let syntax_label = format!("{} · Two-Face Highlighting", tab.syntax);
                ui.label(RichText::new(syntax_label).small());

                if self.git.branch.is_some() {
                    let has_changes = !self.git.file_statuses.is_empty();
                    if has_changes {
                        ui.label(RichText::new("Git: modified").small().color(Color32::from_rgb(229, 192, 123)));
                    } else {
                        ui.label(RichText::new("Git: clean").small().color(Color32::from_rgb(152, 195, 121)));
                    }
                    ui.label(sep.clone());
                }
            });
        });
    }

    fn find_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Find:");
            let r = ui.add(egui::TextEdit::singleline(&mut self.find_query).desired_width(180.0));
            if self.find_focus {
                r.request_focus();
                self.find_focus = false;
            }

            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                self.find_next(back);
                self.find_focus = true;
            }

            if ui.button("Next").clicked() {
                self.find_next(false);
            }
            if ui.button("Prev").clicked() {
                self.find_next(true);
            }

            ui.separator();
            ui.label("Replace:");
            ui.add(egui::TextEdit::singleline(&mut self.replace_query).desired_width(160.0));
            if ui.button("Replace").clicked() {
                self.replace_current();
            }
            if ui.button("Replace All").clicked() {
                self.replace_all_matches();
            }

            ui.separator();
            ui.checkbox(&mut self.find_case_sensitive, "Aa").on_hover_text("Case sensitive");
            ui.checkbox(&mut self.find_use_regex, ".*").on_hover_text("Regular expression");

            if !self.find_query.is_empty() {
                let matches = search::find_matches(
                    &self.tabs[self.active].text,
                    &self.find_query,
                    self.find_case_sensitive,
                    self.find_use_regex,
                );
                ui.label(RichText::new(format!("{} matches", matches.len())).small());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").clicked() {
                    self.find_open = false;
                }
            });
        });
    }

    fn terminal_drawer_ui(&mut self, ui: &mut egui::Ui) {
        let root = self.root.clone();

        // 1. Top bar of terminal drawer
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new("💻 TERMINAL").size(11.0).strong().color(Color32::from_rgb(130, 140, 155)));

            ui.separator();

            // Project tasks quick buttons (e.g. npm run dev, cargo check, etc.)
            if self.project_tasks.is_empty() {
                ui.label(RichText::new("No package.json / cargo scripts detected").small().color(Color32::GRAY));
            } else {
                egui::ScrollArea::horizontal().max_height(24.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for task in self.project_tasks.iter().take(8) {
                            let btn = egui::Button::new(RichText::new(format!("▶ {}", task.name)).small());
                            if ui.add(btn).on_hover_text(format!("Execute '{}' in native terminal", task.command)).clicked() {
                                self.terminal.run_command(&task.command, root.as_deref());
                            }
                        }
                    });
                });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("Close Terminal (⌃`)").clicked() {
                    self.terminal.open = false;
                }
                if ui.small_button("🧹 Clear").on_hover_text("Clear terminal screen (Ctrl+L)").clicked() {
                    self.terminal.clear();
                }
                if ui.small_button("⟳ Reset").on_hover_text("Restart interactive shell").clicked() {
                    self.terminal.restart_shell(root.as_deref());
                }
            });
        });

        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);

        // 2. Native Interactive Terminal Canvas
        render_native_terminal(ui, &mut self.terminal, root.as_deref());
    }

    // ----- Modals & Palettes -----------------------------------------------

    fn execute_command(&mut self, ctx: &egui::Context, cmd_id: &str) {
        match cmd_id {
            "file.new" => {
                self.tabs.push(Tab::untitled());
                self.active = self.tabs.len() - 1;
            }
            "file.open" => self.open_file_dialog(),
            "file.open_folder" => self.open_folder_dialog(),
            "file.save" => {
                self.save(self.active, false);
            }
            "file.save_as" => {
                self.save(self.active, true);
            }
            "file.close_tab" => self.request_close(self.active),
            "file.close_other_tabs" => self.close_other_tabs(self.active),
            "file.reveal_finder" => {
                if let Some(p) = self.tabs.get(self.active).and_then(|t| t.path.as_deref()) {
                    let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
                }
            }
            "file.copy_path" => {
                if let Some(p) = self.tabs.get(self.active).and_then(|t| t.path.as_deref()) {
                    ctx.copy_text(p.display().to_string());
                    self.status = format!("Copied path: {}", p.display());
                }
            }
            "find.quick_open" => self.fuzzy.open_palette(),
            "find.in_file" => {
                self.find_open = true;
                self.find_focus = true;
            }
            "find.in_project" => self.project_search.open = true,
            "navigate.goto_line" => {
                self.goto_line_open = true;
                self.goto_line_input.clear();
            }
            "edit.toggle_comment" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let (cursor_start, cursor_end) = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| (r.primary.index.0, r.secondary.index.0))
                    .unwrap_or((0, 0));
                let tab = &mut self.tabs[self.active];
                let (new_text, _, _) = editor_ops::toggle_comment(&tab.text, cursor_start, cursor_end, &tab.syntax);
                tab.text = new_text;
                tab.recalculate_line_numbers();
            }
            "edit.duplicate_line" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let cursor_start = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| r.primary.index.0)
                    .unwrap_or(0);
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::duplicate_line(&tab.text, cursor_start);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                    state.store(ctx, id);
                }
            }
            "edit.move_line_up" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let cursor_start = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| r.primary.index.0)
                    .unwrap_or(0);
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::move_line(&tab.text, cursor_start, true);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                    state.store(ctx, id);
                }
            }
            "edit.move_line_down" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let cursor_start = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| r.primary.index.0)
                    .unwrap_or(0);
                let tab = &mut self.tabs[self.active];
                let (new_text, new_cursor) = editor_ops::move_line(&tab.text, cursor_start, false);
                tab.text = new_text;
                tab.recalculate_line_numbers();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor))));
                    state.store(ctx, id);
                }
            }
            "edit.indent" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let (cursor_start, cursor_end) = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| (r.primary.index.0, r.secondary.index.0))
                    .unwrap_or((0, 0));
                let tab = &mut self.tabs[self.active];
                if !tab.read_only {
                    let (new_text, s, e) = editor_ops::indent_selection(&tab.text, cursor_start, cursor_end, false, self.settings.tab_size);
                    tab.text = new_text;
                    tab.recalculate_line_numbers();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
                        state.store(ctx, id);
                    }
                }
            }
            "edit.outdent" => {
                let id = self.editor_id();
                let state = egui::TextEdit::load_state(ctx, id);
                let (cursor_start, cursor_end) = state
                    .and_then(|s| s.cursor.char_range())
                    .map(|r| (r.primary.index.0, r.secondary.index.0))
                    .unwrap_or((0, 0));
                let tab = &mut self.tabs[self.active];
                if !tab.read_only {
                    let (new_text, s, e) = editor_ops::indent_selection(&tab.text, cursor_start, cursor_end, true, self.settings.tab_size);
                    tab.text = new_text;
                    tab.recalculate_line_numbers();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(s), egui::text::CCursor::new(e))));
                        state.store(ctx, id);
                    }
                }
            }
            "view.toggle_sidebar" => self.show_sidebar = !self.show_sidebar,
            "view.toggle_terminal" => self.terminal.toggle(self.root.as_deref()),
            "terminal.clear" => self.terminal.clear(),
            "view.toggle_split" => {
                self.split_view = !self.split_view;
                if self.split_view && self.tabs.len() >= 2 && self.split_active == self.active {
                    self.split_active = (self.active + 1) % self.tabs.len();
                }
            }
            "view.toggle_word_wrap" => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.save_session_state();
            }
            "view.toggle_line_numbers" => {
                self.settings.show_line_numbers = !self.settings.show_line_numbers;
                self.save_session_state();
            }
            "theme.onedark" => {
                self.settings.theme = ThemeName::OneDark;
                apply_theme(ctx, &self.settings);
                self.save_session_state();
            }
            "theme.onelight" => {
                self.settings.theme = ThemeName::OneLight;
                apply_theme(ctx, &self.settings);
                self.save_session_state();
            }
            "theme.githubdark" => {
                self.settings.theme = ThemeName::GitHubDark;
                apply_theme(ctx, &self.settings);
                self.save_session_state();
            }
            "theme.monokai" => {
                self.settings.theme = ThemeName::Monokai;
                apply_theme(ctx, &self.settings);
                self.save_session_state();
            }
            "syntax.autodetect" => {
                let tab = &mut self.tabs[self.active];
                tab.syntax = syntax::detect(tab.path.as_deref(), &tab.text);
            }
            "preferences.open" => self.settings_open = true,
            "app.check_updates" => {
                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                self.settings_open = true;
            }
            "help.about" => self.about_open = true,
            _ => {}
        }
    }

    fn command_palette_modal(&mut self, ctx: &egui::Context) {
        if !self.command_palette.open {
            return;
        }

        let mut close = false;
        let mut execute_id = None;

        egui::Window::new("Command Palette (⌘⇧P)")
            .id(egui::Id::unique("command_palette_window"))
            .collapsible(false)
            .resizable(false)
            .fixed_pos(ctx.input(|i| {
                let size = i.viewport().inner_rect.map(|r| r.size()).unwrap_or(egui::Vec2::new(1000.0, 700.0));
                egui::pos2((size.x - 580.0) / 2.0, 70.0)
            }))
            .fixed_size([580.0, 420.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("⚡");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.command_palette.query)
                            .hint_text("Type a command or action (e.g. split, wrap, theme, git)…")
                            .desired_width(520.0),
                    );
                    r.request_focus();

                    if r.changed() {
                        self.command_palette.update_search();
                    }

                    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        if self.command_palette.selected + 1 < self.command_palette.results.len() {
                            self.command_palette.selected += 1;
                        }
                    }
                    if ui.input(|i| i.key_pressed(Key::ArrowUp)) && self.command_palette.selected > 0 {
                        self.command_palette.selected -= 1;
                    }
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        if let Some(m) = self.command_palette.results.get(self.command_palette.selected) {
                            execute_id = Some(m.command.id);
                        }
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                    if self.command_palette.results.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("No matching commands").color(Color32::GRAY));
                        });
                    } else {
                        for (i, m) in self.command_palette.results.iter().enumerate() {
                            let selected = i == self.command_palette.selected;
                            let cmd = m.command;

                            let response = ui.horizontal(|ui| {
                                let mut text = RichText::new(format!("{}  {}: {}", cmd.icon, cmd.category, cmd.title));
                                if selected {
                                    text = text.strong().color(Color32::from_rgb(97, 175, 239));
                                }
                                let label_res = ui.selectable_label(selected, text);
                                if let Some(sc) = cmd.shortcut {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(sc).monospace().small().color(Color32::from_rgb(130, 140, 155)));
                                    });
                                }
                                label_res
                            }).inner;

                            if response.clicked() {
                                execute_id = Some(cmd.id);
                            }
                        }
                    }
                });
            });

        if let Some(id) = execute_id {
            self.command_palette.close();
            self.execute_command(ctx, id);
        } else if close {
            self.command_palette.close();
        }
    }

    fn goto_line_modal(&mut self, ctx: &egui::Context) {
        if !self.goto_line_open {
            return;
        }

        let mut close = false;
        let mut jump = false;

        egui::Window::new("Go to Line (Ctrl+G)")
            .id(egui::Id::unique("goto_line_window"))
            .collapsible(false)
            .resizable(false)
            .fixed_pos(ctx.input(|i| {
                let size = i.viewport().inner_rect.map(|r| r.size()).unwrap_or(egui::Vec2::new(1000.0, 700.0));
                egui::pos2((size.x - 360.0) / 2.0, 90.0)
            }))
            .fixed_size([360.0, 110.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("Enter line or line:col (e.g. 42 or 42:10):").small());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("📍");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.goto_line_input)
                            .hint_text("line[:col]")
                            .desired_width(260.0),
                    );
                    r.request_focus();

                    if ui.button("Go").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        jump = true;
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                });
            });

        if jump {
            let input = self.goto_line_input.trim();
            let parts: Vec<&str> = input.split(':').collect();
            let target_line = parts.first().and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1);
            let target_col = parts.get(1).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1);

            let tab = &self.tabs[self.active];
            let offset = editor_ops::line_col_to_byte_offset(&tab.text, target_line, target_col);
            self.goto_offset = Some(offset);
            self.goto_line_open = false;
        } else if close {
            self.goto_line_open = false;
        }
    }

    fn fuzzy_palette(&mut self, ctx: &egui::Context) {
        if !self.fuzzy.open {
            return;
        }

        let mut close = false;
        let mut picked = None;

        egui::Window::new("Fuzzy Finder (⌘P)")
            .id(egui::Id::unique("fuzzy_palette"))
            .collapsible(false)
            .resizable(false)
            .fixed_pos(ctx.input(|i| {
                let size = i.viewport().inner_rect.map(|r| r.size()).unwrap_or(egui::Vec2::new(1000.0, 700.0));
                egui::pos2((size.x - 560.0) / 2.0, 70.0)
            }))
            .fixed_size([560.0, 400.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.fuzzy.query)
                            .hint_text("Type a file name…")
                            .desired_width(500.0),
                    );
                    r.request_focus();

                    if r.changed() {
                        self.fuzzy.update_search();
                    }

                    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        if self.fuzzy.selected + 1 < self.fuzzy.results.len() {
                            self.fuzzy.selected += 1;
                        }
                    }
                    if ui.input(|i| i.key_pressed(Key::ArrowUp)) && self.fuzzy.selected > 0 {
                        self.fuzzy.selected -= 1;
                    }
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        if let Some(m) = self.fuzzy.results.get(self.fuzzy.selected) {
                            picked = Some(m.path.clone());
                        }
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    for (i, m) in self.fuzzy.results.iter().enumerate() {
                        let selected = i == self.fuzzy.selected;
                        let text = format!("📄 {}  —  {}", m.filename, m.relative);
                        let mut label = RichText::new(text);
                        if selected {
                            label = label.strong().color(Color32::from_rgb(97, 175, 239));
                        }
                        if ui.selectable_label(selected, label).clicked() {
                            picked = Some(m.path.clone());
                        }
                    }
                });
            });

        if let Some(p) = picked {
            self.open_file(&p);
            self.fuzzy.close();
        } else if close {
            self.fuzzy.close();
        }
    }

    fn project_search_modal(&mut self, ctx: &egui::Context) {
        if !self.project_search.open {
            return;
        }

        let mut close = false;
        let mut jump_target = None;

        egui::Window::new("Find in Project (⌘⇧F)")
            .id(egui::Id::unique("project_search"))
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.project_search.query)
                            .hint_text("Search across all files…")
                            .desired_width(320.0),
                    );
                    r.request_focus();

                    ui.checkbox(&mut self.project_search.case_sensitive, "Aa");
                    ui.checkbox(&mut self.project_search.use_regex, ".*");

                    if ui.button("Search").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                        self.project_search.execute(self.root.as_deref());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            close = true;
                        }
                    });
                });

                if !self.project_search.status.is_empty() {
                    ui.label(RichText::new(&self.project_search.status).small().color(Color32::GRAY));
                }
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for m in &self.project_search.results {
                        let text = format!("{}:{}: {}", m.relative, m.line_number, m.line_text.trim());
                        if ui.selectable_label(false, RichText::new(text).monospace()).clicked() {
                            jump_target = Some((m.path.clone(), m.line_number));
                        }
                    }
                });
            });

        if let Some((path, line_num)) = jump_target {
            self.open_file(&path);
            let tab = &self.tabs[self.active];
            let mut byte_offset = 0;
            for (idx, line) in tab.text.lines().enumerate() {
                if idx + 1 == line_num {
                    break;
                }
                byte_offset += line.len() + 1;
            }
            self.goto_offset = Some(byte_offset);
            self.project_search.open = false;
        } else if close {
            self.project_search.open = false;
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut changed = false;
        egui::Window::new("Settings (⌘,)")
            .id(egui::Id::unique("settings_modal"))
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.heading("Editor Preferences");
                ui.separator();

                // Theme
                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    for theme in ThemeName::ALL {
                        if ui.selectable_label(self.settings.theme == *theme, theme.display_name()).clicked() {
                            self.settings.theme = *theme;
                            changed = true;
                        }
                    }
                });

                ui.add_space(8.0);
                // Font Size
                ui.horizontal(|ui| {
                    ui.label("Font Size:");
                    if ui.add(egui::Slider::new(&mut self.settings.font_size, 10.0..=26.0).suffix(" px")).changed() {
                        changed = true;
                    }
                });

                ui.add_space(8.0);
                // Tab Size
                ui.horizontal(|ui| {
                    ui.label("Tab Size:");
                    for sz in [2, 4, 8] {
                        if ui.selectable_label(self.settings.tab_size == sz, format!("{sz} spaces")).clicked() {
                            self.settings.tab_size = sz;
                            changed = true;
                        }
                    }
                });

                ui.add_space(8.0);
                // Word Wrap
                if ui.checkbox(&mut self.settings.word_wrap, "Enable word wrap").changed() {
                    changed = true;
                }

                // Show Line Numbers
                if ui.checkbox(&mut self.settings.show_line_numbers, "Show line numbers").changed() {
                    changed = true;
                }

                // Smart Auto-closing Pairs
                if ui.checkbox(&mut self.settings.auto_close_pairs, "Auto-close quotes & brackets () [] {} \"\" '' ``").changed() {
                    changed = true;
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("History: {} files, {} projects", self.recent_files.len(), self.recent_projects.len())).small());
                    if ui.button("Clear History").clicked() {
                        self.recent_files.clear();
                        self.recent_projects.clear();
                        changed = true;
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // Updates
                ui.heading("Updates");
                ui.horizontal(|ui| {
                    match &self.updater.state.clone() {
                        UpdateState::Idle => {
                            if ui.button("⟳ Check for Updates").clicked() {
                                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                            }
                        }
                        UpdateState::Checking => {
                            ui.spinner();
                            ui.label("Checking yegz.com for updates…");
                        }
                        UpdateState::UpToDate => {
                            ui.colored_label(Color32::from_rgb(152, 195, 121), "✓ You're on the latest version!");
                            if ui.small_button("Check Again").clicked() {
                                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                            }
                        }
                        UpdateState::Available(info) => {
                            ui.colored_label(Color32::from_rgb(97, 175, 239), format!("🚀 Version {} available!", info.version));
                            if ui.button("⚡ Download & Update").clicked() {
                                self.updater.download_and_install(info.clone());
                            }
                        }
                        UpdateState::Downloading => {
                            ui.spinner();
                            ui.label(RichText::new("Downloading update…").color(Color32::from_rgb(229, 192, 123)));
                        }
                        UpdateState::ReadyToRestart { .. } => {
                            ui.colored_label(Color32::from_rgb(152, 195, 121), "✅ Update downloaded!");
                            if ui.button("🔄 Restart & Apply").clicked() {
                                self.save_session_state();
                                let _ = self.updater.apply_and_restart();
                            }
                        }
                        UpdateState::Error(err) => {
                            ui.colored_label(Color32::from_rgb(224, 108, 117), format!("⚠ {err}"));
                            if ui.small_button("Retry").clicked() {
                                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                            }
                        }
                    }
                });

                if ui.checkbox(&mut self.settings.auto_check_updates, "Automatically check for updates on startup").changed() {
                    changed = true;
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // About Yegz & Links
                ui.heading("About Yegz");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Yegz v{}", env!("CARGO_PKG_VERSION"))).strong());
                    ui.label(RichText::new("— A powerful editor. Without the clutter.").color(ui.visuals().text_color().gamma_multiply(0.7)));
                });

                ui.add_space(6.0);
                egui::Grid::new("about_links_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("🌐 Website:");
                        ui.hyperlink_to("https://yegz.com", "https://yegz.com");
                        ui.end_row();

                        ui.label("📦 Source Code:");
                        ui.hyperlink_to("github.com/zigl/yegz", "https://github.com/zigl/yegz");
                        ui.end_row();

                        ui.label("🚀 Releases:");
                        ui.hyperlink_to("Downloads & Release Notes", "https://github.com/zigl/yegz/releases");
                        ui.end_row();

                        ui.label("🐛 Report Issue:");
                        ui.hyperlink_to("github.com/zigl/yegz/issues", "https://github.com/zigl/yegz/issues");
                        ui.end_row();

                        ui.label("📜 License:");
                        ui.hyperlink_to("MIT License", "https://github.com/zigl/yegz/blob/main/LICENSE");
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Done").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.settings_open = false;
                    }
                });
            });

        if changed {
            apply_theme(ctx, &self.settings);
            self.save_session_state();
        }
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }

        egui::Window::new("About Yegz")
            .id(egui::Id::unique("about_modal"))
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.heading("Yegz");
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).small().color(ui.visuals().text_color().gamma_multiply(0.7)));
                    ui.add_space(4.0);
                    ui.label("A powerful editor. Without the clutter.");
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                egui::Grid::new("about_modal_links")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("🌐 Website:");
                        ui.hyperlink_to("https://yegz.com", "https://yegz.com");
                        ui.end_row();

                        ui.label("📦 Source Code:");
                        ui.hyperlink_to("github.com/zigl/yegz", "https://github.com/zigl/yegz");
                        ui.end_row();

                        ui.label("🚀 Releases:");
                        ui.hyperlink_to("Downloads & Release Notes", "https://github.com/zigl/yegz/releases");
                        ui.end_row();

                        ui.label("🐛 Report Issue:");
                        ui.hyperlink_to("github.com/zigl/yegz/issues", "https://github.com/zigl/yegz/issues");
                        ui.end_row();

                        ui.label("📜 License:");
                        ui.hyperlink_to("MIT License", "https://github.com/zigl/yegz/blob/main/LICENSE");
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    match &self.updater.state.clone() {
                        UpdateState::Idle => {
                            if ui.button("⟳ Check for Updates").clicked() {
                                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                            }
                        }
                        UpdateState::Checking => {
                            ui.spinner();
                            ui.label("Checking…");
                        }
                        UpdateState::UpToDate => {
                            ui.colored_label(Color32::from_rgb(152, 195, 121), "✓ Up to date");
                        }
                        UpdateState::Available(info) => {
                            ui.colored_label(Color32::from_rgb(97, 175, 239), format!("🚀 v{} available", info.version));
                            if ui.button("⚡ Update").clicked() {
                                self.updater.download_and_install(info.clone());
                            }
                        }
                        UpdateState::Downloading => {
                            ui.spinner();
                            ui.label("Downloading…");
                        }
                        UpdateState::ReadyToRestart { .. } => {
                            if ui.button("🔄 Restart to Update").clicked() {
                                self.save_session_state();
                                let _ = self.updater.apply_and_restart();
                            }
                        }
                        UpdateState::Error(err) => {
                            ui.colored_label(Color32::from_rgb(224, 108, 117), "⚠ Check failed").on_hover_text(err);
                            if ui.small_button("Retry").clicked() {
                                self.updater.check_for_updates(env!("CARGO_PKG_VERSION"));
                            }
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.about_open = false;
                    }
                });
            });
    }

    fn update_banner(&mut self, ui: &mut egui::Ui) {
        if self.updater.banner_dismissed {
            return;
        }

        match &self.updater.state.clone() {
            UpdateState::Available(info) => {
                egui::Panel::top("update_banner")
                    .frame(
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(33, 44, 66))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(97, 175, 239)))
                            .inner_margin(egui::Margin::symmetric(12, 6)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🚀").size(14.0));
                            ui.label(RichText::new(format!("Yegz v{} is available!", info.version)).strong().color(Color32::from_rgb(97, 175, 239)));
                            ui.label(RichText::new(&info.notes).small().color(Color32::from_rgb(200, 210, 225)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                                    self.updater.banner_dismissed = true;
                                }
                                if ui.button("⚡ Update Now").on_hover_text("Download and install update automatically").clicked() {
                                    self.updater.download_and_install(info.clone());
                                }
                            });
                        });
                    });
            }
            UpdateState::Downloading => {
                egui::Panel::top("update_banner_dl")
                    .frame(
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(40, 44, 52))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(229, 192, 123)))
                            .inner_margin(egui::Margin::symmetric(12, 6)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Downloading Yegz update in background…").strong().color(Color32::from_rgb(229, 192, 123)));
                        });
                    });
            }
            UpdateState::ReadyToRestart { .. } => {
                egui::Panel::top("update_banner_ready")
                    .frame(
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(30, 50, 40))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(152, 195, 121)))
                            .inner_margin(egui::Margin::symmetric(12, 6)),
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✅").size(14.0));
                            ui.label(RichText::new("Update downloaded and ready!").strong().color(Color32::from_rgb(152, 195, 121)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("🔄 Restart & Apply Update").clicked() {
                                    self.save_session_state();
                                    let _ = self.updater.apply_and_restart();
                                }
                            });
                        });
                    });
            }
            _ => {}
        }
    }

    fn tree_actions_modal(&mut self, ctx: &egui::Context) {
        let Some(modal) = self.tree_modal.clone() else { return };

        let mut done = false;
        let mut action: Option<Box<dyn FnOnce(&mut Editor)>> = None;

        egui::Modal::new(egui::Id::unique("tree_action_modal")).show(ctx, |ui| {
            match &modal {
                TreeModal::NewFile(parent) => {
                    ui.heading("Create New File");
                    ui.label(format!("In {}", parent.display()));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.modal_input).desired_width(280.0));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            let name = self.modal_input.trim().to_string();
                            if !name.is_empty() {
                                let new_path = parent.join(name);
                                action = Some(Box::new(move |ed| {
                                    if fs::write(&new_path, "").is_ok() {
                                        ed.dir_cache.clear();
                                        ed.open_file(&new_path);
                                    }
                                }));
                            }
                            done = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            done = true;
                        }
                    });
                }
                TreeModal::NewFolder(parent) => {
                    ui.heading("Create New Folder");
                    ui.label(format!("In {}", parent.display()));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.modal_input).desired_width(280.0));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            let name = self.modal_input.trim().to_string();
                            if !name.is_empty() {
                                let new_path = parent.join(name);
                                action = Some(Box::new(move |ed| {
                                    let _ = fs::create_dir_all(&new_path);
                                    ed.dir_cache.clear();
                                }));
                            }
                            done = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            done = true;
                        }
                    });
                }
                TreeModal::Rename(target) => {
                    ui.heading("Rename");
                    ui.label(format!("Rename {}", target.file_name().unwrap_or_default().to_string_lossy()));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.modal_input).desired_width(280.0));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            let name = self.modal_input.trim().to_string();
                            let target = target.clone();
                            if !name.is_empty() {
                                if let Some(parent) = target.parent() {
                                    let new_path = parent.join(name);
                                    action = Some(Box::new(move |ed| {
                                        if fs::rename(&target, &new_path).is_ok() {
                                            ed.dir_cache.clear();
                                            for tab in &mut ed.tabs {
                                                if tab.path.as_deref() == Some(&target) {
                                                    tab.path = Some(new_path.clone());
                                                }
                                            }
                                        }
                                    }));
                                }
                            }
                            done = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            done = true;
                        }
                    });
                }
                TreeModal::Delete(target) => {
                    ui.heading("Delete");
                    ui.label(format!("Are you sure you want to permanently delete \"{}\"?", target.file_name().unwrap_or_default().to_string_lossy()));
                    let target = target.clone();
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            action = Some(Box::new(move |ed| {
                                if target.is_dir() {
                                    let _ = fs::remove_dir_all(&target);
                                } else {
                                    let _ = fs::remove_file(&target);
                                }
                                ed.dir_cache.clear();
                                ed.tabs.retain(|t| t.path.as_deref() != Some(&target));
                                if ed.tabs.is_empty() {
                                    ed.tabs.push(Tab::untitled());
                                }
                                ed.active = ed.active.min(ed.tabs.len() - 1);
                            }));
                            done = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            done = true;
                        }
                    });
                }
            }
        });

        if let Some(act) = action {
            act(self);
        }
        if done {
            self.tree_modal = None;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending else { return };
        let msg = match pending {
            Pending::CloseTab(i) => format!("Save changes to \"{}\"?", self.tabs[*i].title()),
            Pending::Quit => {
                let n = self.tabs.iter().filter(|t| t.dirty()).count();
                format!("{n} file(s) have unsaved changes. Save before quitting?")
            }
        };

        let mut choice = None;
        egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(msg);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't Save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    choice = Some(2);
                }
            });
        });

        let Some(choice) = choice else { return };
        let pending = self.pending.take().unwrap();
        match (pending, choice) {
            (Pending::CloseTab(i), 0) => {
                if self.save(i, false) {
                    self.close_tab(i);
                }
            }
            (Pending::CloseTab(i), 1) => self.close_tab(i),
            (Pending::Quit, 0) => {
                let all_saved = (0..self.tabs.len())
                    .filter(|&i| self.tabs[i].dirty())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .all(|i| self.save(i, false));
                if all_saved {
                    self.allow_quit = true;
                    self.save_session_state();
                    std::process::exit(0);
                }
            }
            (Pending::Quit, 1) => {
                self.allow_quit = true;
                self.save_session_state();
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

const HIDDEN: &[&str] = &[
    ".git", ".DS_Store", "node_modules", "target", ".idea", ".vscode",
    "Music", "Pictures", "Movies", "Library", ".Trash",
];

thread_local! {
    static CURSOR: std::cell::RefCell<(usize, usize, Option<usize>)> = const { std::cell::RefCell::new((1, 1, None)) };
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.updater.poll();

        // Check for dropped files (drag and drop from Finder)
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for drop in dropped {
            let path = drop.path();
            if path.is_dir() {
                self.set_root(Some(path));
            } else if path.is_file() {
                self.open_file(path);
            }
        }

        // Intercept window close when unsaved changes exist
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_quit {
            if self.tabs.iter().any(Tab::dirty) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Quit);
            } else {
                self.save_session_state();
                std::process::exit(0);
            }
        }

        self.check_disk_modifications();

        if self.pending.is_none() && !self.fuzzy.open && !self.project_search.open && !self.command_palette.open && !self.goto_line_open && self.tree_modal.is_none() {
            self.shortcuts(&ctx);
        }

        // Window title
        let tab = &self.tabs[self.active];
        let title = format!("{}{} — Yegz", if tab.dirty() { "● " } else { "" }, tab.title());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        if self.terminal.open {
            egui::Panel::bottom("terminal_drawer")
                .resizable(true)
                .default_size(self.terminal.height)
                .size_range(120.0..=600.0)
                .frame(
                    egui::Frame::NONE
                        .fill(ui.visuals().panel_fill)
                        .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.4)))
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                )
                .show(ui, |ui| {
                    self.terminal.height = ui.available_height();
                    self.terminal_drawer_ui(ui);
                });
        }

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill.gamma_multiply(0.9))
                    .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.4)))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| self.status_bar(ui));

        if self.show_sidebar {
            egui::Panel::left("tree")
                .resizable(true)
                .default_size(self.sidebar_width)
                .size_range(140.0..=600.0)
                .frame(
                    egui::Frame::NONE
                        .fill(ui.visuals().panel_fill)
                        .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.4)))
                        .inner_margin(egui::Margin::symmetric(10, 8)),
                )
                .show(ui, |ui| {
                    self.sidebar_width = ui.available_width();
                    self.sidebar(ui);
                });
        }

        egui::Panel::top("main_toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.4)))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ui, |ui| self.main_toolbar(ui));

        egui::Panel::top("tabs")
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill.gamma_multiply(0.8))
                    .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color.gamma_multiply(0.4)))
                    .inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 0 }),
            )
            .show(ui, |ui| self.tab_bar(ui));

        self.update_banner(ui);

        if self.find_open {
            egui::Panel::bottom("find").show(ui, |ui| self.find_bar(ui));
        }

        let bg = match self.settings.theme {
            ThemeName::OneDark => Color32::from_rgb(40, 44, 52),
            ThemeName::OneLight => Color32::from_rgb(250, 250, 250),
            ThemeName::GitHubDark => Color32::from_rgb(13, 17, 23),
            ThemeName::Monokai => Color32::from_rgb(39, 40, 34),
        };

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).fill(bg))
            .show(ui, |ui| self.central(ui));

        self.fuzzy_palette(&ctx);
        self.command_palette_modal(&ctx);
        self.goto_line_modal(&ctx);
        self.project_search_modal(&ctx);
        self.settings_dialog(&ctx);
        self.about_dialog(&ctx);
        self.tree_actions_modal(&ctx);
        self.confirm_dialog(&ctx);
    }
}
