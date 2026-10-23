//! Non-text viewers: images (PNG, JPEG, GIF animated, WebP, BMP, SVG, etc.) and PDFs.

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions, Vec2};
use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "svg",
];
pub const PDF_EXTS: &[&str] = &["pdf"];

const PDF_RENDER_SCALE: f32 = 2.0;
const PAGE_GAP: f32 = 14.0;

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Shared zoom state + toolbar for both viewers.
#[derive(Clone, Copy)]
pub struct Zoom {
    pub factor: Option<f32>,
}

impl Zoom {
    pub fn new() -> Self {
        Self { factor: None }
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui, effective: f32, info: &str) {
        ui.horizontal(|ui| {
            if ui.button("−").on_hover_text("Zoom out").clicked() {
                self.factor = Some((effective / 1.25).max(0.05));
            }
            ui.label(RichText::new(format!("{:.0}%", effective * 100.0)).monospace());
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                self.factor = Some((effective * 1.25).min(16.0));
            }
            if ui.selectable_label(self.factor == Some(1.0), "1:1").clicked() {
                self.factor = Some(1.0);
            }
            if ui.selectable_label(self.factor.is_none(), "Fit").clicked() {
                self.factor = None;
            }
            ui.separator();
            ui.label(RichText::new(info).small().color(Color32::GRAY));
        });
        let delta = ui.input(|i| i.zoom_delta());
        if delta != 1.0 && ui.ui_contains_pointer() {
            self.factor = Some((effective * delta).clamp(0.05, 16.0));
        }
    }
}

// ---------------------------------------------------------------------------
// Animated & Static Image Viewer (incl. SVG)

struct FrameData {
    image: ColorImage,
    delay: Duration,
    tex: Option<TextureHandle>,
}

pub struct ImageView {
    frames: Vec<FrameData>,
    total_duration: Duration,
    start_time: Instant,
    size: [usize; 2],
    is_svg: bool,
    zoom: Zoom,
}

impl ImageView {
    pub fn open(bytes: &[u8], path: &Path) -> Result<Self, String> {
        let ext = ext_of(path);
        if ext == "svg" {
            return Self::open_svg(bytes);
        }

        if ext == "gif" {
            if let Ok(animated) = Self::open_gif(bytes) {
                return Ok(animated);
            }
        }

        // Standard static image
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        Ok(Self {
            frames: vec![FrameData {
                image: color,
                delay: Duration::from_millis(100),
                tex: None,
            }],
            total_duration: Duration::from_millis(100),
            start_time: Instant::now(),
            size,
            is_svg: false,
            zoom: Zoom::new(),
        })
    }

    fn open_svg(bytes: &[u8]) -> Result<Self, String> {
        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| e.to_string())?;
        let w = (tree.size().width().ceil() as u32).max(1);
        let h = (tree.size().height().ceil() as u32).max(1);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("Cannot allocate SVG pixmap")?;
        resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
        let size = [w as usize, h as usize];
        let color = ColorImage::from_rgba_unmultiplied(size, pixmap.data());
        Ok(Self {
            frames: vec![FrameData {
                image: color,
                delay: Duration::from_millis(100),
                tex: None,
            }],
            total_duration: Duration::from_millis(100),
            start_time: Instant::now(),
            size,
            is_svg: true,
            zoom: Zoom::new(),
        })
    }

    fn open_gif(bytes: &[u8]) -> Result<Self, String> {
        let decoder = GifDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let gif_frames = decoder.into_frames().collect_frames().map_err(|e| e.to_string())?;
        if gif_frames.is_empty() {
            return Err("Empty GIF".into());
        }

        let first = &gif_frames[0];
        let size = [first.buffer().width() as usize, first.buffer().height() as usize];

        let mut total_duration = Duration::ZERO;
        let mut frames = Vec::with_capacity(gif_frames.len());

        for f in gif_frames {
            let (num, denom) = f.delay().numer_denom_ms();
            let millis = if denom > 0 { (num as f32 / denom as f32) as u64 } else { 100 };
            let delay = Duration::from_millis(millis.max(20));
            total_duration += delay;
            let color = ColorImage::from_rgba_unmultiplied(
                [f.buffer().width() as usize, f.buffer().height() as usize],
                f.buffer().as_raw(),
            );
            frames.push(FrameData {
                image: color,
                delay,
                tex: None,
            });
        }

        if total_duration.is_zero() {
            total_duration = Duration::from_millis(100 * frames.len() as u64);
        }

        Ok(Self {
            frames,
            total_duration,
            start_time: Instant::now(),
            size,
            is_svg: false,
            zoom: Zoom::new(),
        })
    }

    pub fn info(&self) -> String {
        let kind = if self.is_svg {
            "SVG"
        } else if self.frames.len() > 1 {
            "Animated GIF"
        } else {
            "Image"
        };
        if self.frames.len() > 1 {
            format!("{kind} {}×{} · {} frames", self.size[0], self.size[1], self.frames.len())
        } else {
            format!("{kind} {}×{}", self.size[0], self.size[1])
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.frames.is_empty() {
            return;
        }

        // Determine current frame
        let frame_idx = if self.frames.len() <= 1 {
            0
        } else {
            let elapsed_ms = (self.start_time.elapsed().as_millis() % self.total_duration.as_millis().max(1)) as u64;
            let mut acc = 0;
            let mut idx = 0;
            for (i, f) in self.frames.iter().enumerate() {
                acc += f.delay.as_millis() as u64;
                if elapsed_ms < acc {
                    idx = i;
                    break;
                }
            }
            // Request repaint for animated GIF
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            idx
        };

        let info = self.info();

        // Ensure current frame texture is loaded
        let frame = &mut self.frames[frame_idx];
        if frame.tex.is_none() {
            frame.tex = Some(ui.ctx().load_texture(
                format!("frame_{frame_idx}"),
                frame.image.clone(),
                TextureOptions::LINEAR,
            ));
        }
        let tex = frame.tex.as_ref().unwrap();

        let img_size = Vec2::new(self.size[0] as f32, self.size[1] as f32);
        let avail = ui.available_size() - Vec2::new(16.0, 48.0);
        let fit = (avail.x / img_size.x).min(avail.y / img_size.y).min(1.0).max(0.01);
        let effective = self.zoom.factor.unwrap_or(fit);

        self.zoom.toolbar(ui, effective, &info);
        ui.separator();

        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let size = img_size * effective;
            let pad = ((ui.available_size() - size) / 2.0).max(Vec2::ZERO);
            ui.add_space(pad.y);
            ui.horizontal(|ui| {
                ui.add_space(pad.x);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                paint_checkerboard(ui, rect);
                egui::Image::new(tex).paint_at(ui, rect);
            });
        });
    }
}

fn paint_checkerboard(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(50));
    let s = 12.0;
    let vis = rect.intersect(ui.clip_rect());
    if vis.width() <= 0.0 || vis.height() <= 0.0 {
        return;
    }
    let x0 = ((vis.min.x - rect.min.x) / s).floor() as i32;
    let y0 = ((vis.min.y - rect.min.y) / s).floor() as i32;
    let x1 = ((vis.max.x - rect.min.x) / s).ceil() as i32;
    let y1 = ((vis.max.y - rect.min.y) / s).ceil() as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                let min = rect.min + Vec2::new(x as f32 * s, y as f32 * s);
                let r = egui::Rect::from_min_size(min, Vec2::splat(s)).intersect(rect);
                painter.rect_filled(r, 0.0, Color32::from_gray(65));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PDF Viewer with Page Navigation & Controls

enum PdfMsg {
    Page(usize, ColorImage),
    Error(String),
}

struct PdfPage {
    size: Vec2,
    tex: Option<TextureHandle>,
}

pub struct PdfView {
    pages: Vec<PdfPage>,
    rx: Receiver<PdfMsg>,
    rendered: usize,
    error: Option<String>,
    pub current_page: usize,
    page_jump_text: String,
    zoom: Zoom,
}

impl PdfView {
    pub fn open(bytes: Vec<u8>, ctx: &egui::Context) -> Result<Self, String> {
        use hayro::hayro_syntax::Pdf;

        let pdf = Pdf::new(bytes.clone()).map_err(|e| format!("cannot parse PDF: {e:?}"))?;
        let pages: Vec<PdfPage> = pdf
            .pages()
            .iter()
            .map(|p| {
                let (w, h) = p.render_dimensions();
                PdfPage { size: Vec2::new(w, h), tex: None }
            })
            .collect();
        drop(pdf);
        if pages.is_empty() {
            return Err("PDF has no pages".into());
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let pdf = match Pdf::new(bytes) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(PdfMsg::Error(format!("{e:?}")));
                    return;
                }
            };
            let cache = hayro::RenderCache::new();
            let interp = hayro::hayro_interpret::InterpreterSettings::default();
            let settings = hayro::RenderSettings::default();
            let pix = hayro::PixmapSettings {
                x_scale: PDF_RENDER_SCALE,
                y_scale: PDF_RENDER_SCALE,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            };
            for (i, page) in pdf.pages().iter().enumerate() {
                let pm = hayro::render(page, &cache, &interp, &settings, &pix);
                let size = [pm.width() as usize, pm.height() as usize];
                let img = ColorImage::from_rgba_premultiplied(size, pm.data_as_u8_slice());
                if tx.send(PdfMsg::Page(i, img)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });

        Ok(Self {
            pages,
            rx,
            rendered: 0,
            error: None,
            current_page: 0,
            page_jump_text: "1".into(),
            zoom: Zoom::new(),
        })
    }

    pub fn info(&self) -> String {
        let n = self.pages.len();
        if self.rendered < n {
            format!("PDF · {n} pages · rendering {}/{n}", self.rendered)
        } else {
            format!("PDF · {n} pages")
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        loop {
            match self.rx.try_recv() {
                Ok(PdfMsg::Page(i, img)) => {
                    self.pages[i].tex = Some(ui.ctx().load_texture(format!("pdf_{i}"), img, TextureOptions::LINEAR));
                    self.rendered += 1;
                }
                Ok(PdfMsg::Error(e)) => self.error = Some(e),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(224, 108, 117), format!("PDF error: {e}"));
            return;
        }

        let max_w = self.pages.iter().map(|p| p.size.x).fold(1.0, f32::max);
        let fit = ((ui.available_width() - 40.0) / max_w).max(0.05);
        let effective = self.zoom.factor.unwrap_or(fit);

        // Page navigation bar
        let total_pages = self.pages.len();
        let mut scroll_to_page = None;

        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("First page").clicked() && self.current_page > 0 {
                self.current_page = 0;
                self.page_jump_text = "1".into();
                scroll_to_page = Some(0);
            }
            if ui.button("◀").on_hover_text("Previous page").clicked() && self.current_page > 0 {
                self.current_page -= 1;
                self.page_jump_text = (self.current_page + 1).to_string();
                scroll_to_page = Some(self.current_page);
            }

            ui.label("Page");
            let resp = ui.add(egui::TextEdit::singleline(&mut self.page_jump_text).desired_width(36.0));
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Ok(p) = self.page_jump_text.trim().parse::<usize>() {
                    if p >= 1 && p <= total_pages {
                        self.current_page = p - 1;
                        scroll_to_page = Some(self.current_page);
                    }
                }
            }
            ui.label(format!("of {total_pages}"));

            if ui.button("▶").on_hover_text("Next page").clicked() && self.current_page + 1 < total_pages {
                self.current_page += 1;
                self.page_jump_text = (self.current_page + 1).to_string();
                scroll_to_page = Some(self.current_page);
            }
            if ui.button("⏭").on_hover_text("Last page").clicked() && self.current_page + 1 < total_pages {
                self.current_page = total_pages - 1;
                self.page_jump_text = total_pages.to_string();
                scroll_to_page = Some(self.current_page);
            }

            ui.separator();
            self.zoom.toolbar(ui, effective, &self.info());
        });
        ui.separator();

        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                for (idx, page) in self.pages.iter().enumerate() {
                    let size = page.size * effective;
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

                    if let Some(target) = scroll_to_page {
                        if target == idx {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                    }

                    if ui.is_rect_visible(rect) {
                        self.current_page = idx;
                        match &page.tex {
                            Some(tex) => {
                                egui::Image::new(tex).paint_at(ui, rect);
                            }
                            None => {
                                ui.painter().rect_filled(rect, 0.0, Color32::from_gray(240));
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    format!("Rendering Page {}…", idx + 1),
                                    egui::FontId::proportional(14.0),
                                    Color32::GRAY,
                                );
                            }
                        }
                    }
                    ui.add_space(PAGE_GAP);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_svg() {
        let svg = r#"<svg width="100" height="100" xmlns="http://www.w3.org/2000/svg"><circle cx="50" cy="50" r="40" fill="red"/></svg>"#;
        let v = ImageView::open_svg(svg.as_bytes()).unwrap();
        assert_eq!(v.size, [100, 100]);
        assert!(v.is_svg);
    }
}
