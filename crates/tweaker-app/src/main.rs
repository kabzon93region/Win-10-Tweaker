// Enable the "windows" subsystem in release builds: no console window behind the
// GUI. Debug builds keep the console so logs and the self-test are visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod selftest;

use std::path::PathBuf;
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_FAIL: u8 = 1;
const EXIT_USAGE: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Gui,
    SelfTest,
    Smoke,
    Screenshot(PathBuf),
    Help,
    Version,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (mode, section) = match parse_args(arguments.clone()) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            eprintln!();
            print_help();
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match mode {
        Mode::Help => {
            print_help();
            ExitCode::from(EXIT_OK)
        }
        Mode::Version => {
            println!("{}", tweaker_core::product::version_string());
            ExitCode::from(EXIT_OK)
        }
        Mode::SelfTest => to_exit(selftest::run()),
        Mode::Smoke => to_exit(app::run(app::RunMode::Smoke, section, &arguments)),
        Mode::Screenshot(path) => to_exit(app::run(
            app::RunMode::Screenshot(path),
            section,
            &arguments,
        )),
        Mode::Gui => to_exit(app::run(app::RunMode::Gui, section, &arguments)),
    }
}

fn to_exit(result: Result<u8, String>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_FAIL)
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<(Mode, Option<String>), String> {
    let mut mode = Mode::Gui;
    let mut section = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--selftest" => mode = Mode::SelfTest,
            "--smoke" => mode = Mode::Smoke,
            "--section" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--section needs a section id".to_owned())?;
                section = Some(value.clone());
            }
            "--screenshot" => {
                index += 1;
                let path = args
                    .get(index)
                    .ok_or_else(|| "--screenshot needs an output file path".to_owned())?;
                mode = Mode::Screenshot(PathBuf::from(path));
            }
            "--help" | "-h" | "/?" => return Ok((Mode::Help, section)),
            "--version" | "-V" => return Ok((Mode::Version, section)),
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }
    Ok((mode, section))
}

fn print_help() {
    println!("WinTweaker — Windows tweaking, cleanup and diagnostics");
    println!();
    println!("Usage: wintweaker [option]");
    println!();
    println!("  (no option)             open the main window");
    println!("  --selftest              run built-in checks and exit, no window");
    println!("  --smoke                 open the window for 2.5 s to verify rendering, then exit");
    println!("  --screenshot <file>     render the window, save a PNG, then exit");
    println!("  --section <id>          open a section by id (privacy, services, ...)");
    println!("  --version               print the version and exit");
    println!("  --help                  print this help");
    println!();
    println!("Debug builds run as the current user; the release build asks for");
    println!("administrator rights through its embedded manifest.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_mode() {
        assert_eq!(parse_args(vec![]).expect("default"), (Mode::Gui, None));
    }

    #[test]
    fn parses_selftest_and_smoke() {
        assert_eq!(
            parse_args(vec!["--selftest".to_owned()]).expect("selftest"),
            (Mode::SelfTest, None)
        );
        assert_eq!(
            parse_args(vec!["--smoke".to_owned()]).expect("smoke"),
            (Mode::Smoke, None)
        );
    }

    #[test]
    fn parses_screenshot_with_path() {
        let (mode, section) =
            parse_args(vec!["--screenshot".to_owned(), "shot.png".to_owned()]).expect("screenshot");
        assert_eq!(mode, Mode::Screenshot(PathBuf::from("shot.png")));
        assert_eq!(section, None);
    }

    #[test]
    fn parses_section_for_a_screenshot() {
        let (mode, section) = parse_args(vec![
            "--section".to_owned(),
            "services".to_owned(),
            "--screenshot".to_owned(),
            "shot.png".to_owned(),
        ])
        .expect("parsed");
        assert_eq!(mode, Mode::Screenshot(PathBuf::from("shot.png")));
        assert_eq!(section.as_deref(), Some("services"));
    }

    #[test]
    fn section_without_value_is_an_error() {
        assert!(parse_args(vec!["--section".to_owned()]).is_err());
    }

    #[test]
    fn screenshot_without_path_is_an_error() {
        assert!(parse_args(vec!["--screenshot".to_owned()]).is_err());
    }

    #[test]
    fn unknown_argument_is_an_error() {
        assert!(parse_args(vec!["--wat".to_owned()]).is_err());
    }

    #[test]
    fn help_wins_over_other_flags() {
        assert_eq!(
            parse_args(vec!["--selftest".to_owned(), "--help".to_owned()]).expect("help"),
            (Mode::Help, None)
        );
    }
}
