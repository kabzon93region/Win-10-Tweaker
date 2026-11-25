//! `--selftest`: checks the skeleton without opening a window and without
//! touching system settings. Exit code 0 means every hard check passed.
//!
//! The self-test is what automated builds and the acceptance log use instead of
//! clicking through the UI: paths, logger, both localization catalogs, the
//! section catalog, elevation state and OS information.

use tweaker_core::Lang;
use tweaker_core::logging::{self, LoggerConfig};
use tweaker_core::paths::ProductPaths;
use tweaker_core::product::version_string;
use tweaker_core::sections::{Section, load_sections};

use crate::i18n::{self, Localizer};

/// Runs all checks and prints a human-readable report.
pub fn run() -> Result<u8, String> {
    println!("== {} ==", version_string());
    println!("{}", "-".repeat(64));
    let mut problems: Vec<String> = Vec::new();

    let paths = ProductPaths::for_current_user().map_err(|error| format!("paths: {error}"))?;
    if let Err(error) = paths.ensure_runtime_dirs() {
        println!("FAIL folders: {error}");
        return Ok(1);
    }
    let logger = logging::init(LoggerConfig::new(paths.logs()))
        .map_err(|error| format!("cannot start logger: {error}"))?;
    logger.info("selftest: started");

    let lang = i18n::detect_lang();
    let loc = match Localizer::try_load(lang) {
        Ok(loc) => loc,
        Err(error) => {
            problems.push(format!("localization: {error}"));
            Localizer::load(lang)
        }
    };

    let root = paths.root().display().to_string();
    println!("{}", loc.fmt("selftest.paths", &[("path", &root)]));
    let log_file = logging::log_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "?".to_owned());
    println!("{}", loc.fmt("selftest.log", &[("path", &log_file)]));
    println!(
        "{}",
        loc.fmt("selftest.language", &[("language", lang.native_name())])
    );

    let ru_keys = loc.catalog(Lang::Ru).len().to_string();
    let en_keys = loc.catalog(Lang::En).len().to_string();
    println!(
        "{}",
        loc.fmt("selftest.keys", &[("ru", &ru_keys), ("en", &en_keys)])
    );
    if loc.catalog(Lang::Ru).len() != loc.catalog(Lang::En).len() {
        problems.push("localization catalogs have different key counts".to_owned());
    }

    match load_sections() {
        Ok(sections) => {
            let count = sections.len().to_string();
            println!("{}", loc.fmt("selftest.sections", &[("count", &count)]));
            let missing = missing_section_texts(&loc, &sections);
            if !missing.is_empty() {
                problems.push(format!("missing section texts: {}", missing.join(", ")));
            }
        }
        Err(error) => problems.push(format!("section catalog: {error}")),
    }

    let admin = tweaker_windows::elevation::is_elevated();
    let admin_text = if admin {
        loc.t("selftest.admin_yes")
    } else {
        loc.t("selftest.admin_no")
    };
    println!("{}", loc.fmt("selftest.admin", &[("value", &admin_text)]));

    let os = tweaker_windows::os::os_info().display();
    println!("{}", loc.fmt("selftest.os", &[("value", &os)]));

    logger.info("selftest: log write check");
    match logging::log_path() {
        Some(path) => {
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            if size == 0 {
                problems.push(format!("log file is empty: {}", path.display()));
            }
        }
        None => problems.push("log file path is unknown".to_owned()),
    }

    if problems.is_empty() {
        logger.info("selftest: passed");
        println!("{}", loc.t("selftest.ok"));
        Ok(0)
    } else {
        for problem in &problems {
            logger.error(&format!("selftest: {problem}"));
            println!(" - {problem}");
        }
        println!(
            "{}",
            loc.fmt("selftest.fail", &[("reason", &problems.join("; "))])
        );
        Ok(1)
    }
}

fn missing_section_texts(loc: &Localizer, sections: &[Section]) -> Vec<String> {
    let mut missing = Vec::new();
    for section in sections {
        for key in [&section.title_key, &section.hint_key] {
            for lang in Lang::ALL {
                if !loc.catalog(lang).contains_key(key) {
                    missing.push(format!("{key} ({})", lang.code()));
                }
            }
        }
    }
    missing
}
