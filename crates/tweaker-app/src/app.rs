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
use tweaker_core::adapters::{IFileSystem, IStartup, ISystemInfo, StartupEntry, StartupSource};
use tweaker_core::catalog;
use tweaker_core::engine::{ApplyReport, Engine, RevertReport};
use tweaker_core::journal::Journal;
use tweaker_core::logging;
use tweaker_core::model::{Plan, RiskLevel, TweakDefinition, TweakState};
use tweaker_core::paths::ProductPaths;
use tweaker_core::product::{PRODUCT_DIR_NAME, PRODUCT_VERSION, version_string};
use tweaker_core::sections::{Section, SectionKind};
use tweaker_windows::appx::WindowsAppx;
use tweaker_windows::elevation::{self, ElevationResult};
use tweaker_windows::files::WindowsFileSystem;
use tweaker_windows::message;
use tweaker_windows::os::WindowsSystemInfo;
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
    appx: WindowsAppx,
    journal: Option<Arc<Journal>>,
    catalog: CatalogView,
    startup: StartupView,
    apps: AppsView,
    job: Option<ApplyJob>,
    sysinfo: SysinfoView,
    recovery_report: Option<Vec<String>>,
    pending_recovery: Option<usize>,
}
/// One click on the applications screen.
enum AppsAction {
    Refresh,
    AskRemove(Vec<usize>),
    AskRestore(Vec<usize>),
    Confirm,
    Cancel,
}

/// What the confirmation panel will do.
#[derive(Clone, Copy)]
enum AppsPending {
    Remove,
    Restore,
}

/// One long-running apply job executed off the UI thread.
struct ApplyJob {
    progress: Arc<Mutex<JobProgress>>,
    cancel: Arc<AtomicBool>,
    outcome: Arc<Mutex<Option<JobOutcome>>>,
}

/// Live progress of the background job.
#[derive(Default, Clone)]
struct JobProgress {
    index: usize,
    total: usize,
    action: String,
}

/// Final result of the background job.
struct JobOutcome {
    tweaks: Vec<TweakOutcome>,
}

/// Result of one tweak inside the background job.
struct TweakOutcome {
    title_key: String,
    applied: usize,
    skipped: usize,
    failed: Vec<String>,
    error: Option<String>,
    cancelled: bool,
    freed_bytes: u64,
    skipped_files: usize,
}

/// UI state of the system information screen.
#[derive(Default)]
struct SysinfoView {
    rows: Vec<(String, String)>,
    report: Option<String>,
    loaded: bool,
}

/// UI state of the applications section.
#[derive(Default)]
struct AppsView {
    tweaks: Vec<TweakDefinition>,
    states: Vec<TweakState>,
    pending: Option<(AppsPending, Vec<usize>)>,
    report: Option<String>,
    loaded: bool,
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
    sizes: Vec<Option<u64>>,
    report: Option<Vec<String>>,
}

/// One click on a catalog screen.
enum CatalogAction {
    Refresh,
    Measure,
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
            Ok(journal) => Some(Arc::new(journal)),
            Err(error) => {
                logging::error(&format!("journal: cannot open: {error}"));
                None
            }
        };
        let pending_recovery = journal
            .as_ref()
            .and_then(|journal| match journal.pending() {
                Ok(pending) if !pending.is_empty() => {
                    logging::warn(&format!(
                        "journal: {} unfinished operations found",
                        pending.len()
                    ));
                    Some(pending.len())
                }
                Ok(_) => None,
                Err(error) => {
                    logging::warn(&format!("journal: cannot read pending operations: {error}"));
                    None
                }
            });
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
            appx: WindowsAppx,
            journal,
            catalog,
            startup: StartupView::default(),
            apps: AppsView::default(),
            job: None,
            sysinfo: SysinfoView::default(),
            pending_recovery,
            recovery_report: None,
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
        if id == "apps" {
            self.apps = AppsView::default();
            self.refresh_apps();
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
        let mut recovery_action: Option<bool> = None;

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
                    if let Some(count) = self.pending_recovery {
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(
                                        self.loc
                                            .fmt("recovery.body", &[("count", &count.to_string())]),
                                    )
                                    .size(13.0),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button(self.loc.t("recovery.revert")).clicked() {
                                        recovery_action = Some(true);
                                    }
                                    if ui.button(self.loc.t("recovery.hide")).clicked() {
                                        recovery_action = Some(false);
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                    if let Some(lines) = &self.recovery_report {
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                for line in lines {
                                    ui.label(line);
                                }
                                if ui.button(self.loc.t("recovery.hide")).clicked() {
                                    recovery_action = Some(false);
                                }
                            });
                        ui.add_space(8.0);
                    }
                    if section.id == "autorun" {
                        self.draw_autorun(ui);
                    } else if section.id == "sysinfo" {
                        self.draw_sysinfo(ui);
                    } else if section.id == "apps" {
                        self.draw_apps(ui);
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
        match recovery_action {
            Some(true) => self.revert_pending_ops(),
            Some(false) => {
                self.pending_recovery = None;
                self.recovery_report = None;
            }
            None => {}
        }
    }

    /// Reverts operations that were started but never finished (crash recovery).
    fn revert_pending_ops(&mut self) {
        self.pending_recovery = None;
        let Some(journal) = self.journal.as_ref() else {
            return;
        };
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        )
        .with_appx(&self.appx);
        let report = engine.revert_pending();
        let mut lines = Vec::new();
        if let Some(error) = &report.error {
            lines.push(self.loc.fmt("privacy.error", &[("message", error)]));
        } else if report.reverted.is_empty() {
            lines.push(self.loc.t("recovery.nothing"));
        } else {
            lines.push(self.loc.fmt(
                "recovery.done",
                &[("count", &report.reverted.len().to_string())],
            ));
        }
        for failure in &report.failed {
            lines.push(format!("  • {}", failure.error));
        }
        self.recovery_report = Some(lines);
        self.refresh_catalog_states();
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

    /// Reloads the application list and its states.
    fn refresh_apps(&mut self) {
        self.apps.loaded = true;
        let tweaks = match tweaker_core::catalog::load_apps() {
            Ok(tweaks) => tweaks,
            Err(error) => {
                self.apps.tweaks.clear();
                self.apps.states.clear();
                self.apps.report = Some(
                    self.loc
                        .fmt("apps.error", &[("reason", &error.to_string())]),
                );
                return;
            }
        };
        let Some(journal) = self.journal.as_ref() else {
            let reason = "the change journal is unavailable".to_owned();
            self.apps.report = Some(self.loc.fmt("apps.error", &[("reason", &reason)]));
            return;
        };
        let engine = Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            journal,
        )
        .with_appx(&self.appx);
        let states = tweaks.iter().map(|tweak| engine.detect(tweak)).collect();
        self.apps.tweaks = tweaks;
        self.apps.states = states;
    }

    /// Reloads the system information report.
    fn refresh_sysinfo(&mut self) {
        self.sysinfo.loaded = true;
        match WindowsSystemInfo.report() {
            Ok(rows) => {
                self.sysinfo.rows = rows.into_iter().map(|row| (row.label, row.value)).collect();
                self.sysinfo.report = None;
            }
            Err(error) => {
                self.sysinfo.rows.clear();
                self.sysinfo.report =
                    Some(self.loc.fmt("sysinfo.error", &[("reason", &error.message)]));
            }
        }
    }

    fn sysinfo_text(&self) -> String {
        self.sysinfo
            .rows
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\r\n")
    }

    /// Saves the report next to the logs, without sending it anywhere.
    fn save_sysinfo(&mut self) {
        let text = self.sysinfo_text();
        let path = self.paths.config().join("sysinfo.txt");
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            self.sysinfo.report = Some(
                self.loc
                    .fmt("sysinfo.error", &[("reason", &error.to_string())]),
            );
            return;
        }
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.sysinfo.report = Some(
                    self.loc
                        .fmt("sysinfo.saved", &[("path", &path.display().to_string())]),
                );
            }
            Err(error) => {
                self.sysinfo.report = Some(
                    self.loc
                        .fmt("sysinfo.error", &[("reason", &error.to_string())]),
                );
            }
        }
    }

    /// System information: readable report, copy and save, nothing is sent anywhere.
    fn draw_sysinfo(&mut self, ui: &mut egui::Ui) {
        if !self.sysinfo.loaded {
            self.refresh_sysinfo();
        }

        let mut refresh = false;
        let mut copy = false;
        let mut save = false;
        ui.horizontal(|ui| {
            if ui.button(self.loc.t("sysinfo.refresh")).clicked() {
                refresh = true;
            }
            if ui.button(self.loc.t("sysinfo.copy")).clicked() {
                copy = true;
            }
            if ui.button(self.loc.t("sysinfo.save")).clicked() {
                save = true;
            }
        });
        ui.add_space(8.0);
        let text = self.sysinfo_text();
        if copy {
            ui.ctx().copy_text(text.clone());
            self.sysinfo.report = Some(self.loc.t("sysinfo.copied"));
        }
        if refresh {
            self.refresh_sysinfo();
        }
        if save {
            self.save_sysinfo();
        }
        if self.sysinfo.rows.is_empty() {
            ui.label(self.loc.t("sysinfo.empty"));
        } else {
            egui::Grid::new("sysinfo_grid")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for (label, value) in &self.sysinfo.rows {
                        ui.label(egui::RichText::new(label).strong());
                        ui.add(egui::Label::new(value).wrap());
                        ui.end_row();
                    }
                });
        }
        if let Some(report) = &self.sysinfo.report {
            ui.add_space(10.0);
            ui.label(report);
        }
    }

    /// Applications screen: a tile per package, removal and restore with confirmation.
    fn draw_apps(&mut self, ui: &mut egui::Ui) {
        if !self.apps.loaded {
            self.refresh_apps();
        }
        let loc = &self.loc;
        let rows: Vec<AppRow> = self
            .apps
            .tweaks
            .iter()
            .enumerate()
            .map(|(index, tweak)| {
                let state = self.apps.states.get(index).cloned().unwrap_or_else(|| {
                    TweakState::Unavailable {
                        reason: "state is not detected".to_owned(),
                    }
                });
                let (state_key, color) = app_state_text(&state);
                AppRow {
                    title: loc.t(&tweak.title_key),
                    state: loc.t(state_key),
                    color,
                    installed: matches!(state, TweakState::NotApplied),
                    removed: matches!(state, TweakState::Applied),
                }
            })
            .collect();
        let installed = apps_with_state(&self.apps.states, true);
        let removed = apps_with_state(&self.apps.states, false);
        let mut action: Option<AppsAction> = None;

        ui.horizontal(|ui| {
            if ui.button(loc.t("apps.refresh")).clicked() {
                action = Some(AppsAction::Refresh);
            }
            if ui.button(loc.t("apps.remove_all")).clicked() {
                action = Some(AppsAction::AskRemove(installed.clone()));
            }
            if ui.button(loc.t("apps.restore_all")).clicked() {
                action = Some(AppsAction::AskRestore(removed.clone()));
            }
            ui.label(loc.fmt("kind.catalog", &[("count", &rows.len().to_string())]));
        });
        ui.add_space(8.0);
        if rows.is_empty() {
            ui.label(loc.t("apps.empty"));
        } else {
            egui::Grid::new("apps_grid")
                .num_columns(3)
                .spacing([10.0, 10.0])
                .show(ui, |ui| {
                    for (index, row) in rows.iter().enumerate() {
                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(248.0);
                                ui.horizontal(|ui| {
                                    let letter = row
                                        .title
                                        .chars()
                                        .next()
                                        .map(|value| value.to_string())
                                        .unwrap_or_default();
                                    draw_app_icon(ui, &letter, row.color);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&row.title).size(14.0).strong(),
                                        );
                                        ui.label(
                                            egui::RichText::new(&row.state)
                                                .size(12.0)
                                                .color(row.color),
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if row.installed && ui.button(loc.t("apps.remove")).clicked() {
                                        action = Some(AppsAction::AskRemove(vec![index]));
                                    }
                                    if row.removed && ui.button(loc.t("apps.restore")).clicked() {
                                        action = Some(AppsAction::AskRestore(vec![index]));
                                    }
                                });
                            });
                        if (index + 1) % 3 == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
        if let Some((kind, indices)) = &self.apps.pending {
            let names = self.app_names(indices);
            let text = match kind {
                AppsPending::Remove => loc.fmt("apps.confirm_remove", &[("names", &names)]),
                AppsPending::Restore => loc.fmt("apps.confirm_restore", &[("names", &names)]),
            };
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).size(13.5));
                    ui.horizontal(|ui| {
                        if ui.button(loc.t("apps.confirm_yes")).clicked() {
                            action = Some(AppsAction::Confirm);
                        }
                        if ui.button(loc.t("autorun.cancel")).clicked() {
                            action = Some(AppsAction::Cancel);
                        }
                    });
                });
        }
        if let Some(report) = &self.apps.report {
            ui.add_space(8.0);
            ui.label(report);
        }

        match action {
            Some(AppsAction::Refresh) => self.refresh_apps(),
            Some(AppsAction::AskRemove(indices)) => {
                if indices.is_empty() {
                    self.apps.report = Some(self.loc.t("apps.none"));
                } else {
                    self.apps.pending = Some((AppsPending::Remove, indices));
                }
            }
            Some(AppsAction::AskRestore(indices)) => {
                if indices.is_empty() {
                    self.apps.report = Some(self.loc.t("apps.none"));
                } else {
                    self.apps.pending = Some((AppsPending::Restore, indices));
                }
            }
            Some(AppsAction::Confirm) => {
                if let Some((kind, indices)) = self.apps.pending.take() {
                    self.run_apps(indices, matches!(kind, AppsPending::Remove));
                }
            }
            Some(AppsAction::Cancel) => self.apps.pending = None,
            None => {}
        }
    }

    /// Titles of the pending applications, in list order.
    fn app_names(&self, indices: &[usize]) -> String {
        indices
            .iter()
            .filter_map(|index| self.apps.tweaks.get(*index))
            .map(|tweak| self.loc.t(&tweak.title_key))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Removes or restores the selected applications, one journaled tweak each.
    fn run_apps(&mut self, indices: Vec<usize>, remove: bool) {
        let Some(journal) = self.journal.as_ref() else {
            let reason = "the change journal is unavailable".to_owned();
            self.apps.report = Some(self.loc.fmt("apps.error", &[("reason", &reason)]));
            return;
        };
        let mut done = 0usize;
        let mut failures: Vec<String> = Vec::new();
        let mut store_hint = false;
        {
            let engine = Engine::new(
                &self.registry,
                &self.files,
                &self.services,
                &self.processes,
                &self.scheduler,
                journal,
            )
            .with_appx(&self.appx);
            for index in indices {
                let Some(tweak) = self.apps.tweaks.get(index) else {
                    continue;
                };
                if remove {
                    let report = engine.apply_tweak(tweak);
                    if report.success() {
                        done += 1;
                    } else {
                        failures.push(report_message(&report));
                    }
                } else {
                    let report = engine.revert(&tweak.id);
                    if !report.success() {
                        failures.push(revert_message(&report));
                    } else if report.nothing_to_revert {
                        store_hint = true;
                    } else {
                        done += 1;
                    }
                }
            }
        }
        self.refresh_apps();
        let mut message = if let Some(reason) = failures.first() {
            self.loc.fmt("apps.error", &[("reason", reason)])
        } else if done == 0 && store_hint {
            self.loc.t("apps.store_hint")
        } else if done == 0 {
            self.loc.t("apps.none")
        } else {
            self.loc
                .fmt("apps.done_many", &[("count", &done.to_string())])
        };
        if store_hint && done > 0 {
            message.push(' ');
            message.push_str(&self.loc.t("apps.store_hint"));
        }
        self.apps.report = Some(message);
    }

    /// Starts a background run over already planned tweaks.
    fn start_apply_jobs(&mut self, jobs: Vec<(TweakDefinition, Plan)>) {
        let Some(journal) = self.journal.as_ref().map(Arc::clone) else {
            let reason = "the change journal is unavailable".to_owned();
            self.catalog.report = Some(vec![
                self.loc
                    .fmt("privacy.journal_missing", &[("reason", &reason)]),
            ]);
            return;
        };
        let total = jobs.iter().map(|(_, plan)| plan.steps.len()).sum();
        let progress = Arc::new(Mutex::new(JobProgress {
            index: 0,
            total,
            action: String::new(),
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let outcome = Arc::new(Mutex::new(None));
        let worker_progress = Arc::clone(&progress);
        let worker_cancel = Arc::clone(&cancel);
        let worker_outcome = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let registry = WindowsRegistry;
            let files = WindowsFileSystem;
            let services = WindowsServices;
            let processes = SystemProcessRunner;
            let scheduler = WindowsScheduler;
            let appx = WindowsAppx;
            let engine = Engine::new(
                &registry, &files, &services, &processes, &scheduler, &journal,
            )
            .with_appx(&appx);
            let mut current = 0usize;
            let mut outcomes = Vec::new();
            for (tweak, plan) in jobs {
                let title_key = tweak.title_key.clone();
                let report = engine.apply_with(&tweak, &plan, |_index, _total, action| {
                    if let Ok(mut state) = worker_progress.lock() {
                        state.index = current;
                        state.total = total;
                        state.action = action.describe();
                    }
                    current += 1;
                    !worker_cancel.load(Ordering::SeqCst)
                });
                outcomes.push(TweakOutcome {
                    title_key,
                    applied: report.applied.len(),
                    skipped: report.skipped.len(),
                    failed: report
                        .failed
                        .iter()
                        .map(|failure| failure.error.clone())
                        .collect(),
                    error: report.error,
                    cancelled: report.cancelled,
                    freed_bytes: report.freed_bytes,
                    skipped_files: report.skipped_files,
                });
                if report.cancelled {
                    break;
                }
            }
            if let Ok(mut state) = worker_progress.lock() {
                state.index = total;
                state.action.clear();
            }
            if let Ok(mut slot) = worker_outcome.lock() {
                *slot = Some(JobOutcome { tweaks: outcomes });
            }
        });
        self.job = Some(ApplyJob {
            progress,
            cancel,
            outcome,
        });
    }

    /// Collects the background result when it is ready and refreshes the states.
    fn poll_job(&mut self) {
        let Some(job) = self.job.as_ref() else {
            return;
        };
        let outcome = {
            let mut slot = job
                .outcome
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.take()
        };
        let Some(outcome) = outcome else {
            return;
        };
        self.job = None;
        let mut lines = Vec::new();
        let mut cancelled = false;
        for item in outcome.tweaks {
            let title = self.loc.t(&item.title_key);
            if let Some(error) = item.error {
                lines.push(format!(
                    "{title}: {}",
                    self.loc.fmt("privacy.error", &[("message", &error)])
                ));
                continue;
            }
            lines.push(format!(
                "{title}: +{} / ={} / !{}",
                item.applied,
                item.skipped,
                item.failed.len()
            ));
            for failure in item.failed {
                lines.push(format!("  • {failure}"));
            }
            if item.freed_bytes > 0 {
                lines.push(format!(
                    "  {}",
                    self.loc
                        .fmt("cleaner.freed", &[("size", &format_size(item.freed_bytes))])
                ));
            }
            if item.skipped_files > 0 {
                lines.push(format!(
                    "  {}",
                    self.loc.fmt(
                        "cleaner.skipped",
                        &[("count", &item.skipped_files.to_string())]
                    )
                ));
            }
            if item.cancelled {
                cancelled = true;
            }
        }
        if cancelled {
            lines.push(self.loc.t("job.cancelled"));
        }
        self.catalog.report = Some(lines);
        self.refresh_catalog_states();
    }

    fn draw_catalog(&mut self, ui: &mut egui::Ui) {
        self.poll_job();
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
            if self.catalog.section == "cleaner" && ui.button(loc.t("cleaner.measure")).clicked() {
                action = Some(CatalogAction::Measure);
            }
            if ui.button(loc.t("privacy.apply")).clicked() {
                action = Some(CatalogAction::BuildPreview);
            }
            if self.catalog.section != "cleaner" && ui.button(loc.t("privacy.revert")).clicked() {
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
        if let Some(job) = &self.job {
            let progress = job
                .progress
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let fraction = if progress.total == 0 {
                0.0
            } else {
                progress.index as f32 / progress.total as f32
            };
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(280.0)
                        .text(loc.fmt(
                            "job.progress",
                            &[
                                ("done", &progress.index.to_string()),
                                ("total", &progress.total.to_string()),
                            ],
                        )),
                );
                if ui.button(loc.t("job.cancel")).clicked() {
                    job.cancel.store(true, Ordering::SeqCst);
                }
            });
            if !progress.action.is_empty() {
                ui.label(
                    egui::RichText::new(&progress.action)
                        .size(11.0)
                        .color(egui::Color32::from_gray(120)),
                );
            }
            ui.add_space(6.0);
        }

        for (index, row) in rows.iter().enumerate() {
            ui.horizontal(|ui| {
                if selected.len() > index {
                    ui.checkbox(&mut selected[index], "");
                }
                ui.label(egui::RichText::new(&row.title).size(14.0).strong());
                if let Some(Some(bytes)) = self.catalog.sizes.get(index) {
                    ui.label(
                        egui::RichText::new(
                            loc.fmt("cleaner.size", &[("size", &format_size(*bytes))]),
                        )
                        .size(11.0)
                        .color(egui::Color32::from_gray(120)),
                    );
                }
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
            Some(CatalogAction::Measure) => self.measure_cleaner(),
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
        if self.catalog.section != "cleaner" {
            self.catalog.sizes.clear();
        }
    }

    /// Measures the cleanup targets on demand: walking system folders takes time.
    fn measure_cleaner(&mut self) {
        self.catalog.sizes = self
            .catalog
            .tweaks
            .iter()
            .map(|tweak| {
                let paths: Vec<std::path::PathBuf> = tweak
                    .actions
                    .iter()
                    .flat_map(|action| match action {
                        Action::CleanPaths { paths } => paths.clone(),
                        _ => Vec::new(),
                    })
                    .collect();
                if paths.is_empty() {
                    None
                } else {
                    self.files.measure(&paths).ok()
                }
            })
            .collect();
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
        if self.job.is_some() {
            self.catalog.report = Some(vec![self.loc.t("job.busy")]);
            return;
        }
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
        let mut jobs: Vec<(TweakDefinition, Plan)> = Vec::new();
        let mut lines = Vec::new();
        {
            let journal = self.journal.as_ref().expect("checked above");
            let engine = Engine::new(
                &self.registry,
                &self.files,
                &self.services,
                &self.processes,
                &self.scheduler,
                journal,
            );
            for id in &ids {
                let Some(tweak) = self.catalog.tweaks.iter().find(|tweak| &tweak.id == id) else {
                    continue;
                };
                let title = self.loc.t(&tweak.title_key);
                match engine.plan(tweak) {
                    Ok(plan) if plan.is_empty() => {
                        lines.push(format!("{title}: {}", self.loc.t("privacy.applied")));
                    }
                    Ok(plan) => jobs.push((tweak.clone(), plan)),
                    Err(error) => lines.push(format!(
                        "{title}: {}",
                        self.loc
                            .fmt("privacy.error", &[("message", &error.to_string())])
                    )),
                }
            }
        }
        self.catalog.preview.clear();
        if jobs.is_empty() {
            self.catalog.report = Some(lines);
            return;
        }
        self.start_apply_jobs(jobs);
        self.catalog.report = None;
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

/// One application tile on the applications screen.
struct AppRow {
    title: String,
    state: String,
    color: egui::Color32,
    installed: bool,
    removed: bool,
}

/// Localized state word and its color for one application tile.
fn app_state_text(state: &TweakState) -> (&'static str, egui::Color32) {
    match state {
        TweakState::Applied => ("apps.removed", egui::Color32::from_gray(120)),
        TweakState::NotApplied => ("apps.installed", egui::Color32::from_rgb(21, 128, 61)),
        TweakState::Unavailable { .. } => ("apps.unavailable", egui::Color32::from_rgb(180, 83, 9)),
    }
}

/// Indices of installed (`installed = true`) or removed applications.
fn apps_with_state(states: &[TweakState], installed: bool) -> Vec<usize> {
    states
        .iter()
        .enumerate()
        .filter(|(_, state)| {
            if installed {
                matches!(state, TweakState::NotApplied)
            } else {
                matches!(state, TweakState::Applied)
            }
        })
        .map(|(index, _)| index)
        .collect()
}

/// Draws a square tile icon with the first letter of the application name.
fn draw_app_icon(ui: &mut egui::Ui, letter: &str, color: egui::Color32) {
    let size = egui::vec2(30.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
    );
}

fn report_message(report: &ApplyReport) -> String {
    report
        .failed
        .first()
        .map(|failure| failure.error.clone())
        .or_else(|| report.error.clone())
        .unwrap_or_else(|| "apply failed".to_owned())
}

fn revert_message(report: &RevertReport) -> String {
    report
        .failed
        .first()
        .map(|failure| failure.error.clone())
        .or_else(|| report.error.clone())
        .unwrap_or_else(|| "revert failed".to_owned())
}

/// Human-readable byte size for the cleanup preview.
fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
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
    fn app_states_are_split_into_installed_and_removed() {
        let states = vec![
            TweakState::NotApplied,
            TweakState::Applied,
            TweakState::Unavailable {
                reason: "no package manager".to_owned(),
            },
            TweakState::Applied,
        ];
        assert_eq!(apps_with_state(&states, true), vec![0]);
        assert_eq!(apps_with_state(&states, false), vec![1, 3]);
        assert_eq!(app_state_text(&TweakState::Applied).0, "apps.removed");
        assert_eq!(app_state_text(&TweakState::NotApplied).0, "apps.installed");
        assert_eq!(
            app_state_text(&TweakState::Unavailable {
                reason: String::new()
            })
            .0,
            "apps.unavailable"
        );
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
