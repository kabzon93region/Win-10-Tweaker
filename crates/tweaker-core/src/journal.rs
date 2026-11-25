//! Change journal: append-only JSONL, two-phase records, size-based rotation.
//!
//! Every applied action is written twice: `started` before the change and
//! `applied`/`failed` after it. If the process dies between the two, the next
//! start can see the unfinished operation and offer to revert it. Reverts are
//! recorded too, so an operation is never reverted twice.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::action::{Action, RegistryValue};
use crate::adapters::{AppxPackage, RegistryTree, ServiceState, TaskInfo};
use crate::logging;

/// Name of the active journal file.
pub const JOURNAL_FILE_NAME: &str = "journal.jsonl";

const DEFAULT_MAX_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_KEEP_FILES: usize = 5;

/// State of one object before an action, enough to restore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Snapshot {
    /// Registry value; `None` means the value did not exist.
    Registry(Option<RegistryValue>),
    /// Whole registry key tree; `None` means the key did not exist.
    RegistryKey(Option<RegistryTree>),
    /// Text file contents; `None` means the file did not exist.
    TextFile(Option<String>),
    /// Binary file contents; `None` means the file did not exist.
    BinaryFile(Option<Vec<u8>>),
    /// Service state.
    Service(Option<ServiceState>),
    /// Scheduled tasks and their enabled state.
    Tasks(Vec<TaskInfo>),
    /// Store package; `None` means it was not installed.
    Appx(Option<AppxPackage>),
    /// Nothing to restore (irreversible action).
    None,
}

/// Lifecycle phase of one journal record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The action is about to run; the state before is captured.
    Started,
    /// The action ran and the result was verified.
    Applied,
    /// The action failed or the verification did not match.
    Failed,
    /// A previous operation was reverted.
    Reverted,
}

/// One line of the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalRecord {
    /// RFC 3339 timestamp in UTC.
    pub ts: String,
    /// Operation id, shared by the `started` and the `applied`/`failed` record.
    pub op_id: u64,
    /// Tweak id.
    pub tweak_id: String,
    /// Action that was executed.
    pub action: Action,
    /// State before the action, when known.
    pub before: Option<Snapshot>,
    /// State after the action, when verified.
    pub after: Option<Snapshot>,
    /// Phase of this record.
    pub phase: Phase,
    /// Failure text, for `failed` records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// For `reverted` records: the operation that was reverted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverts_op: Option<u64>,
}

/// A successfully applied operation that has not been reverted yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedOp {
    /// Operation id.
    pub op_id: u64,
    /// Tweak id.
    pub tweak_id: String,
    /// Executed action.
    pub action: Action,
    /// State before the action.
    pub before: Snapshot,
}

struct JournalInner {
    file: Option<File>,
    size: u64,
    next_op: u64,
}

/// Thread-safe append-only journal.
pub struct Journal {
    dir: PathBuf,
    max_bytes: u64,
    keep_files: usize,
    inner: Mutex<JournalInner>,
}

impl Journal {
    /// Opens the journal in the given directory with the default limits.
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::with_limits(dir, DEFAULT_MAX_BYTES, DEFAULT_KEEP_FILES)
    }

    /// Opens the journal with explicit rotation limits.
    pub fn with_limits(
        dir: impl Into<PathBuf>,
        max_bytes: u64,
        keep_files: usize,
    ) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let path = dir.join(JOURNAL_FILE_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        let next_op = match read_records(&dir, keep_files.max(1)) {
            Ok(records) => records.iter().map(|record| record.op_id).max().unwrap_or(0) + 1,
            Err(error) => {
                logging::warn(&format!("journal: cannot scan existing records: {error}"));
                1
            }
        };
        Ok(Self {
            dir,
            max_bytes: max_bytes.max(4096),
            keep_files,
            inner: Mutex::new(JournalInner {
                file: Some(file),
                size,
                next_op,
            }),
        })
    }

    /// Path of the active journal file.
    pub fn path(&self) -> PathBuf {
        self.dir.join(JOURNAL_FILE_NAME)
    }

    /// Directory of the journal.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes the `started` record and returns the operation id.
    pub fn begin_apply(
        &self,
        tweak_id: &str,
        action: &Action,
        before: &Snapshot,
    ) -> io::Result<u64> {
        let mut inner = self.lock();
        let op_id = inner.next_op;
        inner.next_op += 1;
        let record = JournalRecord {
            ts: now(),
            op_id,
            tweak_id: tweak_id.to_owned(),
            action: action.clone(),
            before: Some(before.clone()),
            after: None,
            phase: Phase::Started,
            error: None,
            reverts_op: None,
        };
        self.append(&mut inner, &record)?;
        Ok(op_id)
    }

    /// Writes the final record of an operation.
    pub fn finish_apply(
        &self,
        op_id: u64,
        tweak_id: &str,
        action: &Action,
        before: &Snapshot,
        after: Option<Snapshot>,
        result: Result<(), String>,
    ) -> io::Result<()> {
        let mut inner = self.lock();
        let (phase, error) = match result {
            Ok(()) => (Phase::Applied, None),
            Err(message) => (Phase::Failed, Some(message)),
        };
        let record = JournalRecord {
            ts: now(),
            op_id,
            tweak_id: tweak_id.to_owned(),
            action: action.clone(),
            before: Some(before.clone()),
            after,
            phase,
            error,
            reverts_op: None,
        };
        self.append(&mut inner, &record)
    }

    /// Records that a previous operation was reverted.
    pub fn record_revert(
        &self,
        tweak_id: &str,
        action: &Action,
        reverts_op: u64,
    ) -> io::Result<u64> {
        let mut inner = self.lock();
        let op_id = inner.next_op;
        inner.next_op += 1;
        let record = JournalRecord {
            ts: now(),
            op_id,
            tweak_id: tweak_id.to_owned(),
            action: action.clone(),
            before: None,
            after: None,
            phase: Phase::Reverted,
            error: None,
            reverts_op: Some(reverts_op),
        };
        self.append(&mut inner, &record)?;
        Ok(op_id)
    }

    /// Reads every record, oldest file first.
    pub fn records(&self) -> io::Result<Vec<JournalRecord>> {
        read_records(&self.dir, self.keep_files.max(1))
    }

    /// Applied operations that have not been reverted, oldest first.
    pub fn applied_ops(&self, tweak_id: Option<&str>) -> io::Result<Vec<AppliedOp>> {
        let records = self.records()?;
        let mut started: HashMap<u64, Snapshot> = HashMap::new();
        let mut reverted: HashSet<u64> = HashSet::new();
        let mut ops = Vec::new();
        for record in &records {
            match record.phase {
                Phase::Started => {
                    started.insert(
                        record.op_id,
                        record.before.clone().unwrap_or(Snapshot::None),
                    );
                }
                Phase::Applied => {
                    if let Some(before) = started.get(&record.op_id)
                        && tweak_id.is_none_or(|id| record.tweak_id == id)
                    {
                        ops.push(AppliedOp {
                            op_id: record.op_id,
                            tweak_id: record.tweak_id.clone(),
                            action: record.action.clone(),
                            before: before.clone(),
                        });
                    }
                }
                Phase::Reverted => {
                    if let Some(original) = record.reverts_op {
                        reverted.insert(original);
                    }
                }
                Phase::Failed => {}
            }
        }
        ops.retain(|op| !reverted.contains(&op.op_id));
        Ok(ops)
    }

    /// Operations that started but never finished (for example after a crash).
    pub fn pending(&self) -> io::Result<Vec<JournalRecord>> {
        let records = self.records()?;
        let mut started: HashMap<u64, JournalRecord> = HashMap::new();
        let mut finished: HashSet<u64> = HashSet::new();
        for record in records {
            match record.phase {
                Phase::Started => {
                    started.insert(record.op_id, record);
                }
                Phase::Applied | Phase::Failed => {
                    finished.insert(record.op_id);
                }
                Phase::Reverted => {
                    if let Some(original) = record.reverts_op {
                        finished.insert(original);
                    }
                }
            }
        }
        let mut pending: Vec<JournalRecord> = started
            .into_iter()
            .filter(|(op_id, _)| !finished.contains(op_id))
            .map(|(_, record)| record)
            .collect();
        pending.sort_by_key(|record| record.op_id);
        Ok(pending)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, JournalInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn append(&self, inner: &mut JournalInner, record: &JournalRecord) -> io::Result<()> {
        let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
        line.push('\n');
        let bytes = line.len() as u64;
        if inner.size + bytes > self.max_bytes {
            // Rotation is best effort: a journal that cannot rotate must still
            // keep recording changes.
            if let Err(error) = self.rotate(inner) {
                logging::warn(&format!("journal: rotation failed: {error}"));
            }
        }
        let Some(file) = inner.file.as_mut() else {
            return Err(io::Error::other("journal file is not open"));
        };
        file.write_all(line.as_bytes())?;
        file.flush()?;
        inner.size += bytes;
        Ok(())
    }

    fn rotate(&self, inner: &mut JournalInner) -> io::Result<()> {
        if let Some(file) = inner.file.take() {
            drop(file);
        }
        let shift = self.shift_files();
        let reopened = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(JOURNAL_FILE_NAME));
        match reopened {
            Ok(file) => {
                inner.size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
                inner.file = Some(file);
            }
            Err(error) => {
                return Err(shift.err().unwrap_or(error));
            }
        }
        shift
    }

    fn shift_files(&self) -> io::Result<()> {
        if self.keep_files == 0 {
            let current = self.dir.join(JOURNAL_FILE_NAME);
            if current.exists() {
                fs::remove_file(&current)?;
            }
            return Ok(());
        }
        let oldest = self
            .dir
            .join(format!("{JOURNAL_FILE_NAME}.{}", self.keep_files));
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for index in (1..self.keep_files).rev() {
            let from = self.dir.join(format!("{JOURNAL_FILE_NAME}.{index}"));
            let to = self.dir.join(format!("{JOURNAL_FILE_NAME}.{}", index + 1));
            if from.exists() {
                fs::rename(&from, &to)?;
            }
        }
        let current = self.dir.join(JOURNAL_FILE_NAME);
        if current.exists() {
            fs::rename(&current, self.dir.join(format!("{JOURNAL_FILE_NAME}.1")))?;
        }
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn read_records(dir: &Path, keep_files: usize) -> io::Result<Vec<JournalRecord>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for index in (1..=keep_files).rev() {
        let path = dir.join(format!("{JOURNAL_FILE_NAME}.{index}"));
        if path.is_file() {
            files.push(path);
        }
    }
    let current = dir.join(JOURNAL_FILE_NAME);
    if current.is_file() {
        files.push(current);
    }
    let mut records = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file)?;
        for (line_no, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalRecord>(line) {
                Ok(record) => records.push(record),
                Err(error) => logging::warn(&format!(
                    "journal: {}:{} is not a valid record: {error}",
                    file.display(),
                    line_no + 1
                )),
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{RegistryKey, RegistryRoot};
    use crate::test_support::temp_dir;

    fn registry_action() -> Action {
        Action::RegistrySet {
            key: RegistryKey::new(RegistryRoot::Hkcu, "Software\\Test"),
            name: "Value".into(),
            value: RegistryValue::Dword(1),
        }
    }

    #[test]
    fn started_and_applied_form_one_operation() {
        let dir = temp_dir("journal-basic");
        let journal = Journal::new(&dir).expect("journal");
        let before = Snapshot::Registry(None);
        let op = journal
            .begin_apply("PRIV-01", &registry_action(), &before)
            .expect("begin");
        journal
            .finish_apply(op, "PRIV-01", &registry_action(), &before, None, Ok(()))
            .expect("finish");
        let ops = journal.applied_ops(Some("PRIV-01")).expect("ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].before, before);
        assert_eq!(ops[0].action, registry_action());
        assert!(journal.pending().expect("pending").is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn reverted_operation_is_not_reverted_twice() {
        let dir = temp_dir("journal-revert");
        let journal = Journal::new(&dir).expect("journal");
        let before = Snapshot::Registry(None);
        let action = registry_action();
        let op = journal
            .begin_apply("PRIV-01", &action, &before)
            .expect("begin");
        journal
            .finish_apply(op, "PRIV-01", &action, &before, None, Ok(()))
            .expect("finish");
        journal
            .record_revert("PRIV-01", &action, op)
            .expect("revert record");
        assert!(
            journal
                .applied_ops(Some("PRIV-01"))
                .expect("ops")
                .is_empty()
        );
        assert!(journal.applied_ops(None).expect("ops").is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn failed_operation_is_not_applied_and_not_pending() {
        let dir = temp_dir("journal-failed");
        let journal = Journal::new(&dir).expect("journal");
        let before = Snapshot::Registry(None);
        let op = journal
            .begin_apply("PRIV-01", &registry_action(), &before)
            .expect("begin");
        journal
            .finish_apply(
                op,
                "PRIV-01",
                &registry_action(),
                &before,
                None,
                Err("access denied".to_owned()),
            )
            .expect("finish");
        assert!(journal.applied_ops(None).expect("ops").is_empty());
        assert!(journal.pending().expect("pending").is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn unfinished_operation_is_pending() {
        let dir = temp_dir("journal-pending");
        let journal = Journal::new(&dir).expect("journal");
        let before = Snapshot::Registry(None);
        let op = journal
            .begin_apply("PRIV-01", &registry_action(), &before)
            .expect("begin");
        let pending = journal.pending().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].op_id, op);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn journal_survives_restart() {
        let dir = temp_dir("journal-restart");
        let before = Snapshot::Registry(None);
        let action = registry_action();
        {
            let journal = Journal::new(&dir).expect("journal");
            let op = journal
                .begin_apply("PRIV-01", &action, &before)
                .expect("begin");
            journal
                .finish_apply(op, "PRIV-01", &action, &before, None, Ok(()))
                .expect("finish");
        }
        let journal = Journal::new(&dir).expect("reopen");
        let ops = journal.applied_ops(Some("PRIV-01")).expect("ops");
        assert_eq!(ops.len(), 1);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn rotation_keeps_records_readable() {
        let dir = temp_dir("journal-rotate");
        let journal = Journal::with_limits(&dir, 4096, 2).expect("journal");
        let before = Snapshot::Registry(None);
        for _ in 0..200 {
            let action = registry_action();
            let op = journal
                .begin_apply("PRIV-01", &action, &before)
                .expect("begin");
            journal
                .finish_apply(op, "PRIV-01", &action, &before, None, Ok(()))
                .expect("finish");
        }
        assert!(dir.join(format!("{JOURNAL_FILE_NAME}.1")).is_file());
        let records = journal.records().expect("records");
        assert!(!records.is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = temp_dir("journal-corrupt");
        {
            let journal = Journal::new(&dir).expect("journal");
            let before = Snapshot::Registry(None);
            let op = journal
                .begin_apply("PRIV-01", &registry_action(), &before)
                .expect("begin");
            journal
                .finish_apply(op, "PRIV-01", &registry_action(), &before, None, Ok(()))
                .expect("finish");
        }
        let path = dir.join(JOURNAL_FILE_NAME);
        let mut text = fs::read_to_string(&path).expect("read");
        text.push_str("not a json line\n");
        fs::write(&path, text).expect("append garbage");
        let journal = Journal::new(&dir).expect("reopen");
        assert_eq!(journal.records().expect("records").len(), 2);
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
