//! Apply / detect / revert engine.
//!
//! Rules from the package that are enforced here:
//! * every action takes a snapshot of the old state before it runs;
//! * after every action the state is re-read and compared with the target;
//! * every action is journaled twice (started, then applied/failed);
//! * repeated apply is a no-op (idempotent plan);
//! * revert restores exactly the snapshot; a value that did not exist is deleted.

use std::fmt;

use crate::action::{
    Action, RegistryValue, ServiceStartMode, hosts_block_applied, render_hosts_block,
    strip_hosts_block,
};
use crate::adapters::{
    IFileSystem, IProcessRunner, IRegistry, IServiceControl, ServiceState, ServiceStatus,
};
use crate::journal::{Journal, Snapshot};
use crate::model::{Plan, PlanStep, TweakDefinition, TweakState};

/// One action that could not be executed or verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedAction {
    /// Failed action.
    pub action: Action,
    /// Human-readable reason.
    pub error: String,
}

/// Outcome of applying one tweak.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Tweak id.
    pub tweak_id: String,
    /// Actions that ran and were verified.
    pub applied: Vec<Action>,
    /// Actions that were already in the target state.
    pub skipped: Vec<Action>,
    /// Actions that failed.
    pub failed: Vec<FailedAction>,
    /// The run stopped early because the journal became unavailable.
    pub aborted: bool,
    /// Planning failed before anything was changed.
    pub error: Option<String>,
}

impl ApplyReport {
    /// `true` when nothing failed and nothing was skipped silently.
    pub fn success(&self) -> bool {
        self.failed.is_empty() && self.error.is_none() && !self.aborted
    }
}

/// Outcome of reverting a tweak or unfinished operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevertReport {
    /// Tweak id, empty when reverting pending operations of several tweaks.
    pub tweak_id: String,
    /// Actions that were restored and verified.
    pub reverted: Vec<Action>,
    /// Actions that could not be restored.
    pub failed: Vec<FailedAction>,
    /// Nothing was found to revert.
    pub nothing_to_revert: bool,
    /// The journal could not be read.
    pub error: Option<String>,
}

impl RevertReport {
    /// `true` when every restore succeeded (or there was nothing to do).
    pub fn success(&self) -> bool {
        self.failed.is_empty() && self.error.is_none()
    }
}

/// Why a plan could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The tweak cannot be applied on this machine.
    Unavailable {
        /// Explanation shown to the user.
        reason: String,
    },
    /// The old state could not be captured; nothing may be changed.
    Snapshot {
        /// Action whose snapshot failed.
        action: Action,
        /// Explanation.
        message: String,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Unavailable { reason } => write!(f, "unavailable: {reason}"),
            PlanError::Snapshot { action, message } => {
                write!(f, "cannot snapshot {}: {message}", action.describe())
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// The engine, wired to adapters and to the journal.
pub struct Engine<'a> {
    registry: &'a dyn IRegistry,
    files: &'a dyn IFileSystem,
    services: &'a dyn IServiceControl,
    processes: &'a dyn IProcessRunner,
    journal: &'a Journal,
}

impl<'a> Engine<'a> {
    /// Wires the engine to its adapters.
    pub fn new(
        registry: &'a dyn IRegistry,
        files: &'a dyn IFileSystem,
        services: &'a dyn IServiceControl,
        processes: &'a dyn IProcessRunner,
        journal: &'a Journal,
    ) -> Self {
        Self {
            registry,
            files,
            services,
            processes,
            journal,
        }
    }

    /// Current state of a tweak.
    pub fn detect(&self, tweak: &TweakDefinition) -> TweakState {
        let mut all_satisfied = true;
        for action in &tweak.actions {
            match self.satisfied(action) {
                Ok(true) => {}
                Ok(false) => all_satisfied = false,
                Err(reason) => return TweakState::Unavailable { reason },
            }
        }
        if all_satisfied {
            TweakState::Applied
        } else {
            TweakState::NotApplied
        }
    }

    /// Builds the preview plan: only actions that still have to run.
    pub fn plan(&self, tweak: &TweakDefinition) -> Result<Plan, PlanError> {
        let mut steps = Vec::new();
        let mut skipped = Vec::new();
        for action in &tweak.actions {
            match self.satisfied(action) {
                Ok(true) => skipped.push(action.clone()),
                Ok(false) => {
                    let before = self
                        .snapshot(action)
                        .map_err(|message| PlanError::Snapshot {
                            action: action.clone(),
                            message,
                        })?;
                    steps.push(PlanStep {
                        action: action.clone(),
                        before,
                        preview: action.describe(),
                    });
                }
                Err(reason) => return Err(PlanError::Unavailable { reason }),
            }
        }
        Ok(Plan {
            tweak_id: tweak.id.clone(),
            steps,
            skipped,
        })
    }

    /// Executes a plan. Errors of one action do not stop the others.
    pub fn apply(&self, tweak: &TweakDefinition, plan: &Plan) -> ApplyReport {
        let mut report = ApplyReport {
            tweak_id: tweak.id.clone(),
            ..ApplyReport::default()
        };
        for step in &plan.steps {
            // Re-check right before the change: the system may have changed since planning.
            match self.satisfied(&step.action) {
                Ok(true) => {
                    report.skipped.push(step.action.clone());
                    continue;
                }
                Ok(false) => {}
                Err(reason) => {
                    report.failed.push(FailedAction {
                        action: step.action.clone(),
                        error: reason,
                    });
                    continue;
                }
            }
            let before = match self.snapshot(&step.action) {
                Ok(before) => before,
                Err(error) => {
                    report.failed.push(FailedAction {
                        action: step.action.clone(),
                        error,
                    });
                    continue;
                }
            };
            let op_id = match self.journal.begin_apply(&tweak.id, &step.action, &before) {
                Ok(op_id) => op_id,
                Err(error) => {
                    report.failed.push(FailedAction {
                        action: step.action.clone(),
                        error: format!("journal is unavailable: {error}"),
                    });
                    report.aborted = true;
                    break;
                }
            };
            let result = self
                .execute(&step.action)
                .and_then(|()| self.verify(&step.action));
            let after = self.snapshot(&step.action).ok();
            if let Err(error) = self.journal.finish_apply(
                op_id,
                &tweak.id,
                &step.action,
                &before,
                after,
                result.clone(),
            ) {
                report.failed.push(FailedAction {
                    action: step.action.clone(),
                    error: format!("journal is unavailable: {error}"),
                });
                report.aborted = true;
                break;
            }
            match result {
                Ok(()) => report.applied.push(step.action.clone()),
                Err(error) => report.failed.push(FailedAction {
                    action: step.action.clone(),
                    error,
                }),
            }
        }
        report
    }

    /// Plans and applies in one call.
    pub fn apply_tweak(&self, tweak: &TweakDefinition) -> ApplyReport {
        match self.plan(tweak) {
            Ok(plan) => self.apply(tweak, &plan),
            Err(error) => ApplyReport {
                tweak_id: tweak.id.clone(),
                error: Some(error.to_string()),
                ..ApplyReport::default()
            },
        }
    }

    /// Reverts every applied action of the tweak, newest first.
    pub fn revert(&self, tweak_id: &str) -> RevertReport {
        let mut report = RevertReport {
            tweak_id: tweak_id.to_owned(),
            ..RevertReport::default()
        };
        let ops = match self.journal.applied_ops(Some(tweak_id)) {
            Ok(ops) => ops,
            Err(error) => {
                report.error = Some(format!("cannot read the journal: {error}"));
                return report;
            }
        };
        if ops.is_empty() {
            report.nothing_to_revert = true;
            return report;
        }
        for op in ops.iter().rev() {
            self.revert_one(&mut report, &op.tweak_id, &op.action, &op.before, op.op_id);
        }
        report
    }

    /// Restores operations that started but never finished (crash recovery).
    pub fn revert_pending(&self) -> RevertReport {
        let mut report = RevertReport::default();
        let pending = match self.journal.pending() {
            Ok(pending) => pending,
            Err(error) => {
                report.error = Some(format!("cannot read the journal: {error}"));
                return report;
            }
        };
        if pending.is_empty() {
            report.nothing_to_revert = true;
            return report;
        }
        for record in pending {
            let before = record.before.clone().unwrap_or(Snapshot::None);
            self.revert_one(
                &mut report,
                &record.tweak_id,
                &record.action,
                &before,
                record.op_id,
            );
        }
        report
    }

    fn revert_one(
        &self,
        report: &mut RevertReport,
        tweak_id: &str,
        action: &Action,
        before: &Snapshot,
        op_id: u64,
    ) {
        if !action.is_reversible() {
            report.failed.push(FailedAction {
                action: action.clone(),
                error: "this action cannot be reverted".to_owned(),
            });
            return;
        }
        match self
            .restore(action, before)
            .and_then(|()| self.verify_restored(action, before))
        {
            Ok(()) => match self.journal.record_revert(tweak_id, action, op_id) {
                Ok(_) => report.reverted.push(action.clone()),
                Err(error) => report.failed.push(FailedAction {
                    action: action.clone(),
                    error: format!("journal is unavailable: {error}"),
                }),
            },
            Err(error) => report.failed.push(FailedAction {
                action: action.clone(),
                error,
            }),
        }
    }

    fn service_state(&self, service: &str) -> Result<ServiceState, String> {
        self.services
            .query(service)
            .map_err(|error| error.message)?
            .ok_or_else(|| format!("service {service} is not installed"))
    }

    fn satisfied(&self, action: &Action) -> Result<bool, String> {
        match action {
            Action::RegistrySet { key, name, value } => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                Ok(current.as_ref() == Some(value))
            }
            Action::RegistryDelete { key, name } => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                Ok(current.is_none())
            }
            Action::FileWrite { path, content } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                Ok(current.as_deref() == Some(content.as_str()))
            }
            Action::FileDelete { path } => Ok(!self.files.exists(path)),
            Action::ServiceStart { service } => {
                Ok(self.service_state(service)?.status == ServiceStatus::Running)
            }
            Action::ServiceStop { service } => {
                Ok(self.service_state(service)?.status == ServiceStatus::Stopped)
            }
            Action::ServiceStartMode { service, mode } => {
                Ok(self.service_state(service)?.start_mode == *mode)
            }
            Action::ProcessRun { .. } => Ok(false),
            Action::HostsBlock { path, domains } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                Ok(current.is_some_and(|text| hosts_block_applied(&text, domains)))
            }
        }
    }

    fn snapshot(&self, action: &Action) -> Result<Snapshot, String> {
        match action {
            Action::RegistrySet { key, name, .. } | Action::RegistryDelete { key, name } => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                Ok(Snapshot::Registry(current))
            }
            Action::FileWrite { path, .. } | Action::FileDelete { path } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                Ok(Snapshot::TextFile(current))
            }
            Action::ServiceStart { service }
            | Action::ServiceStop { service }
            | Action::ServiceStartMode { service, .. } => {
                let state = self.service_state(service)?;
                Ok(Snapshot::Service(Some(state)))
            }
            Action::ProcessRun { .. } => Ok(Snapshot::None),
            Action::HostsBlock { path, .. } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                Ok(Snapshot::TextFile(current))
            }
        }
    }

    fn execute(&self, action: &Action) -> Result<(), String> {
        match action {
            Action::RegistrySet { key, name, value } => self
                .registry
                .write(key, name, value)
                .map_err(|error| error.message),
            Action::RegistryDelete { key, name } => self
                .registry
                .delete(key, name)
                .map_err(|error| error.message),
            Action::FileWrite { path, content } => self
                .files
                .write_text(path, content)
                .map_err(|error| error.message),
            Action::FileDelete { path } => self.files.delete(path).map_err(|error| error.message),
            Action::ServiceStart { service } => {
                if self.service_state(service)?.status == ServiceStatus::Running {
                    return Ok(());
                }
                self.services.start(service).map_err(|error| error.message)
            }
            Action::ServiceStop { service } => {
                if self.service_state(service)?.status == ServiceStatus::Stopped {
                    return Ok(());
                }
                self.services.stop(service).map_err(|error| error.message)
            }
            Action::ServiceStartMode { service, mode } => {
                if self.service_state(service)?.start_mode == *mode {
                    return Ok(());
                }
                self.services
                    .set_start_mode(service, *mode)
                    .map_err(|error| error.message)
            }
            Action::ProcessRun { program, args } => {
                let output = self
                    .processes
                    .run(program, args)
                    .map_err(|error| error.message)?;
                match output.code {
                    Some(0) => Ok(()),
                    Some(code) => {
                        let detail = output.stderr.trim();
                        if detail.is_empty() {
                            Err(format!("program exited with code {code}"))
                        } else {
                            Err(format!("program exited with code {code}: {detail}"))
                        }
                    }
                    None => Err("program was terminated before it finished".to_owned()),
                }
            }
            Action::HostsBlock { path, domains } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                let mut content = strip_hosts_block(&current.unwrap_or_default());
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push_str("\r\n");
                }
                content.push_str(&render_hosts_block(domains));
                self.files
                    .write_text(path, &content)
                    .map_err(|error| error.message)
            }
        }
    }

    fn verify(&self, action: &Action) -> Result<(), String> {
        match action {
            Action::RegistrySet { key, name, value } => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                if current.as_ref() == Some(value) {
                    Ok(())
                } else {
                    Err(format!("{key}: value {name} differs after write"))
                }
            }
            Action::RegistryDelete { key, name } => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                if current.is_none() {
                    Ok(())
                } else {
                    Err(format!("{key}: value {name} is still present after delete"))
                }
            }
            Action::FileWrite { path, content } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                if current.as_deref() == Some(content.as_str()) {
                    Ok(())
                } else {
                    Err(format!("{}: content differs after write", path.display()))
                }
            }
            Action::FileDelete { path } => {
                if self.files.exists(path) {
                    Err(format!(
                        "{}: file still exists after delete",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            }
            Action::ServiceStart { service } => {
                if self.service_state(service)?.status == ServiceStatus::Running {
                    Ok(())
                } else {
                    Err(format!("service {service} is not running after start"))
                }
            }
            Action::ServiceStop { service } => {
                if self.service_state(service)?.status == ServiceStatus::Stopped {
                    Ok(())
                } else {
                    Err(format!("service {service} is not stopped after stop"))
                }
            }
            Action::ServiceStartMode { service, mode } => {
                if self.service_state(service)?.start_mode == *mode {
                    Ok(())
                } else {
                    Err(format!("service {service} start mode was not changed"))
                }
            }
            Action::ProcessRun { .. } => Ok(()),
            Action::HostsBlock { path, domains } => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                if current.is_some_and(|text| hosts_block_applied(&text, domains)) {
                    Ok(())
                } else {
                    Err(format!(
                        "{}: hosts block is missing after write",
                        path.display()
                    ))
                }
            }
        }
    }

    fn restore(&self, action: &Action, before: &Snapshot) -> Result<(), String> {
        match (action, before) {
            (
                Action::RegistrySet { key, name, .. } | Action::RegistryDelete { key, name },
                Snapshot::Registry(previous),
            ) => match previous {
                Some(value) => self
                    .registry
                    .write(key, name, value)
                    .map_err(|error| error.message),
                None => self
                    .registry
                    .delete(key, name)
                    .map_err(|error| error.message),
            },
            (
                Action::FileWrite { path, .. }
                | Action::FileDelete { path }
                | Action::HostsBlock { path, .. },
                Snapshot::TextFile(previous),
            ) => match previous {
                Some(content) => self
                    .files
                    .write_text(path, content)
                    .map_err(|error| error.message),
                None => self.files.delete(path).map_err(|error| error.message),
            },
            (
                Action::ServiceStart { service }
                | Action::ServiceStop { service }
                | Action::ServiceStartMode { service, .. },
                Snapshot::Service(Some(state)),
            ) => {
                self.services
                    .set_start_mode(service, state.start_mode)
                    .map_err(|error| error.message)?;
                match state.status {
                    ServiceStatus::Running => {
                        self.services.start(service).map_err(|error| error.message)
                    }
                    ServiceStatus::Stopped => {
                        self.services.stop(service).map_err(|error| error.message)
                    }
                    other => Err(format!(
                        "cannot restore service {service} in state {other:?}"
                    )),
                }
            }
            (Action::ProcessRun { .. }, _) => Err("this action cannot be reverted".to_owned()),
            _ => Err("journal snapshot does not match the action".to_owned()),
        }
    }

    fn verify_restored(&self, action: &Action, before: &Snapshot) -> Result<(), String> {
        match (action, before) {
            (
                Action::RegistrySet { key, name, .. } | Action::RegistryDelete { key, name },
                Snapshot::Registry(previous),
            ) => {
                let current = self
                    .registry
                    .read(key, name)
                    .map_err(|error| error.message)?;
                if current == *previous {
                    Ok(())
                } else {
                    Err(format!("{key}: value {name} was not restored"))
                }
            }
            (
                Action::FileWrite { path, .. }
                | Action::FileDelete { path }
                | Action::HostsBlock { path, .. },
                Snapshot::TextFile(previous),
            ) => {
                let current = self.files.read_text(path).map_err(|error| error.message)?;
                if current == *previous {
                    Ok(())
                } else {
                    Err(format!("{}: file was not restored", path.display()))
                }
            }
            (
                Action::ServiceStart { service }
                | Action::ServiceStop { service }
                | Action::ServiceStartMode { service, .. },
                Snapshot::Service(previous),
            ) => {
                let current = self
                    .services
                    .query(service)
                    .map_err(|error| error.message)?;
                if current == *previous {
                    Ok(())
                } else {
                    Err(format!("service {service} was not restored"))
                }
            }
            (Action::ProcessRun { .. }, _) => Err("this action cannot be reverted".to_owned()),
            _ => Err("journal snapshot does not match the action".to_owned()),
        }
    }
}

/// Helper for tests and for the UI: a registry value that must become `value`.
pub fn registry_set(
    key: crate::action::RegistryKey,
    name: impl Into<String>,
    value: RegistryValue,
) -> Action {
    Action::RegistrySet {
        key,
        name: name.into(),
        value,
    }
}

/// Helper: a service must end up with this start mode.
pub fn service_start_mode(service: impl Into<String>, mode: ServiceStartMode) -> Action {
    Action::ServiceStartMode {
        service: service.into(),
        mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{RegistryKey, RegistryRoot};
    use crate::adapters::{AdapterError, IRegistry, ProcessOutput, ServiceState, ServiceStatus};
    use crate::fake::{FakeFileSystem, FakeProcessRunner, FakeRegistry, FakeServiceControl};
    use crate::model::{RiskLevel, TweakDefinition};
    use crate::test_support::temp_dir;
    use std::path::PathBuf;

    struct Harness {
        registry: FakeRegistry,
        files: FakeFileSystem,
        services: FakeServiceControl,
        processes: FakeProcessRunner,
        journal: Journal,
        dir: PathBuf,
    }

    impl Harness {
        fn new(tag: &str) -> Self {
            let dir = temp_dir(&format!("engine-{tag}"));
            let journal = Journal::new(&dir).expect("journal");
            Self {
                registry: FakeRegistry::default(),
                files: FakeFileSystem::default(),
                services: FakeServiceControl::default(),
                processes: FakeProcessRunner::default(),
                journal,
                dir,
            }
        }

        fn engine(&self) -> Engine<'_> {
            Engine::new(
                &self.registry,
                &self.files,
                &self.services,
                &self.processes,
                &self.journal,
            )
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn key() -> RegistryKey {
        RegistryKey::new(RegistryRoot::Hkcu, r"Software\WinTweakerTest")
    }

    fn set_value(value: u32) -> Action {
        registry_set(key(), "Value", RegistryValue::Dword(value))
    }

    fn tweak(actions: Vec<Action>) -> TweakDefinition {
        TweakDefinition::new(
            "PRIV-01",
            "privacy",
            "tweak.priv01.title",
            "tweak.priv01.description",
            RiskLevel::Medium,
            false,
            actions,
        )
    }

    #[test]
    fn detect_reports_not_applied_then_applied() {
        let harness = Harness::new("detect");
        let engine = harness.engine();
        let definition = tweak(vec![set_value(0)]);
        assert_eq!(engine.detect(&definition), TweakState::NotApplied);
        let report = engine.apply_tweak(&definition);
        assert!(report.success(), "{report:?}");
        assert_eq!(engine.detect(&definition), TweakState::Applied);
    }

    #[test]
    fn plan_has_preview_and_before_snapshot() {
        let harness = Harness::new("plan");
        let engine = harness.engine();
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(5));
        let plan = engine.plan(&tweak(vec![set_value(0)])).expect("plan");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan.steps[0].before,
            Snapshot::Registry(Some(RegistryValue::Dword(5)))
        );
        assert!(plan.steps[0].preview.contains("Value"));
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn already_applied_tweak_plans_nothing() {
        let harness = Harness::new("idempotent-plan");
        let engine = harness.engine();
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(0));
        let plan = engine.plan(&tweak(vec![set_value(0)])).expect("plan");
        assert!(plan.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        let report = engine.apply_tweak(&tweak(vec![set_value(0)]));
        assert!(report.success());
        assert!(report.applied.is_empty());
    }

    #[test]
    fn apply_is_idempotent() {
        let harness = Harness::new("idempotent-apply");
        let engine = harness.engine();
        let definition = tweak(vec![set_value(1)]);
        assert!(engine.apply_tweak(&definition).success());
        let second = engine.apply_tweak(&definition);
        assert!(second.success());
        assert!(second.applied.is_empty());
        assert!(second.skipped.is_empty());
        assert_eq!(engine.journal.applied_ops(None).expect("ops").len(), 1);
    }

    #[test]
    fn apply_continues_after_a_failed_action() {
        let harness = Harness::new("partial");
        let engine = harness.engine();
        *harness.registry.fail.borrow_mut() = Some("access denied".to_owned());
        let definition = tweak(vec![
            set_value(1),
            Action::FileWrite {
                path: PathBuf::from(r"C:\Temp\wintweaker-test.txt"),
                content: "hello".to_owned(),
            },
        ]);
        let report = engine.apply_tweak(&definition);
        assert!(!report.success());
        assert_eq!(report.applied.len(), 1, "file write must still run");
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].error.contains("access denied"));
    }

    #[test]
    fn verification_mismatch_is_reported_as_failure() {
        let harness = Harness::new("verify");
        let engine = harness.engine();
        *harness.registry.ignore_writes.borrow_mut() = true;
        let report = engine.apply_tweak(&tweak(vec![set_value(0)]));
        assert!(!report.success());
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].error.contains("differs after write"));
        let ops = engine.journal.applied_ops(None).expect("ops");
        assert!(ops.is_empty(), "failed actions must not be revertible");
    }

    #[test]
    fn revert_removes_created_value() {
        let harness = Harness::new("revert-created");
        let engine = harness.engine();
        let definition = tweak(vec![set_value(0)]);
        assert!(engine.apply_tweak(&definition).success());
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(revert.reverted.len(), 1);
        assert_eq!(harness.registry.get(&key(), "Value"), None);
    }

    #[test]
    fn revert_restores_previous_value() {
        let harness = Harness::new("revert-previous");
        let engine = harness.engine();
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(7));
        assert!(engine.apply_tweak(&tweak(vec![set_value(0)])).success());
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(
            harness.registry.get(&key(), "Value"),
            Some(RegistryValue::Dword(7))
        );
    }

    #[test]
    fn revert_twice_is_a_no_op() {
        let harness = Harness::new("revert-twice");
        let engine = harness.engine();
        let definition = tweak(vec![set_value(0)]);
        assert!(engine.apply_tweak(&definition).success());
        assert!(engine.revert("PRIV-01").success());
        let second = engine.revert("PRIV-01");
        assert!(second.nothing_to_revert);
        assert!(second.reverted.is_empty());
    }

    #[test]
    fn journal_survives_restart_between_apply_and_revert() {
        let dir = temp_dir("engine-restart");
        let registry = FakeRegistry::default();
        let files = FakeFileSystem::default();
        let services = FakeServiceControl::default();
        let processes = FakeProcessRunner::default();
        let definition = tweak(vec![set_value(0)]);
        {
            let journal = Journal::new(&dir).expect("journal");
            let engine = Engine::new(&registry, &files, &services, &processes, &journal);
            assert!(engine.apply_tweak(&definition).success());
        }
        let journal = Journal::new(&dir).expect("journal");
        let engine = Engine::new(&registry, &files, &services, &processes, &journal);
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(registry.get(&key(), "Value"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_operation_can_be_reverted() {
        let harness = Harness::new("pending");
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(7));
        // Simulate a crash: the journal has `started`, the value was already changed.
        let before = Snapshot::Registry(Some(RegistryValue::Dword(7)));
        harness
            .journal
            .begin_apply("PRIV-01", &set_value(0), &before)
            .expect("begin");
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(0));
        let engine = harness.engine();
        let report = engine.revert_pending();
        assert!(report.success(), "{report:?}");
        assert_eq!(
            harness.registry.get(&key(), "Value"),
            Some(RegistryValue::Dword(7))
        );
        assert!(engine.journal.pending().expect("pending").is_empty());
    }

    #[test]
    fn missing_service_makes_the_tweak_unavailable() {
        let harness = Harness::new("service-missing");
        let engine = harness.engine();
        let definition = tweak(vec![Action::ServiceStop {
            service: "NoSuchService".to_owned(),
        }]);
        assert!(matches!(
            engine.detect(&definition),
            TweakState::Unavailable { .. }
        ));
        assert!(matches!(
            engine.plan(&definition),
            Err(PlanError::Unavailable { .. })
        ));
        let report = engine.apply_tweak(&definition);
        assert!(report.error.is_some());
    }

    #[test]
    fn service_actions_apply_and_revert() {
        let harness = Harness::new("service");
        harness.services.add(
            "Spooler",
            ServiceState {
                status: ServiceStatus::Running,
                start_mode: ServiceStartMode::Automatic,
            },
        );
        let engine = harness.engine();
        let definition = tweak(vec![
            Action::ServiceStop {
                service: "Spooler".to_owned(),
            },
            Action::ServiceStartMode {
                service: "Spooler".to_owned(),
                mode: ServiceStartMode::Disabled,
            },
        ]);
        assert!(engine.apply_tweak(&definition).success());
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        let state = harness
            .services
            .query("Spooler")
            .expect("query")
            .expect("state");
        assert_eq!(state.status, ServiceStatus::Running);
        assert_eq!(state.start_mode, ServiceStartMode::Automatic);
    }

    #[test]
    fn process_failure_is_reported() {
        let harness = Harness::new("process");
        *harness.processes.output.borrow_mut() = ProcessOutput {
            code: Some(2),
            stdout: String::new(),
            stderr: "boom".to_owned(),
        };
        let engine = harness.engine();
        let definition = tweak(vec![Action::ProcessRun {
            program: "cmd.exe".to_owned(),
            args: vec!["/c".to_owned(), "exit 2".to_owned()],
        }]);
        let report = engine.apply_tweak(&definition);
        assert!(!report.success());
        assert!(report.failed[0].error.contains("code 2"));
        assert_eq!(harness.processes.runs.borrow().len(), 1);
    }

    #[test]
    fn file_write_is_reverted_to_previous_content() {
        let harness = Harness::new("file");
        let path = PathBuf::from(r"C:\Temp\wintweaker-test.txt");
        harness.files.put(&path, "old");
        let engine = harness.engine();
        let definition = tweak(vec![Action::FileWrite {
            path: path.clone(),
            content: "new".to_owned(),
        }]);
        assert!(engine.apply_tweak(&definition).success());
        assert_eq!(harness.files.get(&path).as_deref(), Some("new"));
        assert!(engine.revert("PRIV-01").success());
        assert_eq!(harness.files.get(&path).as_deref(), Some("old"));
    }

    #[test]
    fn adapter_error_during_snapshot_blocks_the_plan() {
        struct FailingRegistry;
        impl IRegistry for FailingRegistry {
            fn read(
                &self,
                _key: &RegistryKey,
                _name: &str,
            ) -> Result<Option<RegistryValue>, AdapterError> {
                Err(AdapterError::new("registry is unavailable"))
            }
            fn write(
                &self,
                _key: &RegistryKey,
                _name: &str,
                _value: &RegistryValue,
            ) -> Result<(), AdapterError> {
                Err(AdapterError::new("registry is unavailable"))
            }
            fn delete(&self, _key: &RegistryKey, _name: &str) -> Result<(), AdapterError> {
                Err(AdapterError::new("registry is unavailable"))
            }
        }
        let dir = temp_dir("engine-snapshot-fail");
        let journal = Journal::new(&dir).expect("journal");
        let registry = FailingRegistry;
        let files = FakeFileSystem::default();
        let services = FakeServiceControl::default();
        let processes = FakeProcessRunner::default();
        let engine = Engine::new(&registry, &files, &services, &processes, &journal);
        let plan = engine.plan(&tweak(vec![set_value(0)]));
        assert!(matches!(plan, Err(PlanError::Unavailable { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_error_is_reported_and_blocks_the_plan() {
        struct SnapshotFailRegistry {
            reads: std::cell::Cell<usize>,
        }
        impl IRegistry for SnapshotFailRegistry {
            fn read(
                &self,
                _key: &RegistryKey,
                _name: &str,
            ) -> Result<Option<RegistryValue>, AdapterError> {
                let count = self.reads.get() + 1;
                self.reads.set(count);
                if count == 1 {
                    Ok(None)
                } else {
                    Err(AdapterError::new("cannot read the old value"))
                }
            }
            fn write(
                &self,
                _key: &RegistryKey,
                _name: &str,
                _value: &RegistryValue,
            ) -> Result<(), AdapterError> {
                Err(AdapterError::new("must not write"))
            }
            fn delete(&self, _key: &RegistryKey, _name: &str) -> Result<(), AdapterError> {
                Err(AdapterError::new("must not delete"))
            }
        }
        let dir = temp_dir("engine-snapshot-error");
        let journal = Journal::new(&dir).expect("journal");
        let registry = SnapshotFailRegistry {
            reads: std::cell::Cell::new(0),
        };
        let files = FakeFileSystem::default();
        let services = FakeServiceControl::default();
        let processes = FakeProcessRunner::default();
        let engine = Engine::new(&registry, &files, &services, &processes, &journal);
        let plan = engine.plan(&tweak(vec![set_value(0)]));
        assert!(matches!(plan, Err(PlanError::Snapshot { .. })), "{plan:?}");
        assert_eq!(registry.reads.get(), 2);
        assert!(journal.records().expect("records").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn service_start_applies_and_reverts() {
        let harness = Harness::new("service-start");
        harness.services.add(
            "Spooler",
            ServiceState {
                status: ServiceStatus::Stopped,
                start_mode: ServiceStartMode::Automatic,
            },
        );
        let engine = harness.engine();
        let definition = tweak(vec![Action::ServiceStart {
            service: "Spooler".to_owned(),
        }]);
        assert!(engine.apply_tweak(&definition).success());
        let state = harness
            .services
            .query("Spooler")
            .expect("query")
            .expect("state");
        assert_eq!(state.status, ServiceStatus::Running);
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        let state = harness
            .services
            .query("Spooler")
            .expect("query")
            .expect("state");
        assert_eq!(state.status, ServiceStatus::Stopped);
        assert_eq!(state.start_mode, ServiceStartMode::Automatic);
    }

    #[test]
    fn process_run_cannot_be_reverted() {
        let harness = Harness::new("process-revert");
        let engine = harness.engine();
        let definition = tweak(vec![Action::ProcessRun {
            program: "cmd.exe".to_owned(),
            args: vec!["/c".to_owned(), "exit 0".to_owned()],
        }]);
        assert!(engine.apply_tweak(&definition).success());
        let revert = engine.revert("PRIV-01");
        assert!(!revert.success());
        assert!(revert.reverted.is_empty());
        assert_eq!(revert.failed.len(), 1);
        assert!(revert.failed[0].error.contains("cannot be reverted"));
    }

    #[test]
    fn registry_delete_removes_and_revert_restores() {
        let harness = Harness::new("registry-delete");
        harness
            .registry
            .set(&key(), "Value", RegistryValue::Dword(5));
        let engine = harness.engine();
        let definition = tweak(vec![Action::RegistryDelete {
            key: key(),
            name: "Value".to_owned(),
        }]);
        assert!(engine.apply_tweak(&definition).success());
        assert_eq!(harness.registry.get(&key(), "Value"), None);
        assert_eq!(engine.detect(&definition), TweakState::Applied);
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(
            harness.registry.get(&key(), "Value"),
            Some(RegistryValue::Dword(5))
        );
    }

    #[test]
    fn file_delete_removes_and_revert_restores() {
        let harness = Harness::new("file-delete");
        let path = PathBuf::from(r"C:\Temp\wintweaker-delete.txt");
        harness.files.put(&path, "keep me");
        let engine = harness.engine();
        let definition = tweak(vec![Action::FileDelete { path: path.clone() }]);
        assert!(engine.apply_tweak(&definition).success());
        assert!(!harness.files.exists(&path));
        assert_eq!(engine.detect(&definition), TweakState::Applied);
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(harness.files.get(&path).as_deref(), Some("keep me"));
    }

    #[test]
    fn hosts_block_applies_and_reverts() {
        let harness = Harness::new("hosts");
        let path = PathBuf::from(r"C:\Temp\hosts");
        let original = "127.0.0.1 localhost\r\n";
        harness.files.put(&path, original);
        let engine = harness.engine();
        let definition = tweak(vec![Action::HostsBlock {
            path: path.clone(),
            domains: vec!["a.example".to_owned(), "b.example".to_owned()],
        }]);
        assert!(engine.apply_tweak(&definition).success());
        let text = harness.files.get(&path).expect("hosts file");
        assert!(text.contains("0.0.0.0 a.example"));
        assert!(text.contains("0.0.0.0 b.example"));
        assert!(text.contains("localhost"));
        assert_eq!(engine.detect(&definition), TweakState::Applied);
        // Applying again changes nothing.
        assert!(engine.apply_tweak(&definition).applied.is_empty());
        let revert = engine.revert("PRIV-01");
        assert!(revert.success(), "{revert:?}");
        assert_eq!(harness.files.get(&path).as_deref(), Some(original));
    }

    #[test]
    fn hosts_block_preserves_bytes_and_separates_lines() {
        let harness = Harness::new("hosts-bytes");
        let path = PathBuf::from(r"C:\Temp\hosts");
        let original = "127.0.0.1 localhost\r\n# no trailing newline";
        harness.files.put(&path, original);
        let engine = harness.engine();
        let definition = tweak(vec![Action::HostsBlock {
            path: path.clone(),
            domains: vec!["a.example".to_owned()],
        }]);
        assert!(engine.apply_tweak(&definition).success());
        let text = harness.files.get(&path).expect("hosts file");
        assert!(
            text.starts_with("127.0.0.1 localhost\r\n# no trailing newline\r\n"),
            "the original bytes must stay intact: {text:?}"
        );
        assert!(text.contains(crate::action::HOSTS_BLOCK_START));
        assert!(engine.revert("PRIV-01").success());
        assert_eq!(harness.files.get(&path).as_deref(), Some(original));
    }

    #[test]
    fn hosts_block_keeps_foreign_lines_and_ignores_other_blocks() {
        let harness = Harness::new("hosts-foreign");
        let path = PathBuf::from(r"C:\Temp\hosts");
        let original = "127.0.0.1 localhost\r\n0.0.0.0 ads.example\r\n";
        harness.files.put(&path, original);
        let engine = harness.engine();
        let definition = tweak(vec![Action::HostsBlock {
            path: path.clone(),
            domains: vec!["a.example".to_owned()],
        }]);
        assert!(engine.apply_tweak(&definition).success());
        let text = harness.files.get(&path).expect("hosts file");
        assert!(text.contains("0.0.0.0 ads.example"));
        assert_eq!(text.matches(crate::action::HOSTS_BLOCK_START).count(), 1);
    }
}
