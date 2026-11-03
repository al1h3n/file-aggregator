//! The window.
//!
//! Hierarchy, top to bottom of importance:
//! 1. **The files** – names in full contrast on one calm glass panel; folder, size and type
//!    are dimmer, and the remove button only shows on hover.
//! 2. **Copy** – the one tinted (blue) glass button. ⌘/Ctrl + Enter.
//! 3. Add and Save share a quieter glass capsule; Background and Clear sit at the far left.
//!
//! Glass that isn't always there appears only to say something:
//! - a toast grows out of the toolbar ("Copied 12 KB") and folds back into it,
//! - the background picker grows out of its button,
//! - the panel swells and turns blue while files are dragged over the window,
//! - scripts and executables are confirmed on one glass sheet per drop.
//!
//! Layout is computed by hand ([`Layout`]) so glass shapes and widgets always agree.

mod background;
mod format;
mod glass;
mod theme;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Id, Key, KeyboardShortcut, Modifiers, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
    ICON_ADD, ICON_CHECK, ICON_CHECK_CIRCLE, ICON_CLOSE, ICON_CONTENT_COPY, ICON_DELETE, ICON_DOWNLOAD, ICON_ERROR, ICON_INFO, ICON_PALETTE, ICON_UPLOAD_FILE,
    ICON_WARNING,
};
use liquid_rust::{Appearance, ContainerId, Glass, GlassId, Material, Shape, Spring};
use serde::{Deserialize, Serialize};

use crate::aggregator::aggregate_files_with_prompt;
use crate::security::{Inspection, inspect_file, strip_windows_prefix, validate_path_with_size, write_private};
use background::{Background, Photo};
use glass::{GlassLayer, Shared, rect};
use theme::{BODY, CAPTION, Palette, SMALL};

// ---------------------------------------------------------------------------------------
// Layout constants (points)
// ---------------------------------------------------------------------------------------

/// Window edge to glass.
const MARGIN: f32 = 16.0;
/// Between toolbar glass. Equal to the toolbar container's merge spacing, so the toast
/// rests cleanly apart from the bar it grew out of (see liquid-rust's `Container::spacing`).
const GAP: f32 = 12.0;
const BAR_H: f32 = 40.0;
const ADD_W: f32 = 82.0;
const SAVE_W: f32 = 86.0;
const COPY_W: f32 = 100.0;
const PANEL_RADIUS: f32 = 24.0;
/// Panel inner padding.
const PAD: f32 = 16.0;
const HEADER_H: f32 = 30.0;
const ROW_H: f32 = 32.0;
const PICKER_SIZE: egui::Vec2 = egui::vec2(316.0, 232.0);
const SHEET_W: f32 = 420.0;

/// Clipboard managers struggle with more than this.
const MAX_CLIPBOARD: usize = 10 * 1024 * 1024;

const ADD_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const COPY_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

const SETTINGS_KEY: &str = "settings";

/// Opens the window (with `files` already in the list) and runs until it's closed.
pub fn run_gui(files: Vec<PathBuf>) -> Result<()> {
    let icon = image::load_from_memory(include_bytes!("../../icon.png")).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(1, 1));
    let (width, height) = icon.dimensions();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("File Aggregator")
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 440.0])
            .with_drag_and_drop(true)
            .with_icon(egui::IconData { rgba: icon.into_raw(), width, height }),
        ..Default::default()
    };
    eframe::run_native(
        "File Aggregator",
        options,
        Box::new(|cc| {
            let mut app = App::new(cc)?;
            app.add_paths(files);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

/// Light/dark choice. `Auto` follows the background's lightness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Scheme {
    #[default]
    Auto,
    Light,
    Dark,
}

/// Saved between launches.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    background: Background,
    scheme: Scheme,
    /// Lightness of the saved photo, so the right scheme shows before it has loaded.
    photo_lightness: Option<f32>,
}

/// A file in the list, with everything the row shows computed once when it's added.
#[derive(Clone)]
struct FileItem {
    path: PathBuf,
    name: String,
    folder: String,
    /// Full path for the tooltip.
    full: String,
    badge: String,
    size: u64,
    binary: bool,
}

impl FileItem {
    fn new(path: PathBuf, size: u64, binary: bool) -> Self {
        let full = strip_windows_prefix(&path.display().to_string());
        let name = path.file_name().map_or_else(|| full.clone(), |n| n.to_string_lossy().into_owned());
        let folder = path.parent().map(|p| home_relative(&strip_windows_prefix(&p.display().to_string()))).unwrap_or_default();
        let badge = format::badge(path.extension().and_then(|e| e.to_str()));
        Self { path, name, folder, full, badge, size, binary }
    }
}

/// `/home/me/src` -> `~/src` on Unix-like systems, where that's the convention.
fn home_relative(folder: &str) -> String {
    if cfg!(unix)
        && let Some(home) = std::env::home_dir()
        && let Some(rest) = folder.strip_prefix(&*home.to_string_lossy())
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    folder.to_string()
}

/// A script or executable waiting for the user's OK.
struct Pending {
    path: PathBuf,
    size: u64,
    binary: bool,
    kind: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Success,
    Error,
    Info,
}

struct Toast {
    /// New id for every toast, so each one fades in.
    id: u64,
    icon: MaterialIcon,
    tone: Tone,
    text: String,
    /// Offers to put back the files that Clear removed.
    undo: bool,
    until: Instant,
}

enum Done {
    Copied(String),
    Saved(PathBuf, usize),
}

/// Aggregation running on a worker thread, so a 200 MB job never freezes the window.
struct Job {
    copy: bool,
    rx: mpsc::Receiver<Result<Done, String>>,
}

/// Photo being decoded on a worker thread.
struct PhotoJob {
    path: PathBuf,
    rx: mpsc::Receiver<Result<Photo, String>>,
    /// Restoring the saved background at startup (fall back quietly if it's gone).
    restoring: bool,
}

/// Handles of the glass in the scene.
struct Ids {
    toolbar: ContainerId,
    panel: GlassId,
    background_button: GlassId,
    bar: GlassId,
    copy: GlassId,
    clear: Option<GlassId>,
    toast: Option<GlassId>,
    picker: Option<GlassId>,
    sheet: Option<GlassId>,
}

pub struct App {
    glass: GlassLayer,
    ids: Option<Ids>,
    settings: Settings,
    /// Background on screen now (a new photo shows once it has loaded).
    shown: Background,
    shown_lightness: f32,
    photo_upload: Option<image::RgbaImage>,
    photo_job: Option<PhotoJob>,
    palette: Palette,
    files: Vec<FileItem>,
    known: HashSet<PathBuf>,
    prompt: String,
    pending: Vec<Pending>,
    /// What the last Clear removed.
    cleared: Vec<FileItem>,
    toast: Option<Toast>,
    toast_count: u64,
    job: Option<Job>,
    picker_open: bool,
    copied_at: Option<Instant>,
    last_frame: Option<Instant>,
    last_screen: Rect,
    hovering_files: usize,
}

// ---------------------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------------------

/// Where everything goes this frame. Glass and widgets both read from here.
struct Layout {
    panel: Rect,
    header: Rect,
    list: Rect,
    prompt: Rect,
    background_button: Rect,
    clear: Rect,
    bar: Rect,
    add: Rect,
    save: Rect,
    copy: Rect,
    toast: Rect,
    picker: Rect,
    sheet: Rect,
}

impl Layout {
    fn new(screen: Rect, app: &App, ctx: &egui::Context) -> Self {
        let bottom = screen.bottom() - MARGIN;
        let row = |left: f32, width: f32| Rect::from_min_size(pos2(left, bottom - BAR_H), vec2(width, BAR_H));

        // Toolbar: [background] [clear]   (toast lane)   [add | save] [copy]
        let background_button = row(screen.left() + MARGIN, BAR_H);
        let clear = row(background_button.right() + GAP, BAR_H);
        let copy = row(screen.right() - MARGIN - COPY_W, COPY_W);
        let bar = row(copy.left() - GAP - ADD_W - SAVE_W, ADD_W + SAVE_W);
        let add = Rect::from_min_size(bar.min, vec2(ADD_W, BAR_H));
        let save = Rect::from_min_max(pos2(add.right(), bar.top()), bar.max);

        // The toast takes the free lane left of the bar, as wide as its text needs.
        let lane_left = if app.files.is_empty() { background_button.right() } else { clear.right() } + GAP;
        let lane_right = bar.left() - GAP;
        let width = app.toast.as_ref().map_or(160.0, |t| toast_width(ctx, t)).min(lane_right - lane_left).max(BAR_H);
        let toast = row(lane_right - width, width);

        let panel = Rect::from_min_max(pos2(screen.left() + MARGIN, screen.top() + MARGIN), pos2(screen.right() - MARGIN, bar.top() - GAP));
        let inner = panel.shrink2(vec2(PAD, 10.0));
        // The instructions box grows with its text, from 2 to 6 lines.
        let lines = app.prompt.lines().count().max(1) + usize::from(app.prompt.ends_with('\n'));
        let prompt_h = (lines.clamp(2, 6) as f32 * 19.0 + 18.0).min(inner.height() * 0.4);
        let prompt = Rect::from_min_max(pos2(inner.left(), inner.bottom() - prompt_h), inner.max);
        let header_h = if app.files.is_empty() { 0.0 } else { HEADER_H };
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), header_h));
        let list = Rect::from_min_max(pos2(inner.left(), header.bottom()), pos2(inner.right(), prompt.top() - 1.0));

        let picker = Rect::from_min_size(pos2(background_button.left(), background_button.top() - GAP - PICKER_SIZE.y), PICKER_SIZE);
        let names = app.pending.len().min(3) + usize::from(app.pending.len() > 3);
        let sheet_size = vec2(SHEET_W.min(panel.width() - 2.0 * PAD), 168.0 + names as f32 * 18.0);
        let sheet = Rect::from_center_size(panel.center(), sheet_size);
        Self { panel, header, list, prompt, background_button, clear, bar, add, save, copy, toast, picker, sheet }
    }
}

fn toast_width(ctx: &egui::Context, toast: &Toast) -> f32 {
    let text = ctx.fonts_mut(|f| f.layout_no_wrap(toast.text.clone(), FontId::proportional(SMALL), Color32::WHITE).size().x);
    let undo = if toast.undo { 56.0 } else { 0.0 };
    16.0 + 20.0 + 8.0 + text + undo + 18.0
}

/// The content panel: regular glass, a little calmer at the edge than buttons, so it
/// reads as a surface rather than an effect.
fn panel_material() -> Material {
    Material { refraction: 0.5, dispersion: 0.08, specular: 0.22, ..Material::regular() }
}

/// Picker and warning sheet: frostier and more opaque than the panel, with a deeper
/// shadow, so they read as floating above it (like iOS menus).
fn menu_material() -> Material {
    let base = Material::regular();
    Material {
        frost: 18.0,
        shadow: liquid_rust::Shadow { offset_y: 14.0, blur: 36.0, opacity: 0.22 },
        light_tone: liquid_rust::Tone { lighten: 0.55, ..base.light_tone },
        dark_tone: liquid_rust::Tone { darken: 0.55, ..base.dark_tone },
        ..base
    }
}

fn copy_material(enabled: bool) -> Material {
    if enabled { Material::tinted(liquid_rust::Color::BLUE) } else { Material::regular() }
}

// ---------------------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------------------

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let glass = GlassLayer::new(cc).ok_or("File Aggregator needs the wgpu renderer")?;
        egui_material_icons::initialize(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();

        let mut app = Self {
            glass,
            ids: None,
            shown: Background::default(),
            shown_lightness: Background::default().lightness().unwrap_or(0.65),
            photo_upload: None,
            photo_job: None,
            palette: Palette::LIGHT,
            files: Vec::new(),
            known: HashSet::new(),
            prompt: String::new(),
            pending: Vec::new(),
            cleared: Vec::new(),
            toast: None,
            toast_count: 0,
            job: None,
            picker_open: false,
            copied_at: None,
            last_frame: None,
            last_screen: Rect::NOTHING,
            hovering_files: 0,
            settings,
        };
        match app.settings.background.clone() {
            Background::Photo(path) => {
                app.shown_lightness = app.settings.photo_lightness.unwrap_or(app.shown_lightness);
                app.load_photo(&cc.egui_ctx, path, true);
            }
            other => app.show_background(other),
        }
        app.palette = Palette::new(app.is_dark());
        theme::apply(&cc.egui_ctx, &app.palette);
        Ok(app)
    }

    fn is_dark(&self) -> bool {
        match self.settings.scheme {
            Scheme::Auto => self.shown_lightness < background::LIGHT_THRESHOLD,
            Scheme::Light => false,
            Scheme::Dark => true,
        }
    }

    fn show_background(&mut self, background: Background) {
        if let Some(lightness) = background.lightness() {
            self.shown_lightness = lightness;
        }
        self.settings.background = background.clone();
        self.shown = background;
    }

    fn load_photo(&mut self, ctx: &egui::Context, path: PathBuf, restoring: bool) {
        let (tx, rx) = mpsc::channel();
        let (ctx, worker_path) = (ctx.clone(), path.clone());
        std::thread::spawn(move || {
            let _ = tx.send(background::load_photo(&worker_path).map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        self.photo_job = Some(PhotoJob { path, rx, restoring });
    }

    fn toast(&mut self, tone: Tone, text: impl Into<String>) {
        let (icon, seconds) = match tone {
            Tone::Success => (ICON_CHECK_CIRCLE, 2.6),
            Tone::Error => (ICON_ERROR, 6.0),
            Tone::Info => (ICON_INFO, 3.5),
        };
        self.toast_count += 1;
        self.toast = Some(Toast { id: self.toast_count, icon, tone, text: text.into(), undo: false, until: Instant::now() + Duration::from_secs_f32(seconds) });
    }

    // --- files -----------------------------------------------------------------------

    fn push_file(&mut self, item: FileItem) {
        if self.known.insert(item.path.clone()) {
            self.files.push(item);
        }
    }

    /// Adds dropped or picked files. Regular files go straight in; scripts and executables
    /// wait for one confirmation per batch; problems are summed up in one toast.
    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut problems: Vec<String> = Vec::new();
        for path in paths {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            if path.is_dir() {
                problems.push(format!("{name} is a folder — add the files inside it"));
                continue;
            }
            let (canonical, size) = match validate_path_with_size(&path) {
                Ok(ok) => ok,
                Err(e) => {
                    problems.push(format!("{name}: {}", short_reason(&e)));
                    continue;
                }
            };
            if self.known.contains(&canonical) || self.pending.iter().any(|p| p.path == canonical) {
                continue;
            }
            match inspect_file(&canonical) {
                Ok(Inspection { executable: Some(kind), binary }) => self.pending.push(Pending { path: canonical, size, binary, kind }),
                Ok(Inspection { executable: None, binary }) => self.push_file(FileItem::new(canonical, size, binary)),
                Err(e) => problems.push(format!("{name}: {}", short_reason(&e))),
            }
        }
        match problems.len() {
            0 => {}
            1 => self.toast(Tone::Error, problems.remove(0)),
            n => self.toast(Tone::Error, format!("Skipped {n} files. {}", problems[0])),
        }
    }

    fn pick_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().pick_files() {
            self.add_paths(paths);
        }
    }

    fn clear(&mut self) {
        if self.files.is_empty() {
            return;
        }
        self.known.clear();
        self.cleared = std::mem::take(&mut self.files);
        let n = self.cleared.len();
        self.toast(Tone::Info, format!("Cleared {n} file{}", if n == 1 { "" } else { "s" }));
        if let Some(toast) = &mut self.toast {
            toast.undo = true;
            toast.until = Instant::now() + Duration::from_secs(6);
        }
    }

    fn undo_clear(&mut self) {
        for item in std::mem::take(&mut self.cleared) {
            self.push_file(item);
        }
        self.toast = None;
    }

    // --- copy / save -----------------------------------------------------------------

    fn can_run(&self) -> bool {
        !self.files.is_empty() && self.job.is_none()
    }

    fn copy(&mut self, ctx: &egui::Context) {
        if self.can_run() {
            self.start_job(ctx, None);
        }
    }

    fn save(&mut self, ctx: &egui::Context) {
        if !self.can_run() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("Markdown", &["md"]).set_file_name("aggregated.md").save_file() {
            self.start_job(ctx, Some(path));
        }
    }

    fn start_job(&mut self, ctx: &egui::Context, save_to: Option<PathBuf>) {
        let paths: Vec<PathBuf> = self.files.iter().map(|f| f.path.clone()).collect();
        let prompt = (!self.prompt.trim().is_empty()).then(|| self.prompt.clone());
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let copy = save_to.is_none();
        std::thread::spawn(move || {
            let result = aggregate_files_with_prompt(&paths, prompt.as_deref()).map_err(|e| format!("{e:#}")).and_then(|text| match save_to {
                None => Ok(Done::Copied(text)),
                Some(path) => write_private(&path, &text).map(|()| Done::Saved(path, text.len())).map_err(|e| format!("{e:#}")),
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.job = Some(Job { copy, rx });
    }

    /// Picks up results from the worker threads.
    fn poll_jobs(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.job {
            let result = match job.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The worker stopped unexpectedly".into())),
            };
            if let Some(result) = result {
                self.job = None;
                match result {
                    Ok(Done::Copied(text)) if text.len() > MAX_CLIPBOARD => {
                        self.toast(Tone::Error, format!("{} is too big for the clipboard. Use Save instead.", format::human_size(text.len() as u64)));
                    }
                    Ok(Done::Copied(text)) => {
                        let summary = format!(
                            "Copied {} · ≈{} tokens",
                            format::human_size(text.len() as u64),
                            format::compact_count(format::estimate_tokens(text.len() as u64, false))
                        );
                        ctx.copy_text(text);
                        self.copied_at = Some(Instant::now());
                        self.toast(Tone::Success, summary);
                    }
                    Ok(Done::Saved(path, bytes)) => {
                        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                        self.toast(Tone::Success, format!("Saved {name} · {}", format::human_size(bytes as u64)));
                    }
                    Err(e) => self.toast(Tone::Error, e),
                }
            }
        }

        if let Some(job) = &self.photo_job {
            let result = match job.rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The photo loader stopped unexpectedly".into())),
            };
            if let Some(result) = result {
                let job = self.photo_job.take().expect("checked above");
                match result {
                    Ok(photo) => {
                        self.shown_lightness = photo.lightness;
                        self.settings.photo_lightness = Some(photo.lightness);
                        self.settings.background = Background::Photo(job.path.clone());
                        self.shown = Background::Photo(job.path);
                        self.photo_upload = Some(photo.image);
                    }
                    Err(e) => {
                        if job.restoring {
                            self.show_background(Background::default());
                        }
                        self.toast(Tone::Error, format!("Couldn't load the photo: {e}"));
                    }
                }
            }
        }
    }

    // --- input -----------------------------------------------------------------------

    fn handle_input(&mut self, ctx: &egui::Context) {
        let (dropped, hovering) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>(), i.raw.hovered_files.len()));
        self.hovering_files = hovering;
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }

        let modal = self.picker_open || !self.pending.is_empty();
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if !self.pending.is_empty() {
                self.skip_pending();
            } else if self.picker_open {
                self.picker_open = false;
            } else {
                self.toast = None;
            }
        }
        if modal {
            return;
        }
        // Consumed before the text box sees them, so ⌘/Ctrl + Enter doesn't add a newline.
        if ctx.input_mut(|i| i.consume_shortcut(&ADD_SHORTCUT)) {
            self.pick_files();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&COPY_SHORTCUT)) {
            self.copy(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE_SHORTCUT)) {
            self.save(ctx);
        }
    }

    fn accept_pending(&mut self) {
        for p in std::mem::take(&mut self.pending) {
            self.push_file(FileItem::new(p.path, p.size, p.binary));
        }
    }

    fn skip_pending(&mut self) {
        let n = self.pending.len();
        self.pending.clear();
        self.toast(Tone::Info, format!("Skipped {n} file{}", if n == 1 { "" } else { "s" }));
    }

    // --- glass -----------------------------------------------------------------------

    /// Moves the glass to this frame's layout and adds or removes the glass that comes and goes.
    fn sync_glass(&mut self, g: &mut Shared, l: &Layout, resized: bool) {
        let animate = !resized;
        // Blue whenever there's something to copy, except under a picker or sheet (focus is there).
        let can_copy = !self.files.is_empty() && !self.picker_open && self.pending.is_empty();
        let ids = self.ids.get_or_insert_with(|| {
            let toolbar = g.add_container(GAP, 0);
            let picker = g.add_container(GAP, 2);
            Ids {
                toolbar,
                panel: g.add(Glass::new(rect(l.panel)).shape(Shape::Rounded(PANEL_RADIUS)).material(panel_material())),
                background_button: g.add(Glass::new(rect(l.background_button)).interactive().container(picker)),
                bar: g.add(Glass::new(rect(l.bar)).interactive().container(toolbar)),
                // Copy stays out of the toolbar container: fused liquid-rust glass shares one
                // material, so merging with the bar (e.g. while pressed) would drop its tint.
                copy: g.add(Glass::new(rect(l.copy)).material(copy_material(can_copy)).interactive()),
                clear: None,
                toast: None,
                picker: None,
                sheet: None,
            }
        });

        // Files dragged over the window: the panel swells a little and turns blue.
        let hovering = self.hovering_files > 0;
        g.frame_to(ids.panel, if hovering { l.panel.expand(4.0) } else { l.panel }, animate || hovering);
        let drop_tint = liquid_rust::Color::BLUE.with_alpha(0.22);
        g.set_material(ids.panel, if hovering { panel_material().tint(drop_tint) } else { panel_material() });

        g.frame_to(ids.background_button, l.background_button, animate);
        g.frame_to(ids.bar, l.bar, animate);
        g.frame_to(ids.copy, l.copy, animate);
        g.set_material(ids.copy, copy_material(can_copy));

        match (!self.files.is_empty(), ids.clear) {
            (true, None) => ids.clear = Some(g.add(Glass::new(rect(l.clear)).interactive())),
            (true, Some(id)) => g.frame_to(id, l.clear, animate),
            (false, Some(id)) => {
                g.remove(id);
                ids.clear = None;
            }
            (false, None) => {}
        }

        match (self.toast.is_some(), ids.toast) {
            (true, None) => ids.toast = Some(g.expand_from(ids.bar, Glass::new(rect(l.toast)).container(ids.toolbar), Spring::new(0.45, 0.18))),
            (true, Some(id)) => g.frame_to(id, l.toast, true),
            (false, Some(id)) => {
                g.collapse_into(id, ids.bar, Spring::new(0.32, 0.0));
                ids.toast = None;
            }
            (false, None) => {}
        }

        match (self.picker_open, ids.picker) {
            (true, None) => {
                ids.picker = Some(g.expand_from(
                    ids.background_button,
                    Glass::new(rect(l.picker)).shape(Shape::Rounded(26.0)).material(menu_material()),
                    Spring::new(0.5, 0.2),
                ))
            }
            (true, Some(id)) => g.frame_to(id, l.picker, animate),
            (false, Some(id)) => {
                g.collapse_into(id, ids.background_button, Spring::new(0.34, 0.0));
                ids.picker = None;
            }
            (false, None) => {}
        }

        match (!self.pending.is_empty(), ids.sheet) {
            (true, None) => ids.sheet = Some(g.add(Glass::new(rect(l.sheet)).shape(Shape::Rounded(28.0)).material(menu_material()).z(3))),
            (true, Some(id)) => g.frame_to(id, l.sheet, animate),
            (false, Some(id)) => {
                g.remove(id);
                ids.sheet = None;
            }
            (false, None) => {}
        }

        // Labels follow the colour scheme, so small glass must too (no flipping with the backdrop).
        g.set_appearance(Appearance { dark: self.palette.dark, adapt_to_content: false, ..Appearance::default() });
        g.set_background(self.shown.clone(), self.photo_upload.take());
    }

    /// Lets the glass light up under the pointer when a glass button is pressed.
    fn feed_pointer(ctx: &egui::Context, g: &mut Shared) {
        let (pos, pressed, down, released) =
            ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
        if let Some(pos) = pos {
            if pressed {
                g.pointer_down(pos);
            } else if down && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                g.pointer_move(pos);
            }
        }
        if released {
            g.pointer_up();
        }
    }

    // --- drawing ---------------------------------------------------------------------

    fn panel_ui(&mut self, ui: &mut egui::Ui, l: &Layout) {
        let p = self.palette;
        let painter = ui.painter().clone();

        if !self.files.is_empty() {
            let total: u64 = self.files.iter().map(|f| f.size).sum();
            let tokens: u64 = self.files.iter().map(|f| format::estimate_tokens(f.size, f.binary)).sum();
            let n = self.files.len();
            let summary = format!("{n} file{}  ·  {}  ·  ≈{} tokens", if n == 1 { "" } else { "s" }, format::human_size(total), format::compact_count(tokens));
            painter.text(l.header.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, summary, FontId::proportional(SMALL), p.secondary);
            hairline(&painter, l.header.bottom(), l.header.x_range(), p.separator);
        }

        if self.files.is_empty() {
            self.empty_state(ui, l.list);
        } else {
            self.file_rows(ui, l.list);
        }

        hairline(&painter, l.prompt.top(), l.prompt.x_range(), p.separator);
        let prompt_rect = l.prompt.shrink2(vec2(2.0, 0.0)).with_min_y(l.prompt.top() + 9.0);
        ui.scope_builder(UiBuilder::new().max_rect(prompt_rect), |ui| {
            egui::ScrollArea::vertical().id_salt("prompt").auto_shrink([false, true]).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.prompt)
                        .hint_text(RichText::new("Instructions for the AI (optional) — for example, “Explain the error handling”").color(p.tertiary))
                        .font(FontId::proportional(BODY))
                        .text_color(p.text)
                        .frame(egui::Frame::NONE)
                        .margin(vec2(2.0, 0.0))
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
            });
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, area: Rect) {
        let p = self.palette;
        let response = ui.interact(area, Id::unique("empty_state"), Sense::click());
        let dragging = self.hovering_files > 0;
        let painter = ui.painter();
        // Hover only tints the icon and hint: the empty panel stays calm.
        let hot = dragging || response.hovered();
        let c = area.center();
        let icon_color = if hot { p.accent } else { p.tertiary };
        painter.text(c - vec2(0.0, 30.0), Align2::CENTER_CENTER, ICON_UPLOAD_FILE.codepoint, FontId::new(40.0, ICON_UPLOAD_FILE.font_family()), icon_color);
        let (title, title_color) = match self.hovering_files {
            0 => ("Drop files here".to_string(), p.text),
            1 => ("Release to add 1 file".to_string(), p.accent),
            n => (format!("Release to add {n} files"), p.accent),
        };
        painter.text(c + vec2(0.0, 12.0), Align2::CENTER_CENTER, title, FontId::proportional(16.0), title_color);
        let hint = format!("or click to choose  ·  {}", ui.ctx().format_shortcut(&ADD_SHORTCUT));
        painter.text(c + vec2(0.0, 34.0), Align2::CENTER_CENTER, hint, FontId::proportional(SMALL), if response.hovered() { p.accent } else { p.secondary });
        if response.clicked() {
            self.pick_files();
        }
    }

    fn file_rows(&mut self, ui: &mut egui::Ui, area: Rect) {
        let p = self.palette;
        let mut remove = None;
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().id_salt("files").auto_shrink(false).show_rows(ui, ROW_H, self.files.len(), |ui, range| {
                // Only the visible rows are laid out, so thousands of files scroll smoothly.
                for index in range {
                    let item = &self.files[index];
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                    let painter = ui.painter();
                    // Rows touch, so test the pointer against the row itself (one row at a
                    // time); the remove button on top would otherwise "steal" the hover.
                    let hovered = ui.ctx().pointer_hover_pos().is_some_and(|pos| row.contains(pos) && pos.y < row.bottom() && ui.clip_rect().contains(pos))
                        && ui.is_enabled();
                    if hovered {
                        painter.rect_filled(row, CornerRadius::same(9), p.hover);
                    }

                    // Type badge.
                    let badge = Rect::from_center_size(pos2(row.left() + 24.0, row.center().y), vec2(38.0, 18.0));
                    painter.rect_filled(badge, CornerRadius::same(5), p.fill);
                    painter.text(badge.center(), Align2::CENTER_CENTER, &item.badge, FontId::monospace(9.5), p.secondary);

                    // Remove button (only visible on hover).
                    let close = Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), vec2(24.0, 24.0));
                    let close_response = ui.interact(close, Id::unique(("remove", &item.path)), Sense::click());
                    if hovered || close_response.hovered() {
                        if close_response.hovered() {
                            painter.circle_filled(close.center(), 11.0, p.hover);
                        }
                        let color = if close_response.hovered() { p.text } else { p.secondary };
                        painter.text(close.center(), Align2::CENTER_CENTER, ICON_CLOSE.codepoint, FontId::new(16.0, ICON_CLOSE.font_family()), color);
                    }
                    if close_response.clicked() {
                        remove = Some(index);
                    }

                    // Size, right-aligned before the remove button.
                    let size = painter.layout_no_wrap(format::human_size(item.size), FontId::proportional(CAPTION), p.tertiary);
                    let size_pos = pos2(close.left() - 8.0 - size.size().x, row.center().y - size.size().y / 2.0);
                    let size_left = size_pos.x;
                    painter.galley(size_pos, size, p.tertiary);

                    // Name, then the folder in what's left (its end, which tells folders apart).
                    let left = badge.right() + 12.0;
                    let room = (size_left - 16.0 - left).max(0.0);
                    let name = truncated(painter, &item.name, FontId::proportional(BODY), p.text, (room - 48.0).max(room * 0.6));
                    let name_w = name.size().x;
                    painter.galley(pos2(left, row.center().y - name.size().y / 2.0), name, p.text);
                    let folder_room = room - name_w - 10.0;
                    if folder_room > 40.0 {
                        let chars = (folder_room / (SMALL * 0.52)) as usize;
                        let folder = truncated(painter, &format::elide_start(&item.folder, chars), FontId::proportional(SMALL), p.tertiary, folder_room);
                        painter.galley(pos2(left + name_w + 10.0, row.center().y - folder.size().y / 2.0 + 0.5), folder, p.tertiary);
                    }
                    response.on_hover_text(item.full.as_str());
                }
            });
        });
        if let Some(index) = remove {
            let item = self.files.remove(index);
            self.known.remove(&item.path);
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui, l: &Layout, g: &Shared) {
        let p = self.palette;
        let ctx = ui.ctx().clone();
        let can_run = self.can_run();

        if !self.files.is_empty()
            && glass_button(ui, l.clear, "Clear list", Glyph::Icon(ICON_DELETE), None, p.secondary, true, p).on_hover_text("Clear the list").clicked()
        {
            self.clear();
        }
        let add_tip = format!("Add files  ·  {}", ctx.format_shortcut(&ADD_SHORTCUT));
        if glass_button(ui, l.add, "Add files", Glyph::Icon(ICON_ADD), Some("Add"), p.text, true, p).on_hover_text(add_tip).clicked() {
            self.pick_files();
        }
        let save_tip = format!("Save as Markdown  ·  {}", ctx.format_shortcut(&SAVE_SHORTCUT));
        if glass_button(ui, l.save, "Save", Glyph::Icon(ICON_DOWNLOAD), Some("Save"), p.text, can_run, p).on_hover_text(save_tip).clicked() {
            self.save(&ctx);
        }

        // Copy: white on the tinted glass; a spinner while running, a check right after.
        let copying = self.job.as_ref().is_some_and(|j| j.copy);
        let just_copied = self.copied_at.is_some_and(|t| t.elapsed() < Duration::from_millis(1400));
        if let Some(t) = self.copied_at.filter(|_| just_copied) {
            ctx.request_repaint_after(Duration::from_millis(1400).saturating_sub(t.elapsed()));
        }
        // White on the blue glass whenever there is something to copy (tint: see `sync_glass`).
        let copy_color = if self.files.is_empty() { p.text } else { Color32::WHITE };
        let glyph = match (copying, just_copied) {
            (true, _) => Glyph::Spinner,
            (false, true) => Glyph::Icon(ICON_CHECK),
            (false, false) => Glyph::Icon(ICON_CONTENT_COPY),
        };
        let copy_tip = format!("Copy to clipboard  ·  {}", ctx.format_shortcut(&COPY_SHORTCUT));
        let copy = glass_button(ui, l.copy, "Copy", glyph, Some("Copy"), copy_color, can_run || copying, p);
        if copy.on_hover_text(copy_tip).clicked() {
            self.copy(&ctx);
        }

        self.toast_ui(ui, l, g);
    }

    fn toast_ui(&mut self, ui: &mut egui::Ui, l: &Layout, g: &Shared) {
        let p = self.palette;
        let Some(toast) = &self.toast else { return };
        // Draw inside the glass as it is right now, so text never spills while it grows.
        let glass = self.ids.as_ref().and_then(|ids| ids.toast).and_then(|id| g.current_frame(id)).unwrap_or(l.toast);
        let fade = ui.ctx().animate_bool_with_time(Id::unique(("toast", toast.id)), true, 0.25);
        let mut painter = ui.painter().with_clip_rect(glass.intersect(l.toast));
        painter.multiply_opacity(fade);
        // Registered first so the Undo button (added after) sits on top of it for clicks.
        let dismiss = ui.interact(l.toast, Id::unique("toast_area"), Sense::click()).clicked();
        let (icon_color, text_color) = match toast.tone {
            Tone::Success => (p.success, p.text),
            Tone::Error => (p.danger, p.text),
            Tone::Info => (p.secondary, p.text),
        };
        let mut x = l.toast.left() + 16.0;
        painter.text(pos2(x, l.toast.center().y), Align2::LEFT_CENTER, toast.icon.codepoint, FontId::new(18.0, toast.icon.font_family()), icon_color);
        x += 28.0;
        let undo_w = if toast.undo { 56.0 } else { 0.0 };
        let text = truncated(&painter, &toast.text, FontId::proportional(SMALL), text_color, l.toast.right() - 16.0 - undo_w - x);
        painter.galley(pos2(x, l.toast.center().y - text.size().y / 2.0), text, text_color);

        let mut undo_clicked = false;
        if toast.undo {
            let undo = Rect::from_min_max(pos2(l.toast.right() - undo_w - 8.0, l.toast.top() + 6.0), pos2(l.toast.right() - 8.0, l.toast.bottom() - 6.0));
            let response = ui.interact(undo, Id::unique(("undo", toast.id)), Sense::click());
            if response.hovered() {
                painter.rect_filled(undo, CornerRadius::same(14), p.hover);
            }
            painter.text(undo.center(), Align2::CENTER_CENTER, "Undo", FontId::proportional(SMALL), p.accent);
            undo_clicked = response.clicked();
        }
        if undo_clicked {
            self.undo_clear();
        } else if dismiss {
            self.toast = None;
        }
    }

    fn picker_ui(&mut self, ctx: &egui::Context, l: &Layout, g: &Shared, screen: Rect) {
        let p = self.palette;
        let glass = self.ids.as_ref().and_then(|ids| ids.picker).and_then(|id| g.current_frame(id)).unwrap_or(l.picker);
        let fade = ctx.animate_bool_with_time(Id::unique("picker_fade"), true, 0.2);
        let mut chosen: Option<Background> = None;
        let mut choose_photo = false;
        let current = self.settings.background.clone();

        egui::Area::new(Id::unique("picker")).order(egui::Order::Foreground).fixed_pos(l.picker.min).constrain(false).show(ctx, |ui| {
            ui.set_clip_rect(glass.intersect(screen));
            ui.multiply_opacity(fade);
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 14)).show(ui, |ui| {
                ui.set_width(PICKER_SIZE.x - 32.0);
                ui.set_height(PICKER_SIZE.y - 28.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Background").size(BODY).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (scheme, label) in [(Scheme::Dark, "Dark"), (Scheme::Light, "Light"), (Scheme::Auto, "Auto")] {
                            ui.selectable_value(&mut self.settings.scheme, scheme, RichText::new(label).size(CAPTION));
                        }
                    });
                });
                ui.add_space(6.0);

                section(ui, "Color", p);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for &(name, color) in background::SOLIDS {
                        let selected = current == Background::Solid(color);
                        if swatch(ui, color, color, selected, p).on_hover_text(name).clicked() {
                            chosen = Some(Background::Solid(color));
                        }
                    }
                    let mut custom = if let Background::Solid(c) = current { c } else { background::DEFAULT_GREY };
                    if color_well(ui, "solid", &mut custom, p) {
                        chosen = Some(Background::Solid(custom));
                    }
                });
                ui.add_space(4.0);

                section(ui, "Gradient", p);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for &(name, a, b) in background::GRADIENTS {
                        let selected = current == Background::Gradient(a, b);
                        if swatch(ui, a, b, selected, p).on_hover_text(name).clicked() {
                            chosen = Some(Background::Gradient(a, b));
                        }
                    }
                    // Fine-tune the two ends of the current gradient.
                    if let Background::Gradient(mut a, mut b) = current {
                        let from = color_well(ui, "gradient_from", &mut a, p);
                        let to = color_well(ui, "gradient_to", &mut b, p);
                        if from || to {
                            chosen = Some(Background::Gradient(a, b));
                        }
                    }
                });
                ui.add_space(4.0);

                section(ui, "Photo", p);
                ui.horizontal(|ui| {
                    let loading = self.photo_job.is_some();
                    if ui.add_enabled(!loading, egui::Button::new(RichText::new("Choose photo…").size(SMALL))).clicked() {
                        choose_photo = true;
                    }
                    if loading {
                        ui.add(egui::Spinner::new().size(14.0));
                    } else if let Background::Photo(path) = &current {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        ui.add(egui::Label::new(RichText::new(name).size(SMALL).color(p.secondary)).truncate());
                    }
                });
            });
        });

        if let Some(background) = chosen {
            self.show_background(background);
        }
        if choose_photo
            && let Some(path) = rfd::FileDialog::new().add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff"]).pick_file()
        {
            self.load_photo(ctx, path, false);
        }

        // Close on a press outside (unless egui's colour picker popup is open).
        let pressed_outside =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pos| !l.picker.contains(pos) && !l.background_button.contains(pos)));
        if pressed_outside && !egui::Popup::is_any_open(ctx) {
            self.picker_open = false;
        }
    }

    fn sheet_ui(&mut self, ctx: &egui::Context, l: &Layout, g: &Shared, screen: Rect) {
        let p = self.palette;
        let glass = self.ids.as_ref().and_then(|ids| ids.sheet).and_then(|id| g.current_frame(id)).unwrap_or(l.sheet);
        let fade = ctx.animate_bool_with_time(Id::unique("sheet_fade"), true, 0.2);
        let (mut accept, mut skip) = (false, false);
        let n = self.pending.len();

        egui::Area::new(Id::unique("sheet")).order(egui::Order::Foreground).fixed_pos(l.sheet.min).constrain(false).show(ctx, |ui| {
            ui.set_clip_rect(glass.intersect(screen));
            ui.multiply_opacity(fade);
            egui::Frame::NONE.inner_margin(egui::Margin::same(20)).show(ui, |ui| {
                ui.set_width(l.sheet.width() - 40.0);
                ui.set_height(l.sheet.height() - 40.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ICON_WARNING.codepoint).family(ICON_WARNING.font_family()).size(22.0).color(p.warning));
                    let title = if n == 1 { "Add a script or executable?".to_string() } else { format!("Add {n} scripts or executables?") };
                    ui.label(RichText::new(title).size(15.5).color(p.text));
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new("These files can run code when opened. Their contents will be included as text — read them before running anything.")
                        .size(SMALL)
                        .color(p.secondary),
                );
                ui.add_space(6.0);
                for item in self.pending.iter().take(3) {
                    let name = item.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(format!("{name}  —  {}", item.kind)).size(SMALL).color(p.tertiary)).truncate());
                }
                if n > 3 {
                    ui.label(RichText::new(format!("and {} more", n - 3)).size(SMALL).color(p.tertiary));
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let add = egui::Button::new(RichText::new("Add anyway").size(BODY).color(Color32::WHITE))
                                .fill(p.accent)
                                .corner_radius(CornerRadius::same(15))
                                .min_size(vec2(108.0, 30.0));
                            accept = ui.add(add).clicked();
                            let skip_button = egui::Button::new(RichText::new("Skip").size(BODY).color(p.text))
                                .fill(p.fill)
                                .corner_radius(CornerRadius::same(15))
                                .min_size(vec2(76.0, 30.0));
                            skip = ui.add(skip_button).clicked();
                        });
                    });
                });
            });
        });

        if accept || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            self.accept_pending();
        } else if skip {
            self.skip_pending();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = self.last_frame.map_or(0.0, |t| (now - t).as_secs_f32());
        self.last_frame = Some(now);

        self.poll_jobs(&ctx);
        self.handle_input(&ctx);
        if let Some(toast) = &self.toast {
            match toast.until.checked_duration_since(now) {
                Some(left) => ctx.request_repaint_after(left),
                None => self.toast = None,
            }
        }
        let palette = Palette::new(self.is_dark());
        if palette != self.palette {
            self.palette = palette;
            theme::apply(&ctx, &palette);
        }

        let screen = ctx.content_rect();
        let resized = screen != self.last_screen;
        self.last_screen = screen;
        let glass = self.glass.clone();
        // Queue the glass first: it shares egui's background layer with the root `ui`, and
        // shapes in one layer are painted in order. (It's drawn after `ui` returns, with the
        // state this frame leaves behind.)
        glass.paint(&ctx);
        let mut g = glass.lock();

        // Widgets first (they react to clicks), then the glass follows the resulting state.
        let layout = Layout::new(screen, self, &ctx);
        let modal = self.picker_open || !self.pending.is_empty();
        let dim = ctx.animate_bool_with_time(Id::unique("dim"), modal, 0.2);
        ui.scope_builder(UiBuilder::new().max_rect(screen), |ui| {
            if modal {
                ui.disable();
            }
            // Labels are drawn above all glass, so whatever sits under the picker or sheet
            // must fade far enough not to show through it.
            ui.multiply_opacity(1.0 - 0.88 * dim);
            self.panel_ui(ui, &layout);
            self.toolbar_ui(ui, &layout, &g);
        });
        // The background button stays live (and undimmed) while its picker is open, so it
        // can close it again.
        let picker_color = if self.picker_open { self.palette.accent } else { self.palette.secondary };
        if glass_button(ui, layout.background_button, "Background", Glyph::Icon(ICON_PALETTE), None, picker_color, self.pending.is_empty(), self.palette)
            .on_hover_text("Background")
            .clicked()
        {
            self.picker_open = !self.picker_open;
        } else if self.picker_open {
            self.picker_ui(&ctx, &layout, &g, screen);
        }
        if !self.pending.is_empty() {
            self.sheet_ui(&ctx, &layout, &g, screen);
        }

        let layout = Layout::new(screen, self, &ctx);
        self.sync_glass(&mut g, &layout, resized);
        if !modal {
            Self::feed_pointer(&ctx, &mut g);
        }
        if g.update(dt) {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }
}

// ---------------------------------------------------------------------------------------
// Small drawing helpers
// ---------------------------------------------------------------------------------------

fn hairline(painter: &egui::Painter, y: f32, x: egui::Rangef, color: Color32) {
    painter.hline(x, y, Stroke::new(1.0, color));
}

/// Single-line text cut with "…" to fit `max_width`.
fn truncated(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    painter.layout_job(job)
}

fn section(ui: &mut egui::Ui, title: &str, p: Palette) {
    ui.label(RichText::new(title).size(CAPTION).color(p.tertiary));
}

/// What a glass button shows before its label.
enum Glyph {
    Icon(MaterialIcon),
    /// Work in progress.
    Spinner,
}

/// A button drawn over glass: hover highlight, then the glyph and optional label, centred.
/// `name` identifies it and is what screen readers announce.
#[allow(clippy::too_many_arguments)]
fn glass_button(ui: &mut egui::Ui, rect: Rect, name: &str, glyph: Glyph, label: Option<&str>, color: Color32, enabled: bool, p: Palette) -> egui::Response {
    let response = ui.interact(rect, Id::unique(("glass_button", name)), if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect.shrink(1.0), CornerRadius::same((rect.height() / 2.0) as u8), p.hover);
    }
    let color = if enabled { color } else { color.gamma_multiply(0.35) };
    // Glyph width is fixed so the label doesn't shift when the icon swaps for a spinner.
    const GLYPH: f32 = 20.0;
    let label = label.map(|l| painter.layout_no_wrap(l.to_string(), FontId::proportional(BODY), color));
    let width = GLYPH + label.as_ref().map_or(0.0, |g| 6.0 + g.size().x);
    let glyph_rect = Rect::from_center_size(pos2(rect.center().x - width / 2.0 + GLYPH / 2.0, rect.center().y), vec2(GLYPH, GLYPH));
    match glyph {
        Glyph::Icon(icon) => {
            painter.text(glyph_rect.center(), Align2::CENTER_CENTER, icon.codepoint, FontId::new(19.0, icon.font_family()), color);
        }
        Glyph::Spinner => egui::Spinner::new().size(15.0).color(color).paint_at(ui, glyph_rect.shrink(2.5)),
    }
    if let Some(label) = label {
        painter.galley(pos2(glyph_rect.right() + 6.0, rect.center().y - label.size().y / 2.0), label, color);
    }
    response
}

/// Round colour swatch (`a == b` for a solid colour) with a ring when selected.
fn swatch(ui: &mut egui::Ui, a: background::Rgb, b: background::Rgb, selected: bool, p: Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    let radius = 10.0;
    if a == b {
        painter.circle_filled(center, radius, Color32::from_rgb(a[0], a[1], a[2]));
    } else {
        painter.add(gradient_disc(center, radius, a, b));
    }
    painter.circle_stroke(center, radius, Stroke::new(1.0, p.separator));
    if selected {
        painter.circle_stroke(center, radius + 2.5, Stroke::new(2.0, p.accent));
    } else if response.hovered() {
        painter.circle_stroke(center, radius + 2.5, Stroke::new(1.5, p.separator));
    }
    response
}

/// "Any colour" swatch: a hue wheel around the current colour; click for egui's picker.
/// Returns `true` when the colour changed.
fn color_well(ui: &mut egui::Ui, id: &str, color: &mut background::Rgb, p: Palette) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let response = response.on_hover_text("Any color");
    let painter = ui.painter();
    let center = rect.center();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, Color32::WHITE);
    const SEGMENTS: u32 = 36;
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let angle = t * std::f32::consts::TAU;
        mesh.colored_vertex(center + 10.0 * vec2(angle.cos(), angle.sin()), egui::ecolor::Hsva::new(t, 0.75, 1.0, 1.0).into());
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.circle(center, 5.0, Color32::from_rgb(color[0], color[1], color[2]), Stroke::new(1.5, Color32::WHITE));
    if response.hovered() {
        painter.circle_stroke(center, 12.5, Stroke::new(1.5, p.separator));
    }
    let mut changed = false;
    egui::Popup::from_toggle_button_response(&response).id(Id::unique(("color_well", id))).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
        |ui| {
            let mut c = Color32::from_rgb(color[0], color[1], color[2]);
            if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
                *color = [c.r(), c.g(), c.b()];
                changed = true;
            }
        },
    );
    changed
}

/// A disc filled with a top-left → bottom-right gradient, mixed in OKLab like the background.
fn gradient_disc(center: egui::Pos2, radius: f32, a: background::Rgb, b: background::Rgb) -> egui::Shape {
    let (la, lb) = (background::oklab(a), background::oklab(b));
    let color_at = |pos: egui::Pos2| {
        let d = (pos - center) / radius; // -1..1
        let t = ((d.x + d.y) * 0.25 + 0.5).clamp(0.0, 1.0);
        oklab_to_color([la[0] + (lb[0] - la[0]) * t, la[1] + (lb[1] - la[1]) * t, la[2] + (lb[2] - la[2]) * t])
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color_at(center));
    const SEGMENTS: u32 = 32;
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let pos = center + radius * vec2(angle.cos(), angle.sin());
        mesh.colored_vertex(pos, color_at(pos));
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    egui::Shape::mesh(mesh)
}

fn oklab_to_color(c: [f32; 3]) -> Color32 {
    let l_ = c[0] + 0.396_337_78 * c[1] + 0.215_803_76 * c[2];
    let m_ = c[0] - 0.105_561_346 * c[1] - 0.063_854_17 * c[2];
    let s_ = c[0] - 0.089_484_18 * c[1] - 1.291_485_5 * c[2];
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let rgb = [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ];
    let [r, g, b] = rgb.map(|v| {
        let v = v.clamp(0.0, 1.0);
        let e = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0 + 0.5) as u8
    });
    Color32::from_rgb(r, g, b)
}

/// The useful part of an error for a toast: the limit that was hit, or the OS reason.
fn short_reason(error: &anyhow::Error) -> String {
    let message = error.to_string();
    if message.contains("too large") {
        "larger than the 50 MB limit".to_string()
    } else if message.contains("Not a regular file") {
        "not a regular file".to_string()
    } else if message.contains("traversal") || message.contains("not allowed") || message.contains("device name") || message.contains("data stream") {
        "this path isn't allowed".to_string()
    } else {
        error.root_cause().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_round_trip() {
        for c in [[0u8, 0, 0], [255, 255, 255], [0x8E, 0x8E, 0x93], [0x00, 0x88, 0xFF], [0xE3, 0x8B, 0x6B]] {
            let back = oklab_to_color(background::oklab(c));
            assert_eq!([back.r(), back.g(), back.b()], c);
        }
    }

    #[test]
    fn home_folder_is_shortened() {
        if let Some(home) = std::env::home_dir().filter(|_| cfg!(unix)) {
            let home = home.to_string_lossy();
            assert_eq!(home_relative(&format!("{home}/src")), "~/src");
            assert_eq!(home_relative(&format!("{home}x/src")), format!("{home}x/src"));
        }
        assert_eq!(home_relative("/opt/data"), "/opt/data");
    }

    #[test]
    fn settings_survive_a_round_trip_and_old_data() {
        let settings = Settings { background: Background::Gradient([1, 2, 3], [4, 5, 6]), scheme: Scheme::Dark, photo_lightness: Some(0.3) };
        let text = ron::to_string(&settings).unwrap();
        let back: Settings = ron::from_str(&text).unwrap();
        assert_eq!(back.background, settings.background);
        assert_eq!(back.scheme, Scheme::Dark);
        // Missing fields fall back to defaults (forward compatible).
        let partial: Settings = ron::from_str("(scheme: Light)").unwrap();
        assert_eq!(partial.background, Background::default());
    }
}
