//! Native Interactive Terminal & Task Runner.
//! Powered by portable-pty (real macOS TTY) and vt100 virtual terminal parser.
//! Direct character-by-character interactive shell with zero textboxes.

use eframe::egui::{self, Color32, Key, Rect, Sense, Stroke, Vec2};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTask {
    pub name: String,
    pub command: String,
    pub source: &'static str,
}

pub struct PtyTerminal {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    child: Arc<Mutex<Box<dyn Child + Send>>>,
    pub rows: u16,
    pub cols: u16,
}

impl PtyTerminal {
    pub fn spawn(working_dir: Option<&Path>, rows: u16, cols: u16) -> Result<Self, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to create PTY: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l"); // Login shell to load full environment, nvm, node, cargo, PATH
        if let Some(dir) = working_dir {
            cmd.cwd(dir);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn shell: {e}"))?;

        // Drop the slave side so that the master gets EOF when child exits
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {e}"))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY writer: {e}"))?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 1000)));
        let parser_reader = Arc::clone(&parser);

        // Background reader thread
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut p) = parser_reader.lock() {
                    p.process(&buf[..n]);
                }
            }
        });

        Ok(Self {
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            parser,
            child: Arc::new(Mutex::new(child)),
            rows,
            cols,
        })
    }

    pub fn write_str(&self, s: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(s.as_bytes());
            let _ = w.flush();
        }
    }

    pub fn write_bytes(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows == self.rows && cols == self.cols) || rows == 0 || cols == 0 {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    pub fn is_alive(&self) -> bool {
        if let Ok(mut ch) = self.child.lock() {
            ch.try_wait().map(|status| status.is_none()).unwrap_or(false)
        } else {
            false
        }
    }
}

pub struct TerminalDrawer {
    pub open: bool,
    pub height: f32,
    pub pty: Option<PtyTerminal>,
    pub blink_start: std::time::Instant,
}

impl TerminalDrawer {
    pub fn new() -> Self {
        Self {
            open: false,
            height: 240.0,
            pty: None,
            blink_start: std::time::Instant::now(),
        }
    }

    pub fn toggle(&mut self, working_dir: Option<&Path>) {
        self.open = !self.open;
        if self.open && self.pty.is_none() {
            self.ensure_pty(working_dir, 24, 80);
        }
    }

    pub fn ensure_pty(&mut self, working_dir: Option<&Path>, rows: u16, cols: u16) {
        if self.pty.is_none() || !self.pty.as_ref().map_or(false, |p| p.is_alive()) {
            if let Ok(pty) = PtyTerminal::spawn(working_dir, rows, cols) {
                self.pty = Some(pty);
            }
        }
    }

    pub fn run_command(&mut self, cmd: &str, working_dir: Option<&Path>) {
        self.open = true;
        self.ensure_pty(working_dir, 24, 80);
        if let Some(ref pty) = self.pty {
            pty.write_str(&format!("{cmd}\n"));
        }
    }

    pub fn send_input_bytes(&self, bytes: &[u8]) {
        if let Some(ref pty) = self.pty {
            pty.write_bytes(bytes);
        }
    }

    pub fn is_running(&self) -> bool {
        self.pty.as_ref().map_or(false, |p| p.is_alive())
    }

    pub fn clear(&mut self) {
        if let Some(ref pty) = self.pty {
            pty.write_bytes(b"\x0c"); // Ctrl+L clear
        }
    }

    pub fn restart_shell(&mut self, working_dir: Option<&Path>) {
        self.pty = None;
        self.ensure_pty(working_dir, 24, 80);
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
                    let mut sorted: BTreeMap<&str, &serde_json::Value> = scripts.iter().map(|(k, v)| (k.as_str(), v)).collect();

                    let priority = ["dev", "start", "build", "test", "lint"];
                    for p in priority {
                        if sorted.remove(p).is_some() {
                            tasks.push(ProjectTask {
                                name: format!("npm run {p}"),
                                command: format!("npm run {p}"),
                                source: "package.json",
                            });
                        }
                    }

                    for (name, _) in sorted {
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

/// Converts a vt100 Color to egui Color32.
pub fn vt100_color_to_egui(color: vt100::Color, is_bg: bool) -> Color32 {
    match color {
        vt100::Color::Default => {
            if is_bg {
                Color32::from_rgb(24, 26, 31) // Native terminal dark background
            } else {
                Color32::from_rgb(220, 225, 235) // Native terminal text
            }
        }
        vt100::Color::Idx(idx) => ansi_16_to_color(idx),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

pub fn ansi_16_to_color(idx: u8) -> Color32 {
    match idx {
        0 => Color32::from_rgb(30, 30, 30),
        1 => Color32::from_rgb(224, 108, 117), // Red
        2 => Color32::from_rgb(152, 195, 121), // Green
        3 => Color32::from_rgb(229, 192, 123), // Yellow
        4 => Color32::from_rgb(97, 175, 239),  // Blue
        5 => Color32::from_rgb(198, 120, 221), // Magenta
        6 => Color32::from_rgb(86, 182, 194),  // Cyan
        7 => Color32::from_rgb(171, 178, 191), // Light gray
        8 => Color32::from_rgb(92, 99, 112),   // Dark gray
        9 => Color32::from_rgb(235, 128, 137), // Bright Red
        10 => Color32::from_rgb(168, 215, 133), // Bright Green
        11 => Color32::from_rgb(245, 208, 135), // Bright Yellow
        12 => Color32::from_rgb(117, 195, 255), // Bright Blue
        13 => Color32::from_rgb(218, 140, 241), // Bright Magenta
        14 => Color32::from_rgb(106, 202, 214), // Bright Cyan
        15 => Color32::from_rgb(255, 255, 255), // Bright White
        16..=231 => {
            let i = idx - 16;
            let r = (i / 36) % 6;
            let g = (i / 6) % 6;
            let b = i % 6;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(conv(r), conv(g), conv(b))
        }
        232..=255 => {
            let gray = (idx - 232) * 10 + 8;
            Color32::from_rgb(gray, gray, gray)
        }
    }
}

/// Renders the native interactive terminal canvas.
pub fn render_native_terminal(
    ui: &mut egui::Ui,
    drawer: &mut TerminalDrawer,
    working_dir: Option<&Path>,
) {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let char_width = ui.painter().layout_no_wrap("M".into(), font_id.clone(), Color32::WHITE).size().x;
    let char_size = Vec2::new(char_width.max(7.0), row_height.max(12.0));

    let available_size = ui.available_size();
    let cols = (available_size.x / char_size.x).floor().max(20.0) as u16;
    let rows = (available_size.y / char_size.y).floor().max(4.0) as u16;

    drawer.ensure_pty(working_dir, rows, cols);

    if let Some(ref mut pty) = drawer.pty {
        pty.resize(rows, cols);
    }

    let (response, painter) = ui.allocate_painter(available_size, Sense::click_and_drag());
    let rect = response.rect;

    // Draw dark terminal background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(24, 26, 31));

    // Request focus on click
    if response.clicked() {
        response.request_focus();
    }

    let has_focus = response.has_focus();
    if has_focus {
        // Subtle outline when focused
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Color32::from_rgb(97, 175, 239).gamma_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }

    // Process keyboard inputs when focused
    if has_focus {
        let mut input_bytes = Vec::new();

        ui.input(|i| {
            // Text events
            for event in &i.events {
                match event {
                    egui::Event::Text(text) => {
                        input_bytes.extend_from_slice(text.as_bytes());
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        let ctrl = modifiers.ctrl || modifiers.command;

                        if ctrl {
                            match key {
                                Key::C => input_bytes.push(0x03), // Ctrl+C (SIGINT)
                                Key::D => input_bytes.push(0x04), // Ctrl+D (EOF)
                                Key::L => input_bytes.push(0x0c), // Ctrl+L (Clear screen)
                                Key::Z => input_bytes.push(0x1a), // Ctrl+Z
                                Key::U => input_bytes.push(0x15), // Ctrl+U
                                Key::W => input_bytes.push(0x17), // Ctrl+W
                                Key::A => input_bytes.push(0x01), // Ctrl+A
                                Key::E => input_bytes.push(0x05), // Ctrl+E
                                Key::R => input_bytes.push(0x12), // Ctrl+R
                                _ => {}
                            }
                        } else {
                            match key {
                                Key::Enter => input_bytes.push(b'\r'),
                                Key::Backspace => input_bytes.push(0x7f),
                                Key::Tab => input_bytes.push(b'\t'),
                                Key::Escape => input_bytes.push(0x1b),
                                Key::ArrowUp => input_bytes.extend_from_slice(b"\x1b[A"),
                                Key::ArrowDown => input_bytes.extend_from_slice(b"\x1b[B"),
                                Key::ArrowRight => input_bytes.extend_from_slice(b"\x1b[C"),
                                Key::ArrowLeft => input_bytes.extend_from_slice(b"\x1b[D"),
                                Key::Home => input_bytes.extend_from_slice(b"\x1b[H"),
                                Key::End => input_bytes.extend_from_slice(b"\x1b[F"),
                                Key::PageUp => input_bytes.extend_from_slice(b"\x1b[5~"),
                                Key::PageDown => input_bytes.extend_from_slice(b"\x1b[6~"),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        if !input_bytes.is_empty() {
            drawer.send_input_bytes(&input_bytes);
        }
    }

    // Render cells from vt100 Parser
    let Some(ref pty) = drawer.pty else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Terminal initializing…",
            font_id,
            Color32::GRAY,
        );
        return;
    };

    let (cursor_row, cursor_col, hide_cursor) = if let Ok(p) = pty.parser.lock() {
        let screen = p.screen();
        let (cr, cc) = screen.cursor_position();
        let hide = screen.hide_cursor();

        for row in 0..rows {
            let row_y = rect.min.y + (row as f32) * char_size.y;
            if row_y > rect.max.y {
                break;
            }

            let mut col = 0;
            while col < cols {
                if let Some(cell) = screen.cell(row, col) {
                    let contents = cell.contents();
                    let fg = vt100_color_to_egui(cell.fgcolor(), false);
                    let bg = vt100_color_to_egui(cell.bgcolor(), true);

                    let cell_x = rect.min.x + (col as f32) * char_size.x;
                    let cell_rect = Rect::from_min_size(
                        egui::pos2(cell_x, row_y),
                        char_size,
                    );

                    // Draw custom background if not default
                    if cell.bgcolor() != vt100::Color::Default {
                        painter.rect_filled(cell_rect, 0.0, bg);
                    }

                    if !contents.is_empty() && contents != " " {
                        painter.text(
                            egui::pos2(cell_x, row_y),
                            egui::Align2::LEFT_TOP,
                            &contents,
                            font_id.clone(),
                            fg,
                        );
                    }
                }
                col += 1;
            }
        }
        (cr, cc, hide)
    } else {
        (0, 0, false)
    };

    // Draw native blinking cursor
    if !hide_cursor {
        let elapsed = drawer.blink_start.elapsed().as_millis();
        let blink_on = (elapsed / 500) % 2 == 0 || !has_focus;

        if blink_on {
            let cursor_x = rect.min.x + (cursor_col as f32) * char_size.x;
            let cursor_y = rect.min.y + (cursor_row as f32) * char_size.y;
            let cursor_rect = Rect::from_min_size(
                egui::pos2(cursor_x, cursor_y),
                Vec2::new(char_size.x, char_size.y),
            );

            if has_focus {
                painter.rect_filled(
                    cursor_rect,
                    1.0,
                    Color32::from_rgb(97, 175, 239).gamma_multiply(0.75),
                );
            } else {
                painter.rect_stroke(
                    cursor_rect,
                    1.0,
                    Stroke::new(1.0, Color32::from_rgb(130, 140, 155)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vt100_parser_and_colors() {
        let mut parser = vt100::Parser::new(24, 80, 100);
        parser.process(b"\x1b[32mSuccess\x1b[0m\r\n");
        let screen = parser.screen();
        let (r, c) = screen.cursor_position();
        assert_eq!(r, 1);
        assert_eq!(c, 0);

        if let Some(cell) = screen.cell(0, 0) {
            assert_eq!(cell.contents(), "S");
            assert_eq!(cell.fgcolor(), vt100::Color::Idx(2));
        }
    }

    #[test]
    fn test_ansi_color_conversion() {
        let red = ansi_16_to_color(1);
        assert_eq!(red, Color32::from_rgb(224, 108, 117));
        let green = ansi_16_to_color(2);
        assert_eq!(green, Color32::from_rgb(152, 195, 121));
    }

    #[test]
    fn test_detect_package_json() {
        let temp_dir = std::env::temp_dir().join("yegz_test_pkg_native");
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
