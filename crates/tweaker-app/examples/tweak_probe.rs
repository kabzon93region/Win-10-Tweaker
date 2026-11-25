//! Developer probe for live checks of the catalog outside the GUI.
//!
//! Usage: `tweak_probe <ID> [detect|apply|revert]`
//!
//! It is an example binary, not part of the shipped executable: the release file
//! is `wintweaker.exe`, and this helper exists so an engineer can inspect and
//! exercise one catalog item on a test machine with the same engine the UI uses.

use std::process::ExitCode;

use tweaker_core::catalog;
use tweaker_core::engine::Engine;
use tweaker_core::journal::Journal;
use tweaker_core::paths::ProductPaths;
use tweaker_windows::files::WindowsFileSystem;
use tweaker_windows::process::SystemProcessRunner;
use tweaker_windows::registry::WindowsRegistry;
use tweaker_windows::scheduler::WindowsScheduler;
use tweaker_windows::services::WindowsServices;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(id) = args.first().cloned() else {
        usage();
        return ExitCode::from(2);
    };
    let command = args.get(1).cloned().unwrap_or_else(|| "detect".to_owned());
    match run(&id, &command) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(id: &str, command: &str) -> Result<u8, String> {
    let paths = ProductPaths::for_current_user().map_err(|error| error.to_string())?;
    paths
        .ensure_runtime_dirs()
        .map_err(|error| format!("cannot create product folders: {error}"))?;
    let journal = Journal::new(paths.journal())
        .map_err(|error| format!("cannot open the journal: {error}"))?;
    let registry = WindowsRegistry;
    let files = WindowsFileSystem;
    let services = WindowsServices;
    let processes = SystemProcessRunner;
    let scheduler = WindowsScheduler;
    let engine = Engine::new(
        &registry, &files, &services, &processes, &scheduler, &journal,
    );

    let tweaks = catalog::load_privacy().map_err(|error| error.to_string())?;
    let tweak = tweaks
        .iter()
        .find(|tweak| tweak.id == id)
        .ok_or_else(|| format!("no such tweak: {id}"))?;

    match command {
        "detect" => {
            println!("{id}: {:?}", engine.detect(tweak));
            Ok(0)
        }
        "apply" => {
            let report = engine.apply_tweak(tweak);
            println!(
                "{id}: applied={} skipped={} failed={} error={:?}",
                report.applied.len(),
                report.skipped.len(),
                report.failed.len(),
                report.error
            );
            for failure in &report.failed {
                eprintln!("failed: {}: {}", failure.action.describe(), failure.error);
            }
            Ok(u8::from(!report.success()))
        }
        "revert" => {
            let report = engine.revert(id);
            println!(
                "{id}: reverted={} failed={} nothing_to_revert={}",
                report.reverted.len(),
                report.failed.len(),
                report.nothing_to_revert
            );
            for failure in &report.failed {
                eprintln!("failed: {}: {}", failure.action.describe(), failure.error);
            }
            Ok(u8::from(!report.success()))
        }
        other => Err(format!(
            "unknown command: {other} (expected detect, apply or revert)"
        )),
    }
}

fn usage() {
    eprintln!("usage: tweak_probe <ID> [detect|apply|revert]");
}
