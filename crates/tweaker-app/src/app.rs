//! Main window: left menu with the 14 sections, a content area and a status bar.
//!
//! The skeleton deliberately has no business logic: it proves that the window,
//! localization, logging and the elevation check work. Tweak items arrive in the
//! next stages, always as catalog data, never as `if` branches in this file.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use tweaker_core::action::{Action, RegistryKey, RegistryRoot, RegistryValue};
use tweaker_core::adapters::{IStartup, StartupEntry, StartupSource};
use tweaker_core::catalog;
use tweaker_core::engine::Engine;
use tweaker_core::journal::Journal;
use tweaker_core::logging;
use tweaker_core::model::{RiskLevel, TweakDefinition, TweakState};
use tweaker_core::paths::ProductPaths;
use tweaker_core::product::{PRODUCT_DIR_NAME, PRODUCT_VERSION, version_string};
use tweaker_core::sections::{Section, SectionKind};
use tweaker_windows::elevation::{self, ElevationResult};
use tweaker_windows::files::WindowsFileSystem;
use tweaker_windows::message;
use tweaker_windows::process::SystemProcessRunner;
use tweaker_windows::registry::WindowsRegistry;
use tweaker_windows::scheduler::WindowsScheduler;
use tweaker_windows::services::WindowsServices;
use tweaker_windows::single_instance::SingleInstance;
use tweaker_windows::startup::WindowsStartup;

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
///
/// `section` opens a specific section by id (used by `--section` for screenshots
/// and manual checks); `None` opens the first section.
pub fn run(mode: RunMode, section: Option<String>, args: &[String]) -> Result<u8, String> {
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

    // Section ids are validated before the optional elevation restart: the new
    // process receives the same arguments, and an unknown id must fail either way.
    let sections = tweaker_core::sections::load_sections()
        .map_err(|error| format!("cannot load section catalog: {error}"))?;
    logger.info(&format!("sections: {}", sections.len()));
    if let Some(id) = section.as_deref()
        && !sections.iter().any(|candidate| candidate.id == id)
    {
        return Err(format!("unknown section: {id}"));
    }

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
            args,
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

    // A second window would share the journal with the first one; automated modes
    // are exempt so tests and screenshots can run in parallel.
    let _instance = if matches!(mode, RunMode::Gui) {
        match SingleInstance::acquire(&instance_name()) {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => {
                logger.warn("instance: another copy is already running");
                message::info(&loc.t("instance.title"), &loc.t("instance.body"));
                return Ok(0);
            }
            Err(error) => {
                logger.error(&format!("instance: cannot take the guard: {error}"));
                return Ok(1);
            }
        }
    } else {
        None
    };

    run_window(loc, paths, sections, mode, section)
}

/// Name of the session-local mutex that marks a running window.
fn instance_name() -> String {
    format!("Local\\{PRODUCT_DIR_NAME}.Instance")
}

fn run_window(
    loc: Localizer,
    paths: ProductPaths,
    sections: Vec<Section>,
    mode: RunMode,
    initial_section: Option<String>,
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
            let app = TweakerApp::new(
                loc,
                paths,
                sections,
                admin,
                mode,
                initial_section,
                app_state,
            );
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
    registry: WindowsRegistry,
    files: WindowsFileSystem,
    services: WindowsServices,
    processes: SystemProcessRunner,
    scheduler: WindowsScheduler,
    journal: Option<Journal>,
    catalog: CatalogView,
    startup: StartupView,
}

/// UI state of the autorun section.
#[derive(Default)]
struct StartupView {
    entries: Vec<StartupEntry>,
    selected: Option<usize>,
    report: Option<String>,
    pending_delete: Option<usize>,
    loaded: bool,
}

/// UI state of one catalog section.
#[derive(Default)]
struct CatalogView {
    /// Section id whose data is loaded; empty until the first selection.
    section: String,
    tweaks: Vec<TweakDefinition>,
    selected: Vec<bool>,
    states: Vec<TweakState>,
    preview: Vec<String>,
    pending: Vec<String>,
    report: Option<Vec<String>>,
}

/// One click on a catalog screen.
enum CatalogAction {
    Refresh,
    BuildPreview,
    Confirm,
    Cancel,
    Revert,
    ClearReport,
}

/// One precomputed row of the privacy list.
struct CatalogRow {
    title: String,
    description: String,
    risk: String,
    risk_color: egui::Color32,
    state: String,
    state_color: egui::Color32,
    reboot: bool,
}

/// Loads the catalog of the section stored in `view.section`.
///
/// Unknown sections (tools, or data not added yet) keep the list empty so the
/// screen falls back to the placeholder.
fn load_catalog(view: &mut CatalogView) {
    match catalog::load_section(&view.section) {
        Ok(Some(tweaks)) => {
            logging::info(&format!(
                "catalog: {} items in {}",
                tweaks.len(),
                view.section
            ));
            view.selected = vec![false; tweaks.len()];
            view.states.clear();
            view.tweaks = tweaks;
        }
        Ok(None) => {
            view.selected.clear();
            view.states.clear();
            view.tweaks.clear();
        }
        Err(error) => {
            logging::error(&format!("catalog: cannot load {}: {error}", view.section));
            view.selected.clear();
            view.states.clear();
            view.tweaks.clear();
        }
    }
}

fn catalog_state_text(loc: &Localizer, state: &TweakState) -> (String, egui::Color32) {
    match state {
        TweakState::Applied => (
            loc.t("privacy.applied"),
            egui::Color32::from_rgb(21, 128, 61),
        ),
        TweakState::NotApplied => (loc.t("privacy.not_applied"), egui::Color32::from_gray(120)),
        TweakState::Unavailable { reason } => (
            loc.fmt("privacy.unavailable", &[("reason", reason)]),
            egui::Color32::from_rgb(180, 83, 9),
        ),
    }
}

impl TweakerApp {
    fn new(
        loc: Localizer,
        paths: ProductPaths,
        sections: Vec<Section>,
        admin: bool,
        mode: RunMode,
        initial_section: Option<String>,
        screenshot_state: Arc<ScreenshotState>,
    ) -> Self {
        let selected = initial_section
            .as_deref()
            .and_then(|id| sections.iter().position(|section| section.id == id))
            .unwrap_or(0);
        let mut catalog = CatalogView {
            section: sections[selected].id.clone(),
            ..CatalogView::default()
        };
        load_catalog(&mut catalog);
        let journal = match Journal::new(paths.journal()) {
            Ok(journal) => Some(journal),
            Err(error) => {
                logging::error(&format!("journal: cannot open: {error}"));
                None
            }
        };
        Self {
            loc,
            sections,
            selected,
            admin,
            mode,
            started: Instant::now(),
            frames: 0,
            screenshot_requested: false,
            screenshot_state,
            closing: false,
            paths,
            registry: WindowsRegistry,
            files: WindowsFileSystem,
            services: WindowsServices,
            processes: SystemProcessRunner,
            scheduler: WindowsScheduler,
            journal,
            catalog,
            startup: StartupView::default(),
        }
    }

    /// Switches the visible section and loads its catalog data when needed.
    fn select_section(&mut self, index: usize) {
        if index == self.selected {
            return;
        }
        self.selected = index;
        let id = self.sections[index].id.clone();
        self.startup = StartupView::default();
        if id == "autorun" {
            self.refresh_startup();
            return;
        }
        if self.catalog.section == id {
            return;
        }
        self.catalog = CatalogView {
            section: id,
            ..CatalogView::default()
        };
        load_catalog(&mut self.catalog);
    }

    /// Reloads the autorun list from the system.
    fn refresh_startup(&mut self) {
        match WindowsStartup.entries() {
            Ok(entries) => {
                logging::info(&format!("autorun: {} entries", entries.len()));
                self.startup.entries = entries;
                self.startup.loaded = true;
                if self
                    .startup
                    .selected
                    .is_some_and(|index| index >= self.startup.entries.len())
                {
                    self.startup.selected = None;
                }
            }
            Err(error) => {
                logging::error(&format!("autorun: cannot read entries: {}", error.message));
                self.startup.entries.clear();
                self.startup.selected = None;
                self.startup.loaded = true;
                self.startup.report = Some(error.message);
            }
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
        if selected != self.selected {
            self.select_section(selected);
        }
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
        let has_catalog = self.catalog.section == section.id && !self.catalog.tweaks.is_empty();
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
                    if section.id == "autorun" {
                        self.draw_autorun(ui);
                    } else if has_catalog {
                        self.draw_catalog(ui);
                    } else {
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
                    }
                });
        });
    }

    /// Autorun list: five sources, toggle and delete through the engine.
    fn draw_autorun(&mut self, ui: &mut egui::Ui) {
        if !self.startup.loaded {
            self.refresh_startup();
        }
        let loc = &self.loc;
        let rows: Vec<(String, String, String, bool, String)> = self
            .startup
            .entries
            .iter()
            .map(|entry| {
                let state = if entry.enabled {
                    loc.t("autorun.enabled")
                } else {
                    loc.t("autorun.disabled")
                };
                (
                    entry.name.clone(),
                    loc.t(&format!("autorun.source.{}", entry.source.label())),
                    entry.command.clone(),
                    entry.enabled,
                    state,
                )
            })
            .collect();
        let mut action: Option<StartupAction> = None;
        ui.horizontal(|ui| {
            if ui.button(loc.t("autorun.refresh")).clicked() {
                action = Some(StartupAction::Refresh);
            }
            ui.label(loc.fmt("kind.catalog", &[("count", &rows.len().to_string())]));
        });
        ui.add_space(8.0);
        if rows.is_empty() {
            ui.label(loc.t("autorun.empty"));
        } else {
            for (index, (name, source, command, enabled, state)) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    let selected = self.startup.selected == Some(index);
                    if ui.selectable_label(selected, name).clicked() {
                        action = Some(StartupAction::Select(index));
                    }
                    ui.label(
                        egui::RichText::new(source)
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                    );
                    let color = if *enabled {
                        egui::Color32::from_rgb(21, 128, 61)
                    } else {
                        egui::Color32::from_gray(120)
                    };
                    ui.label(egui::RichText::new(state).size(11.0).color(color));
                    ui.label(
                        egui::RichText::new(short_path(std::path::Path::new(command)))
                            .size(11.0)
                            .color(egui::Color32::from_gray(140)),
                    );
                });
            }
        }
        if let Some(index) = self.startup.selected
            && let Some(entry) = self.startup.entries.get(index)
        {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(loc.fmt("autorun.command", &[("command", &entry.command)]));
            ui.horizontal(|ui| {
                let toggle = if entry.enabled {
                    loc.t("autorun.toggle_off")
                } else {
                    loc.t("autorun.toggle_on")
                };
                if ui.button(toggle).clicked() {
                    action = Some(StartupAction::Toggle(index));
                }
                if entry.source != StartupSource::ScheduledTask
                    && ui.button(loc.t("autorun.delete")).clicked()
                {
                    action = Some(StartupAction::AskDelete(index));
                }
                if entry.source != StartupSource::ScheduledTask
                    && ui.button(loc.t("autorun.open")).clicked()
                {
                    action = Some(StartupAction::Open(index));
                }
            });
        }
        if self.startup.pending_delete.is_some() {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(loc.t("autorun.confirm"))
                            .size(14.0)
                            .strong(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(loc.t("autorun.confirm_yes")).clicked() {
                            action = Some(StartupAction::ConfirmDelete);
                        }
                        if ui.button(loc.t("autorun.cancel")).clicked() {
                            action = Some(StartupAction::CancelDelete);
                        }
                    });
                });
        }
        if let Some(report) = &self.startup.report {
            ui.add_space(8.0);
            ui.label(report);
        }

        match action {
            Some(StartupAction::Refresh) => self.refresh_startup(),
            Some(StartupAction::Select(index)) => {
                self.startup.selected = Some(index);
                self.startup.pending_delete = None;
            }
            Some(StartupAction::Toggle(index)) => self.toggle_startup(index),
            Some(StartupAction::AskDelete(index)) => {
                self.startup.pending_delete = Some(index);
            }
            Some(StartupAction::ConfirmDelete) => {
                if let Some(index) = self.startup.pending_delete.take() {
                    self.delete_startup(index);
                }
            }
            Some(StartupAction::CancelDelete) => {
                self.startup.pending_delete = None;
            }
            Some(StartupAction::Open(index)) => self.open_startup_location(index),
            None => {}
        }
    }

    fn toggle_startup(&mut self, index: usize) {
        let Some(entry) = self.startup.entries.get(index).cloned() else {
            return;
        };
        let action = startup_toggle_action(&entry);
        let label = entry.name.clone();
        self.apply_startup(vec![action], &label);
        self.refresh_startup();
    }

    fn delete_startup(&mut self, index: usize) {
        let Some(entry) = self.startup.entries.get(index).cloned() else {
            return;
        };
        let Some(action) = startup_delete_action(&entry) else {
            self.startup.report = Some(self.loc.t("autorun.task_delete"));
            return;
        };
        let label = entry.name.clone();
        self.apply_startup(vec![action], &label);
        self.startup.selected = None;
        self.refresh_startup();
    }

    fn open_startup_location(&mut self, index: usize) {
        let Some(entry) = self.startup.entries.get(index) else {
            return;
        };
        if let Err(error) = WindowsStartup.open_location(entry) {
            self.startup.report =
                Some(self.loc.fmt("autorun.error", &[("reason", &error.message)]));
        }
    }

    fn apply_startup(&mut self, actions: Vec<Action>, label: &str) {
        let Some(journal) = self.journal.as_ref() else {
            let reason = "the change journal is unavailable".to_owned();
            self.startup.report = Some(
                self.loc
                    .fmt("privacy.journal_missing", &[("reason", &reason)]),
            );
            return;
        };
        let tweak = TweakDefinition::new(
            "AUTORUN",
            "autorun",
            "section.autorun",
            "hint.autorun",
            RiskLevel::Medium,
            false,
            actions,
        );
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        );
        let report = engine.apply_tweak(&tweak);
        if report.success() {
            self.startup.report = Some(self.loc.fmt("autorun.done", &[("name", label)]));
        } else {
            let reason = report
                .failed
                .first()
                .map(|failure| failure.error.clone())
                .or(report.error)
                .unwrap_or_default();
            self.startup.report = Some(self.loc.fmt("autorun.error", &[("reason", &reason)]));
        }
    }

    fn draw_catalog(&mut self, ui: &mut egui::Ui) {
        if self.catalog.states.len() != self.catalog.tweaks.len() {
            self.refresh_catalog_states();
        }
        let loc = &self.loc;
        let mut rows = Vec::with_capacity(self.catalog.tweaks.len());
        for (index, tweak) in self.catalog.tweaks.iter().enumerate() {
            let state = self.catalog.states.get(index).cloned().unwrap_or_else(|| {
                TweakState::Unavailable {
                    reason: "state is not detected".to_owned(),
                }
            });
            let (state_text, state_color) = catalog_state_text(loc, &state);
            let risk_text = loc.t(&format!("privacy.risk.{}", tweak.risk.label()));
            let risk_color = match tweak.risk {
                RiskLevel::Low => egui::Color32::from_rgb(21, 128, 61),
                RiskLevel::Medium => egui::Color32::from_gray(120),
                RiskLevel::High => egui::Color32::from_rgb(180, 83, 9),
                RiskLevel::Critical => egui::Color32::from_rgb(185, 28, 28),
            };
            rows.push(CatalogRow {
                title: loc.t(&tweak.title_key),
                description: loc.t(&tweak.description_key),
                risk: risk_text,
                risk_color,
                state: state_text,
                state_color,
                reboot: tweak.requires_reboot,
            });
        }
        let mut selected = self.catalog.selected.clone();
        let mut action: Option<CatalogAction> = None;
        let selected_count = selected.iter().filter(|value| **value).count();
        let risky_selected = self
            .catalog
            .tweaks
            .iter()
            .enumerate()
            .any(|(index, tweak)| {
                selected.get(index).copied().unwrap_or(false) && tweak.risk.requires_confirmation()
            });

        ui.horizontal(|ui| {
            if ui.button(loc.t("privacy.reload")).clicked() {
                action = Some(CatalogAction::Refresh);
            }
            if ui.button(loc.t("privacy.apply")).clicked() {
                action = Some(CatalogAction::BuildPreview);
            }
            if ui.button(loc.t("privacy.revert")).clicked() {
                action = Some(CatalogAction::Revert);
            }
            ui.label(loc.fmt(
                "privacy.selected",
                &[("count", &selected_count.to_string())],
            ));
            ui.label(loc.fmt("privacy.items", &[("count", &rows.len().to_string())]));
        });
        if risky_selected {
            ui.label(
                egui::RichText::new(loc.t("privacy.need_confirm"))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(180, 83, 9)),
            );
        }
        ui.add_space(6.0);

        for (index, row) in rows.iter().enumerate() {
            ui.horizontal(|ui| {
                if selected.len() > index {
                    ui.checkbox(&mut selected[index], "");
                }
                ui.label(egui::RichText::new(&row.title).size(14.0).strong());
                ui.label(
                    egui::RichText::new(&row.risk)
                        .size(11.0)
                        .color(row.risk_color),
                );
                ui.label(
                    egui::RichText::new(&row.state)
                        .size(11.0)
                        .color(row.state_color),
                );
                if row.reboot {
                    ui.label(
                        egui::RichText::new(loc.t("privacy.reboot"))
                            .size(11.0)
                            .color(egui::Color32::from_gray(130)),
                    );
                }
            })
            .response
            .on_hover_text(&row.description);
        }

        if !self.catalog.preview.is_empty() {
            ui.add_space(12.0);
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(loc.t("privacy.preview_title"))
                            .size(15.0)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    for line in &self.catalog.preview {
                        ui.label(line);
                    }
                    if risky_selected {
                        ui.label(
                            egui::RichText::new(loc.t("privacy.need_confirm"))
                                .size(12.0)
                                .color(egui::Color32::from_rgb(180, 83, 9)),
                        );
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(loc.t("privacy.confirm")).clicked() {
                            action = Some(CatalogAction::Confirm);
                        }
                        if ui.button(loc.t("privacy.cancel")).clicked() {
                            action = Some(CatalogAction::Cancel);
                        }
                    });
                });
        }

        if let Some(report) = &self.catalog.report {
            ui.add_space(12.0);
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(loc.t("privacy.report"))
                            .size(15.0)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    for line in report {
                        ui.label(line);
                    }
                    ui.add_space(6.0);
                    if ui.button(loc.t("privacy.done")).clicked() {
                        action = Some(CatalogAction::ClearReport);
                    }
                });
        }

        if selected != self.catalog.selected {
            self.catalog.selected = selected;
        }
        match action {
            Some(CatalogAction::Refresh) => self.refresh_catalog_states(),
            Some(CatalogAction::BuildPreview) => self.build_catalog_preview(),
            Some(CatalogAction::Confirm) => self.confirm_catalog(),
            Some(CatalogAction::Cancel) => {
                self.catalog.preview.clear();
                self.catalog.pending.clear();
            }
            Some(CatalogAction::Revert) => self.revert_catalog(),
            Some(CatalogAction::ClearReport) => self.catalog.report = None,
            None => {}
        }
    }

    fn engine_ready(&self) -> bool {
        self.journal.is_some()
    }

    fn detect_privacy(&self) -> Vec<TweakState> {
        if !self.engine_ready() {
            return self
                .catalog
                .tweaks
                .iter()
                .map(|_| TweakState::Unavailable {
                    reason: "the change journal is unavailable".to_owned(),
                })
                .collect();
        }
        let journal = self.journal.as_ref().expect("checked above");
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        );
        self.catalog
            .tweaks
            .iter()
            .map(|tweak| engine.detect(tweak))
            .collect()
    }

    fn refresh_catalog_states(&mut self) {
        self.catalog.states = self.detect_privacy();
    }

    fn selected_tweaks(&self) -> Vec<TweakDefinition> {
        self.catalog
            .tweaks
            .iter()
            .enumerate()
            .filter(|(index, _)| self.catalog.selected.get(*index).copied().unwrap_or(false))
            .map(|(_, tweak)| tweak.clone())
            .collect()
    }

    fn build_catalog_preview(&mut self) {
        let tweaks = self.selected_tweaks();
        if tweaks.is_empty() {
            self.catalog.report = Some(vec![self.loc.t("privacy.no_selection")]);
            return;
        }
        if !self.engine_ready() {
            let reason = "the change journal is unavailable".to_owned();
            self.catalog.report = Some(vec![
                self.loc
                    .fmt("privacy.journal_missing", &[("reason", &reason)]),
            ]);
            return;
        }
        let journal = self.journal.as_ref().expect("checked above");
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        );
        let mut lines = Vec::new();
        let mut ids = Vec::new();
        for tweak in &tweaks {
            let title = self.loc.t(&tweak.title_key);
            ids.push(tweak.id.clone());
            match engine.plan(tweak) {
                Ok(plan) if plan.is_empty() => {
                    lines.push(format!("{title}: {}", self.loc.t("privacy.applied")));
                }
                Ok(plan) => {
                    lines.push(title);
                    for step in plan.steps {
                        lines.push(format!("  • {}", step.preview));
                    }
                }
                Err(error) => lines.push(format!(
                    "{title}: {}",
                    self.loc
                        .fmt("privacy.error", &[("message", &error.to_string())])
                )),
            }
        }
        self.catalog.preview = lines;
        self.catalog.pending = ids;
        self.catalog.report = None;
    }

    fn confirm_catalog(&mut self) {
        let ids = std::mem::take(&mut self.catalog.pending);
        if ids.is_empty() {
            self.catalog.preview.clear();
            return;
        }
        if !self.engine_ready() {
            let reason = "the change journal is unavailable".to_owned();
            self.catalog.report = Some(vec![
                self.loc
                    .fmt("privacy.journal_missing", &[("reason", &reason)]),
            ]);
            self.catalog.preview.clear();
            return;
        }
        let journal = self.journal.as_ref().expect("checked above");
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        );
        let mut lines = Vec::new();
        for id in &ids {
            let Some(tweak) = self.catalog.tweaks.iter().find(|tweak| &tweak.id == id) else {
                continue;
            };
            let title = self.loc.t(&tweak.title_key);
            let report = engine.apply_tweak(tweak);
            if let Some(error) = &report.error {
                lines.push(format!(
                    "{title}: {}",
                    self.loc.fmt("privacy.error", &[("message", error)])
                ));
                continue;
            }
            lines.push(format!(
                "{title}: +{} / ={} / !{}",
                report.applied.len(),
                report.skipped.len(),
                report.failed.len()
            ));
            for failure in &report.failed {
                lines.push(format!("  • {}", failure.error));
            }
        }
        self.catalog.preview.clear();
        self.catalog.report = Some(lines);
        self.refresh_catalog_states();
    }

    fn revert_catalog(&mut self) {
        let ids: Vec<String> = self
            .selected_tweaks()
            .into_iter()
            .map(|tweak| tweak.id)
            .collect();
        if ids.is_empty() {
            self.catalog.report = Some(vec![self.loc.t("privacy.no_selection")]);
            return;
        }
        if !self.engine_ready() {
            let reason = "the change journal is unavailable".to_owned();
            self.catalog.report = Some(vec![
                self.loc
                    .fmt("privacy.journal_missing", &[("reason", &reason)]),
            ]);
            return;
        }
        let journal = self.journal.as_ref().expect("checked above");
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        );
        let mut lines = Vec::new();
        for id in &ids {
            let title = self
                .catalog
                .tweaks
                .iter()
                .find(|tweak| &tweak.id == id)
                .map(|tweak| self.loc.t(&tweak.title_key))
                .unwrap_or_else(|| id.clone());
            let report = engine.revert(id);
            if let Some(error) = &report.error {
                lines.push(format!(
                    "{title}: {}",
                    self.loc.fmt("privacy.error", &[("message", error)])
                ));
                continue;
            }
            if report.nothing_to_revert {
                lines.push(format!("{title}: {}", self.loc.t("privacy.revert_none")));
                continue;
            }
            lines.push(format!(
                "{title}: {} {}",
                self.loc.t("privacy.reverted"),
                report.reverted.len()
            ));
            for failure in &report.failed {
                lines.push(format!("  • {}", failure.error));
            }
        }
        self.catalog.report = Some(lines);
        self.refresh_catalog_states();
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

/// One click on the autorun screen.
enum StartupAction {
    Refresh,
    Select(usize),
    Toggle(usize),
    AskDelete(usize),
    ConfirmDelete,
    CancelDelete,
    Open(usize),
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_WOW_KEY: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// Builds the action that flips the enabled state.
fn startup_toggle_action(entry: &StartupEntry) -> Action {
    let enabled = !entry.enabled;
    let approval = format!(
        "{APPROVED_KEY}\\{}",
        match entry.source {
            StartupSource::HklmRun32 => "Run32",
            StartupSource::StartupFolder => "StartupFolder",
            _ => "Run",
        }
    );
    match entry.source {
        StartupSource::ScheduledTask => Action::TaskSetEnabled {
            path: entry.name.clone(),
            enabled,
        },
        _ => Action::RegistrySet {
            key: RegistryKey::new(RegistryRoot::Hkcu, approval),
            name: entry.name.clone(),
            value: RegistryValue::Binary(approval_binary(enabled)),
        },
    }
}

/// Builds the action that removes the entry; tasks cannot be deleted here yet.
fn startup_delete_action(entry: &StartupEntry) -> Option<Action> {
    match entry.source {
        StartupSource::HkcuRun => Some(Action::RegistryDelete {
            key: RegistryKey::new(RegistryRoot::Hkcu, RUN_KEY),
            name: entry.name.clone(),
        }),
        StartupSource::HklmRun => Some(Action::RegistryDelete {
            key: RegistryKey::new(RegistryRoot::Hklm, RUN_KEY),
            name: entry.name.clone(),
        }),
        StartupSource::HklmRun32 => Some(Action::RegistryDelete {
            key: RegistryKey::new(RegistryRoot::Hklm, RUN_WOW_KEY),
            name: entry.name.clone(),
        }),
        StartupSource::StartupFolder => Some(Action::FileDeleteAny {
            path: entry.location.clone(),
        }),
        StartupSource::ScheduledTask => None,
    }
}

/// `StartupApproved` keeps a 12-byte record; 2 means enabled, 3 means disabled.
fn approval_binary(enabled: bool) -> Vec<u8> {
    let mut bytes = vec![0u8; 12];
    bytes[0] = if enabled { 2 } else { 3 };
    bytes
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

    fn startup_entry(source: StartupSource, name: &str) -> StartupEntry {
        StartupEntry {
            source,
            name: name.to_owned(),
            command: format!("C:\\{name}.exe"),
            enabled: true,
            location: std::path::PathBuf::from(format!("C:\\{name}.lnk")),
        }
    }

    #[test]
    fn startup_toggle_uses_the_approval_record() {
        let action = startup_toggle_action(&startup_entry(StartupSource::HkcuRun, "App"));
        match action {
            Action::RegistrySet { key, name, value } => {
                assert_eq!(key.root, RegistryRoot::Hkcu);
                assert!(key.path.ends_with(r"StartupApproved\Run"));
                assert_eq!(name, "App");
                assert_eq!(
                    value,
                    RegistryValue::Binary(vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                );
            }
            other => panic!("unexpected action: {other:?}"),
        }
        let action = startup_toggle_action(&startup_entry(StartupSource::HklmRun32, "Old"));
        match action {
            Action::RegistrySet { key, .. } => {
                assert!(key.path.ends_with(r"StartupApproved\Run32"))
            }
            other => panic!("unexpected action: {other:?}"),
        }
        let action =
            startup_toggle_action(&startup_entry(StartupSource::ScheduledTask, r"\Test\Task"));
        assert_eq!(
            action,
            Action::TaskSetEnabled {
                path: r"\Test\Task".to_owned(),
                enabled: false
            }
        );
    }

    #[test]
    fn startup_delete_uses_the_right_source() {
        assert_eq!(
            startup_delete_action(&startup_entry(StartupSource::HkcuRun, "App")),
            Some(Action::RegistryDelete {
                key: RegistryKey::new(RegistryRoot::Hkcu, RUN_KEY),
                name: "App".to_owned()
            })
        );
        assert_eq!(
            startup_delete_action(&startup_entry(StartupSource::HklmRun, "Shared")),
            Some(Action::RegistryDelete {
                key: RegistryKey::new(RegistryRoot::Hklm, RUN_KEY),
                name: "Shared".to_owned()
            })
        );
        assert_eq!(
            startup_delete_action(&startup_entry(StartupSource::HklmRun32, "Old")),
            Some(Action::RegistryDelete {
                key: RegistryKey::new(RegistryRoot::Hklm, RUN_WOW_KEY),
                name: "Old".to_owned()
            })
        );
        assert_eq!(
            startup_delete_action(&startup_entry(StartupSource::StartupFolder, "Tool")),
            Some(Action::FileDeleteAny {
                path: std::path::PathBuf::from(r"C:\Tool.lnk")
            })
        );
        assert_eq!(
            startup_delete_action(&startup_entry(StartupSource::ScheduledTask, r"\Test\Task")),
            None
        );
    }

    #[test]
    fn startup_approval_bytes_mark_enabled_and_disabled() {
        assert_eq!(approval_binary(true)[0], 2);
        assert_eq!(approval_binary(false)[0], 3);
        assert_eq!(approval_binary(true).len(), 12);
    }

    #[test]
    fn catalog_loader_reads_services_and_skips_unknown_sections() {
        let mut view = CatalogView {
            section: "services".to_owned(),
            ..CatalogView::default()
        };
        load_catalog(&mut view);
        assert_eq!(view.tweaks.len(), 24);
        assert_eq!(view.selected.len(), 24);
        assert!(view.tweaks.iter().all(|tweak| tweak.section == "services"));

        let mut unknown = CatalogView {
            section: "sysinfo".to_owned(),
            ..CatalogView::default()
        };
        load_catalog(&mut unknown);
        assert!(unknown.tweaks.is_empty());
        assert!(unknown.selected.is_empty());
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
