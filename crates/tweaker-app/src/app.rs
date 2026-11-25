//! Main window: left menu with the 14 sections, a content area and a status bar.
//!
//! The skeleton deliberately has no business logic: it proves that the window,
//! localization, logging and the elevation check work. Tweak items arrive in the
//! next stages, always as catalog data, never as `if` branches in this file.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use tweaker_core::logging;
use tweaker_core::paths::ProductPaths;
use tweaker_core::product::{PRODUCT_VERSION, version_string};
use tweaker_core::sections::{Section, SectionKind};
use tweaker_windows::elevation::{self, ElevationResult};

use crate::i18n::{self, Localizer};

/// How long `--smoke` keeps the window open before closing itself.
const SMOKE_DURATION: Duration = Duration::from_millis(2500);

/// How long the screenshot mode waits for the renderer before giving up.
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(20);

/// What the program should do after startup checks.
pub enum RunMode {
    /// Normal window, no elevation shortcut.
    Gui,
    /// Open the window, render a few frames, close automatically.
    Smoke,
    /// Render the window, save a PNG screenshot, then close.
    Screenshot(std::path::PathBuf),
}

impl RunMode {
    fn name(&self) -> &'static str {
        match self {
            RunMode::Gui => "gui",
            RunMode::Smoke => "smoke",
            RunMode::Screenshot(_) => "screenshot",
        }
    }

    fn automated(&self) -> bool {
        !matches!(self, RunMode::Gui)
    }
}

/// Result of the `--screenshot` mode, shared by the UI thread, the screenshot
/// callback and the watchdog thread. The process exit code reflects this state.
#[derive(Default)]
struct ScreenshotState {
    /// The callback ran (successfully or not).
    done: AtomicBool,
    /// Saving the PNG failed.
    failed: AtomicBool,
    /// The renderer did not answer in time.
    timed_out: AtomicBool,
    /// Human-readable error of the failed save.
    error: Mutex<String>,
}

/// Starts the application.
pub fn run(mode: RunMode) -> Result<u8, String> {
    let paths = ProductPaths::for_current_user()
        .map_err(|error| format!("cannot resolve product paths: {error}"))?;
    paths.ensure_runtime_dirs().map_err(|error| {
        format!(
            "cannot create product folders in {}: {error}",
            paths.root().display()
        )
    })?;
    let logger =
        logging::init(logging::LoggerConfig::new(paths.logs()).with_echo(cfg!(debug_assertions)))
            .map_err(|error| format!("cannot start logger: {error}"))?;
    logger.info(&format!(
        "start: {} (mode={})",
        version_string(),
        mode.name()
    ));

    let loc = Localizer::load(i18n::detect_lang());
    logger.info(&format!("language: {}", loc.lang().code()));

    if mode.automated() {
        if !elevation::is_elevated() {
            logger.warn(
                "automated mode runs without administrator rights (expected in a debug build)",
            );
        }
    } else {
        let result = elevation::ensure_elevated(
            &loc.t("elevation.title"),
            &loc.t("elevation.body"),
            &loc.t("elevation.failed"),
        );
        match result {
            ElevationResult::AlreadyElevated => {}
            ElevationResult::Relaunched => {
                logger.info("elevation: elevated instance started, closing this one");
                return Ok(0);
            }
            ElevationResult::Declined => {
                logger.warn("elevation: declined by the user, nothing changed");
                return Ok(0);
            }
            ElevationResult::Failed(error) => {
                logger.error(&format!("elevation: restart failed: {error}"));
                return Ok(1);
            }
        }
    }

    let sections = tweaker_core::sections::load_sections()
        .map_err(|error| format!("cannot load section catalog: {error}"))?;
    logger.info(&format!("sections: {}", sections.len()));

    run_window(loc, paths, sections, mode)
}

fn run_window(
    loc: Localizer,
    paths: ProductPaths,
    sections: Vec<Section>,
    mode: RunMode,
) -> Result<u8, String> {
    let admin = elevation::is_elevated();
    let title = loc.t("app.window");
    let screenshot_mode = matches!(mode, RunMode::Screenshot(_));
    let screenshot_state = Arc::new(ScreenshotState::default());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([960.0, 620.0]),
        ..Default::default()
    };
    let app_state = Arc::clone(&screenshot_state);
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            install_style(&cc.egui_ctx);
            let app = TweakerApp::new(loc, paths, sections, admin, mode, app_state);
            app.start_watchdog(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| format!("cannot start the window: {error}"))?;
    logging::info("exit: window closed");

    if screenshot_mode {
        if screenshot_state.timed_out.load(Ordering::SeqCst) {
            return Err("screenshot: the renderer did not return an image in time".to_owned());
        }
        if screenshot_state.failed.load(Ordering::SeqCst) {
            let error = screenshot_state
                .error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            return Err(format!("screenshot was not saved: {error}"));
        }
        if !screenshot_state.done.load(Ordering::SeqCst) {
            return Err("screenshot was not saved".to_owned());
        }
    }
    Ok(0)
}

/// Applies the visual style and installs the Windows system fonts.
pub fn install_style(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
    });
    install_system_fonts(ctx);
}

fn install_system_fonts(ctx: &egui::Context) {
    let Some(windir) = std::env::var_os("WINDIR") else {
        logging::warn("ui: WINDIR is not set, using built-in fonts");
        return;
    };
    let fonts_dir = std::path::PathBuf::from(windir).join("Fonts");
    let mut fonts = egui::FontDefinitions::default();
    let mut installed = 0;
    let candidates = [
        ("segoeui.ttf", egui::FontFamily::Proportional, "Segoe UI"),
        ("consola.ttf", egui::FontFamily::Monospace, "Consolas"),
    ];
    for (file, family, name) in candidates {
        let path = fonts_dir.join(file);
        match std::fs::read(&path) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.to_owned());
                installed += 1;
            }
            Err(error) => logging::warn(&format!("ui: cannot read {}: {error}", path.display())),
        }
    }
    if installed > 0 {
        ctx.set_fonts(fonts);
        logging::info(&format!("ui: installed {installed} system font(s)"));
    } else {
        logging::warn("ui: system fonts are missing, using built-in fonts");
    }
}

struct TweakerApp {
    loc: Localizer,
    sections: Vec<Section>,
    selected: usize,
    admin: bool,
    mode: RunMode,
    started: Instant,
    frames: u64,
    screenshot_requested: bool,
    screenshot_state: Arc<ScreenshotState>,
    closing: bool,
    paths: ProductPaths,
}

impl TweakerApp {
    fn new(
        loc: Localizer,
        paths: ProductPaths,
        sections: Vec<Section>,
        admin: bool,
        mode: RunMode,
        screenshot_state: Arc<ScreenshotState>,
    ) -> Self {
        Self {
            loc,
            sections,
            selected: 0,
            admin,
            mode,
            started: Instant::now(),
            frames: 0,
            screenshot_requested: false,
            screenshot_state,
            closing: false,
            paths,
        }
    }

    /// In `--screenshot` mode, closes the window and records a timeout if the
    /// renderer never returns an image — even when the UI stops being called.
    fn start_watchdog(&self, ctx: &egui::Context) {
        if !matches!(self.mode, RunMode::Screenshot(_)) {
            return;
        }
        let state = Arc::clone(&self.screenshot_state);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < SCREENSHOT_TIMEOUT {
                if state.done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if state.done.load(Ordering::SeqCst) {
                return;
            }
            state.timed_out.store(true, Ordering::SeqCst);
            logging::error("screenshot: the renderer did not return an image in time");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
        });
    }

    fn draw_header(&mut self, ui: &mut egui::Ui) {
        let title = self.loc.t("app.title");
        let version = self
            .loc
            .fmt("app.version_label", &[("version", PRODUCT_VERSION)]);
        let badge = if self.admin {
            self.loc.t("app.badge.admin")
        } else {
            self.loc.t("app.badge.no_admin")
        };
        let badge_color = if self.admin {
            egui::Color32::from_rgb(21, 128, 61)
        } else {
            egui::Color32::from_rgb(180, 83, 9)
        };
        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(20.0).strong());
                ui.label(
                    egui::RichText::new(version)
                        .size(12.0)
                        .color(egui::Color32::from_gray(120)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(badge)
                            .size(13.0)
                            .strong()
                            .color(badge_color),
                    );
                });
            });
            ui.add_space(6.0);
        });
    }

    fn draw_status(&mut self, ui: &mut egui::Ui) {
        let log_text = self
            .loc
            .fmt("status.log", &[("path", &short_path(&self.paths.logs()))]);
        let journal_text = self.loc.fmt(
            "status.journal",
            &[("path", &short_path(&self.paths.journal()))],
        );
        let lang_text = self.loc.fmt(
            "status.language",
            &[("language", self.loc.lang().native_name())],
        );
        let version_text = self
            .loc
            .fmt("status.version", &[("version", PRODUCT_VERSION)]);
        let small = |text: String| {
            egui::RichText::new(text)
                .size(11.0)
                .color(egui::Color32::from_gray(125))
        };
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(small(log_text));
                ui.separator();
                ui.label(small(journal_text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(small(version_text));
                    ui.separator();
                    ui.label(small(lang_text));
                });
            });
            ui.add_space(2.0);
        });
    }

    fn draw_menu(&mut self, ui: &mut egui::Ui) {
        let loc = &self.loc;
        let labels: Vec<(String, String)> = self
            .sections
            .iter()
            .map(|section| (loc.t(&section.title_key), loc.t(&section.hint_key)))
            .collect();
        let menu_title = loc.t("menu.title");
        let mut selected = self.selected;
        egui::Panel::left("menu")
            .resizable(false)
            .exact_size(300.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(menu_title)
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_gray(115)),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, (title, hint)) in labels.iter().enumerate() {
                            let response = ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::Button::new(title.as_str()).selected(index == selected),
                            );
                            if response.on_hover_text(hint.as_str()).clicked() {
                                selected = index;
                            }
                        }
                    });
            });
        self.selected = selected;
    }

    fn draw_central(&mut self, ui: &mut egui::Ui) {
        let section = self.sections[self.selected].clone();
        let title = self.loc.t(&section.title_key);
        let hint = self.loc.t(&section.hint_key);
        let kind_text = match section.kind {
            SectionKind::Catalog => self
                .loc
                .fmt("kind.catalog", &[("count", &section.items.to_string())]),
            SectionKind::Tool => self.loc.t("kind.tool"),
        };
        let placeholder_title = self.loc.t("placeholder.title");
        let placeholder_body = self.loc.t("placeholder.body");
        let is_about = section.id == "about";
        let about_lines = if is_about {
            self.about_lines()
        } else {
            Vec::new()
        };
        let about_title = self.loc.t("about.title");

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(title).size(26.0).strong());
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(hint)
                            .size(14.0)
                            .color(egui::Color32::from_gray(110)),
                    );
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(kind_text).size(13.0));
                    ui.add_space(14.0);
                    egui::Frame::group(ui.style())
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            if is_about {
                                ui.label(egui::RichText::new(about_title).size(16.0).strong());
                                ui.add_space(4.0);
                                for line in &about_lines {
                                    ui.label(line);
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(placeholder_title).size(16.0).strong(),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(placeholder_body)
                                        .size(13.5)
                                        .color(egui::Color32::from_gray(95)),
                                );
                            }
                        });
                });
        });
    }

    fn about_lines(&self) -> Vec<String> {
        let root = self.paths.root().display().to_string();
        let logs = self.paths.logs().display().to_string();
        let journal = self.paths.journal().display().to_string();
        vec![
            self.loc.t("about.description"),
            self.loc.t("about.license"),
            self.loc.t("about.stage"),
            self.loc.fmt(
                "about.sections",
                &[("count", &self.sections.len().to_string())],
            ),
            self.loc.fmt("about.data_path", &[("path", &root)]),
            self.loc.fmt("about.log_path", &[("path", &logs)]),
            self.loc.fmt("about.journal_path", &[("path", &journal)]),
        ]
    }
}

impl eframe::App for TweakerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Panels reserve space in this order: top, bottom, left, then the rest.
        self.draw_header(ui);
        self.draw_status(ui);
        self.draw_menu(ui);
        self.draw_central(ui);

        self.frames += 1;
        if self.frames == 3 {
            logging::info("ui: first frames rendered");
        }

        let screenshot_path = match &self.mode {
            RunMode::Screenshot(path) => Some(path.clone()),
            _ => None,
        };
        let smoke = matches!(self.mode, RunMode::Smoke);
        let ctx = ui.ctx().clone();

        if let Some(path) = screenshot_path {
            if !self.screenshot_requested && self.frames >= 2 {
                self.screenshot_requested = true;
                let state = Arc::clone(&self.screenshot_state);
                logging::info(&format!("ui: screenshot requested -> {}", path.display()));
                ctx.request_screenshot(move |image| {
                    match save_screenshot(&path, &image) {
                        Ok(()) => logging::info("ui: screenshot saved"),
                        Err(error) => {
                            logging::error(&format!("ui: cannot save screenshot: {error}"));
                            *state
                                .error
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()) = error;
                            state.failed.store(true, Ordering::SeqCst);
                        }
                    }
                    state.done.store(true, Ordering::SeqCst);
                });
            }
            let finished = self.screenshot_state.done.load(Ordering::SeqCst)
                || self.screenshot_state.timed_out.load(Ordering::SeqCst);
            if !self.closing && finished {
                self.closing = true;
                logging::info("screenshot: closing the window");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if smoke && !self.closing && self.started.elapsed() >= SMOKE_DURATION {
            self.closing = true;
            logging::info("smoke: window rendered, closing");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
}

/// Converts the egui image to RGBA and writes a PNG file.
fn save_screenshot(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let [width, height] = image.size;
    let mut bytes = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    image::save_buffer(
        path,
        &bytes,
        width as u32,
        height as u32,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| error.to_string())
}

/// Keeps a long path readable in the status bar.
fn short_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    const LIMIT: usize = 64;
    let length = text.chars().count();
    if length <= LIMIT {
        return text;
    }
    let tail: String = text.chars().skip(length - LIMIT).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_path_keeps_short_paths_untouched() {
        let path = std::path::Path::new(r"C:\Temp\logs");
        assert_eq!(short_path(path), r"C:\Temp\logs");
    }

    #[test]
    fn short_path_trims_long_paths_to_the_tail() {
        let long = format!("C:\\{}", "a".repeat(200));
        let shortened = short_path(std::path::Path::new(&long));
        assert!(shortened.starts_with('…'));
        assert_eq!(shortened.chars().count(), 65);
        assert!(shortened.ends_with('a'));
    }

    fn test_dir(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before 1970")
            .as_nanos();
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("app-{tag}-{unique}"))
    }

    #[test]
    fn screenshot_is_saved_as_png() {
        let image = egui::ColorImage {
            size: [4, 3],
            source_size: egui::vec2(4.0, 3.0),
            pixels: vec![egui::Color32::from_rgb(10, 20, 30); 12],
        };
        let dir = test_dir("png");
        std::fs::create_dir_all(&dir).expect("create test dir");
        let path = dir.join("shot.png");
        save_screenshot(&path, &image).expect("save screenshot");
        let bytes = std::fs::read(&path).expect("read screenshot");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::open(&path).expect("decode screenshot");
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 3);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
