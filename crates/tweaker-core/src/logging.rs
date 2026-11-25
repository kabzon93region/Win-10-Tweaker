//! Technical log: INFO / WARN / ERROR lines in `%LocalAppData%\WinTweaker\logs`.
//!
//! Rules from the clean-room package:
//! * every failure is visible in the log and in the report (no silent errors);
//! * secrets never reach the log;
//! * the file is rotated by size, so it cannot grow without limit.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Local;

/// Log severity. Ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Normal progress: started, finished, counts.
    Info,
    /// Something was skipped or looks unexpected, work continues.
    Warn,
    /// An operation failed; the message must say what and why.
    Error,
}

impl Level {
    /// Fixed-width tag written into every line.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// Settings for the file logger.
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    /// Directory for log files (created if missing).
    pub dir: PathBuf,
    /// Rotate when the current file would grow past this many bytes.
    pub max_bytes: u64,
    /// How many rotated files to keep besides the current one.
    pub keep_files: usize,
    /// Also echo lines to the console. Useful for `--selftest` and development.
    pub echo: bool,
}

impl LoggerConfig {
    /// Default policy: 1 MiB per file, 5 rotated files, no console echo.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_bytes: 1024 * 1024,
            keep_files: 5,
            echo: false,
        }
    }

    /// Enables or disables console echo.
    pub fn with_echo(mut self, echo: bool) -> Self {
        self.echo = echo;
        self
    }

    /// Overrides rotation limits (tests use tiny values).
    pub fn with_rotation(mut self, max_bytes: u64, keep_files: usize) -> Self {
        self.max_bytes = max_bytes.max(1);
        self.keep_files = keep_files;
        self
    }
}

/// Name of the active log file inside the log directory.
pub const LOG_FILE_NAME: &str = "wintweaker.log";

struct LoggerState {
    file: Option<File>,
    size: u64,
    /// When rotation failed (file locked by another process), do not retry before.
    rotation_retry_at: Option<Instant>,
    /// Lines written while the file was unavailable; replayed after a reopen.
    pending: VecDeque<String>,
}

/// After a failed rotation the logger keeps writing to the current file and tries
/// again later, so a locked file does not turn into a log storm.
const ROTATION_RETRY_DELAY: Duration = Duration::from_secs(60);

/// Upper bound of buffered lines while the log file is unavailable. Enough to
/// survive a short problem without letting the program grow without limit.
const MAX_PENDING_LINES: usize = 200;

/// Thread-safe size-rotating file logger.
pub struct Logger {
    dir: PathBuf,
    max_bytes: u64,
    keep_files: usize,
    echo: bool,
    state: Mutex<LoggerState>,
}

impl Logger {
    /// Creates the logger and opens (or reopens) the active log file.
    pub fn new(config: LoggerConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.dir)?;
        let path = config.dir.join(LOG_FILE_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        let logger = Self {
            dir: config.dir,
            max_bytes: config.max_bytes,
            keep_files: config.keep_files,
            echo: config.echo,
            state: Mutex::new(LoggerState {
                file: Some(file),
                size,
                rotation_retry_at: None,
                pending: VecDeque::new(),
            }),
        };
        if size >= logger.max_bytes {
            let mut state = logger.lock();
            if logger.rotate(&mut state).is_err() {
                // Keep logging into the current file and retry later. Fail the
                // constructor only if even reopening the file is impossible.
                state.rotation_retry_at = Some(Instant::now() + ROTATION_RETRY_DELAY);
                if state.file.is_none() {
                    return Err(io::Error::other(
                        "cannot reopen the log file after an initial rotation",
                    ));
                }
            }
        }
        Ok(logger)
    }

    /// Full path of the active log file.
    pub fn path(&self) -> PathBuf {
        self.dir.join(LOG_FILE_NAME)
    }

    /// Writes one line. Never panics: logging must not take the program down.
    pub fn log(&self, level: Level, message: &str) {
        let line = format!(
            "{} [{}] {}\r\n",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            level.as_str(),
            message
        );
        self.echo(level, &line);
        let mut state = self.lock();
        let bytes = line.len() as u64;
        let now = Instant::now();

        if state.file.is_none() {
            // The file disappeared (deleted folder, failed rotation): try to reopen
            // on every line and replay what was buffered meanwhile.
            if self.reopen(&mut state).is_ok() {
                self.flush_pending(&mut state);
            }
        }

        let may_rotate = state
            .rotation_retry_at
            .is_none_or(|retry_at| now >= retry_at);
        if state.file.is_some()
            && state.size + bytes > self.max_bytes
            && may_rotate
            && self.rotate(&mut state).is_err()
        {
            state.rotation_retry_at = Some(now + ROTATION_RETRY_DELAY);
        }

        if !self.write_line(&mut state, &line) {
            // Nothing is silently dropped: the line waits for the file to come back.
            if state.pending.len() >= MAX_PENDING_LINES {
                state.pending.pop_front();
            }
            state.pending.push_back(line);
            if self.echo {
                let _ = writeln!(
                    io::stderr(),
                    "[WARN] log file is unavailable, line buffered"
                );
            }
        }
    }

    /// Appends one line to the open file; reports whether it reached the disk.
    fn write_line(&self, state: &mut LoggerState, line: &str) -> bool {
        let Some(file) = state.file.as_mut() else {
            return false;
        };
        if file.write_all(line.as_bytes()).is_ok() && file.flush().is_ok() {
            state.size += line.len() as u64;
            true
        } else {
            false
        }
    }

    /// Replays lines buffered while the file was unavailable.
    fn flush_pending(&self, state: &mut LoggerState) {
        while let Some(line) = state.pending.pop_front() {
            if !self.write_line(state, &line) {
                state.pending.push_front(line);
                break;
            }
        }
    }

    /// Test hook: pretends the open file handle is gone.
    #[cfg(test)]
    fn simulate_missing_file(&self) {
        let mut state = self.lock();
        state.file = None;
        state.size = 0;
    }

    /// Convenience wrappers.
    pub fn info(&self, message: &str) {
        self.log(Level::Info, message);
    }

    /// Writes a WARN line.
    pub fn warn(&self, message: &str) {
        self.log(Level::Warn, message);
    }

    /// Writes an ERROR line.
    pub fn error(&self, message: &str) {
        self.log(Level::Error, message);
    }

    fn echo(&self, level: Level, line: &str) {
        if !self.echo {
            return;
        }
        if level == Level::Info {
            let _ = io::stdout().write_all(line.as_bytes());
            let _ = io::stdout().flush();
        } else {
            let _ = io::stderr().write_all(line.as_bytes());
            let _ = io::stderr().flush();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LoggerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Shifts `wintweaker.log` -> `.1`, `.1` -> `.2`, ... and drops the oldest.
    ///
    /// A failure does not lose lines: the caller reopens the current file and
    /// keeps writing, then retries later.
    fn rotate(&self, state: &mut LoggerState) -> io::Result<()> {
        if let Some(file) = state.file.take() {
            drop(file);
        }
        let shift_result = self.shift_files();
        match self.reopen(state) {
            Ok(()) => {
                self.flush_pending(state);
                shift_result
            }
            Err(error) => {
                state.file = None;
                state.size = 0;
                Err(shift_result.err().unwrap_or(error))
            }
        }
    }

    /// Opens the current log file for appending and remembers its size.
    fn reopen(&self, state: &mut LoggerState) -> io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(LOG_FILE_NAME))?;
        state.size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        state.file = Some(file);
        Ok(())
    }

    fn shift_files(&self) -> io::Result<()> {
        let current = self.dir.join(LOG_FILE_NAME);
        if self.keep_files == 0 {
            if current.exists() {
                fs::remove_file(&current)?;
            }
            return Ok(());
        }
        let oldest = self
            .dir
            .join(format!("{LOG_FILE_NAME}.{}", self.keep_files));
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for index in (1..self.keep_files).rev() {
            let from = self.dir.join(format!("{LOG_FILE_NAME}.{index}"));
            let to = self.dir.join(format!("{LOG_FILE_NAME}.{}", index + 1));
            if from.exists() {
                fs::rename(&from, &to)?;
            }
        }
        if current.exists() {
            fs::rename(&current, self.dir.join(format!("{LOG_FILE_NAME}.1")))?;
        }
        Ok(())
    }
}

static GLOBAL: OnceLock<Logger> = OnceLock::new();

/// Initializes the process-wide logger once.
///
/// Later calls return the already installed logger; a second initialization is
/// ignored on purpose (two logger instances would fight over the same file).
pub fn init(config: LoggerConfig) -> io::Result<&'static Logger> {
    if let Some(existing) = GLOBAL.get() {
        return Ok(existing);
    }
    let logger = Logger::new(config)?;
    let _ = GLOBAL.set(logger);
    Ok(GLOBAL.get().expect("logger was set above"))
}

/// The process-wide logger, if [`init`] has run.
pub fn global() -> Option<&'static Logger> {
    GLOBAL.get()
}

/// Path of the active log file, if the logger is initialized.
pub fn log_path() -> Option<PathBuf> {
    global().map(Logger::path)
}

/// Logs an INFO line through the global logger; no-op before initialization.
pub fn info(message: &str) {
    if let Some(logger) = global() {
        logger.info(message);
    }
}

/// Logs a WARN line through the global logger; no-op before initialization.
pub fn warn(message: &str) {
    if let Some(logger) = global() {
        logger.warn(message);
    }
}

/// Logs an ERROR line through the global logger; no-op before initialization.
pub fn error(message: &str) {
    if let Some(logger) = global() {
        logger.error(message);
    }
}

/// Masks a secret for logs and reports: keeps only the last four characters.
///
/// `"abcdefgh"` becomes `"****efgh"`; short values are fully masked.
pub fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    let visible: String = chars[chars.len() - 4..].iter().collect();
    format!("{}{visible}", "*".repeat(chars.len() - 4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_dir;

    fn read(path: &std::path::Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn writes_levels_and_timestamps() {
        let dir = temp_dir("levels");
        let logger = Logger::new(LoggerConfig::new(&dir)).expect("logger");
        logger.info("started");
        logger.warn("skipped");
        logger.error("failed: rc=5");
        let text = read(&logger.path());
        assert!(text.contains("[INFO] started"));
        assert!(text.contains("[WARN] skipped"));
        assert!(text.contains("[ERROR] failed: rc=5"));
        assert!(text.lines().count() >= 3);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn rotates_by_size_and_keeps_old_files() {
        let dir = temp_dir("rotate");
        let logger = Logger::new(LoggerConfig::new(&dir).with_rotation(200, 2)).expect("logger");
        for index in 0..40 {
            logger.info(&format!("line {index} with some payload to grow the file"));
        }
        assert!(dir.join(LOG_FILE_NAME).is_file());
        assert!(dir.join(format!("{LOG_FILE_NAME}.1")).is_file());
        assert!(dir.join(format!("{LOG_FILE_NAME}.2")).is_file());
        assert!(!dir.join(format!("{LOG_FILE_NAME}.3")).exists());
        let current = read(&dir.join(LOG_FILE_NAME));
        assert!(!current.is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn appends_to_existing_file_after_restart() {
        let dir = temp_dir("append");
        {
            let logger = Logger::new(LoggerConfig::new(&dir)).expect("first");
            logger.info("first run");
        }
        let logger = Logger::new(LoggerConfig::new(&dir)).expect("second");
        logger.info("second run");
        let text = read(&logger.path());
        assert!(text.contains("first run"));
        assert!(text.contains("second run"));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn rotation_without_history_keeps_only_the_current_file() {
        let dir = temp_dir("rotate-zero");
        let logger = Logger::new(LoggerConfig::new(&dir).with_rotation(120, 0)).expect("logger");
        for index in 0..20 {
            logger.info(&format!("line {index} with payload"));
        }
        assert!(dir.join(LOG_FILE_NAME).is_file());
        assert!(!dir.join(format!("{LOG_FILE_NAME}.1")).exists());
        let size = fs::metadata(dir.join(LOG_FILE_NAME)).expect("meta").len();
        assert!(
            size <= 120,
            "current file must stay under the limit, got {size}"
        );
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn rotation_failure_keeps_writing_to_the_current_file() {
        let dir = temp_dir("rotate-fail");
        let logger = Logger::new(LoggerConfig::new(&dir).with_rotation(120, 1)).expect("logger");
        // A directory in place of the rotated file makes the rename fail.
        let blocker = dir.join(format!("{LOG_FILE_NAME}.1"));
        fs::create_dir_all(&blocker).expect("create blocker");
        for index in 0..20 {
            logger.info(&format!("line {index} with payload"));
        }
        let text = read(&logger.path());
        assert!(
            text.contains("line 19"),
            "logger must keep writing after a failed rotation"
        );
        fs::remove_dir_all(&blocker).expect("remove blocker");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn initial_rotation_failure_does_not_drop_the_logger() {
        let dir = temp_dir("rotate-fail-new");
        fs::create_dir_all(&dir).expect("create dir");
        // A non-empty log file bigger than the limit, so the constructor itself
        // tries to rotate it (the interesting branch).
        fs::write(dir.join(LOG_FILE_NAME), b"0123456789").expect("seed log");
        let blocker = dir.join(format!("{LOG_FILE_NAME}.1"));
        fs::create_dir_all(&blocker).expect("create blocker");
        let logger = Logger::new(LoggerConfig::new(&dir).with_rotation(1, 1))
            .expect("logger must survive a failed initial rotation");
        // The retry is scheduled, so the next line does not hammer the disk.
        assert!(logger.lock().rotation_retry_at.is_some());
        logger.info("after failed rotation");
        assert!(read(&logger.path()).contains("after failed rotation"));
        fs::remove_dir_all(&blocker).expect("remove blocker");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn lines_are_buffered_while_the_file_is_unavailable() {
        let dir = temp_dir("buffer");
        let logger = Logger::new(LoggerConfig::new(&dir)).expect("logger");
        logger.simulate_missing_file();
        // The log path is occupied by a directory, so reopening must fail.
        fs::remove_file(dir.join(LOG_FILE_NAME)).expect("remove log file");
        fs::create_dir(dir.join(LOG_FILE_NAME)).expect("occupy log path");
        logger.info("buffered line");
        assert_eq!(logger.lock().pending.len(), 1);
        fs::remove_dir(dir.join(LOG_FILE_NAME)).expect("free log path");
        logger.info("after restore");
        let text = read(&logger.path());
        assert!(
            text.contains("buffered line"),
            "buffered line must be replayed after the file is back"
        );
        assert!(text.contains("after restore"));
        assert!(logger.lock().pending.is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn masks_secrets() {
        assert_eq!(mask_secret("abcdefgh"), "****efgh");
        assert_eq!(mask_secret("abc"), "***");
        assert_eq!(mask_secret(""), "");
        assert_eq!(mask_secret("abcd"), "****");
        assert_eq!(mask_secret("0123456789abcdef"), "************cdef");
    }

    #[test]
    fn level_tags_are_fixed_width() {
        assert_eq!(Level::Info.as_str(), "INFO");
        assert_eq!(Level::Warn.as_str(), "WARN");
        assert_eq!(Level::Error.as_str(), "ERROR");
        assert!(Level::Info < Level::Warn && Level::Warn < Level::Error);
    }
}
