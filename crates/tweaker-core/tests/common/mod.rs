//! Shared harness for the catalog acceptance tests.
//!
//! Every catalog item is seeded from its own action list, so the same checks work
//! for privacy, services and every section added later.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tweaker_core::action::{Action, ServiceStartMode};
use tweaker_core::adapters::{ServiceState, ServiceStatus};
use tweaker_core::catalog;
use tweaker_core::engine::Engine;
use tweaker_core::fake::{
    FakeFileSystem, FakeProcessRunner, FakeRegistry, FakeScheduler, FakeServiceControl,
};
use tweaker_core::journal::{Journal, Phase, Snapshot};
use tweaker_core::model::{TweakDefinition, TweakState};

pub struct Harness {
    pub registry: FakeRegistry,
    pub files: FakeFileSystem,
    pub services: FakeServiceControl,
    pub processes: FakeProcessRunner,
    pub scheduler: FakeScheduler,
    pub journal: Journal,
    dir: PathBuf,
}

impl Harness {
    pub fn new(tag: &str) -> Self {
        let dir = temp_dir(tag);
        let journal = Journal::new(&dir).expect("journal");
        Self {
            registry: FakeRegistry::default(),
            files: FakeFileSystem::default(),
            services: FakeServiceControl::default(),
            processes: FakeProcessRunner::default(),
            scheduler: FakeScheduler::default(),
            journal,
            dir,
        }
    }

    pub fn engine(&self) -> Engine<'_> {
        Engine::new(
            &self.registry,
            &self.files,
            &self.services,
            &self.processes,
            &self.scheduler,
            &self.journal,
        )
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before 1970")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-tmp")
        .join(format!("catalog-{tag}-{unique}"))
}

pub fn section(section: &str) -> Vec<TweakDefinition> {
    catalog::load_section(section)
        .expect("catalog must load")
        .unwrap_or_else(|| panic!("no catalog data for {section}"))
}

pub fn item(section_id: &str, id: &str) -> TweakDefinition {
    section(section_id)
        .into_iter()
        .find(|tweak| tweak.id == id)
        .unwrap_or_else(|| panic!("{id} is missing from {section_id}"))
}

/// Puts the fake machine into a state where every action still has work to do:
/// services run automatically, tasks are enabled, hosts exists without the
/// managed block. Registry values are absent by default.
pub fn seed(harness: &Harness, tweaks: &[TweakDefinition]) {
    for tweak in tweaks {
        for action in &tweak.actions {
            match action {
                Action::ServiceStart { service } => {
                    harness.services.add(
                        service,
                        ServiceState {
                            status: ServiceStatus::Stopped,
                            start_mode: ServiceStartMode::Automatic,
                        },
                    );
                }
                Action::ServiceStop { service } | Action::ServiceStartMode { service, .. } => {
                    harness.services.add(
                        service,
                        ServiceState {
                            status: ServiceStatus::Running,
                            start_mode: ServiceStartMode::Automatic,
                        },
                    );
                }
                Action::TaskSetEnabled { path, .. } => harness.scheduler.add(path, true),
                Action::TaskSetEnabledMatching {
                    folder, prefixes, ..
                } => {
                    let folder = folder.trim_end_matches('\\');
                    for prefix in prefixes {
                        harness
                            .scheduler
                            .add(&format!("{folder}\\{prefix}_Test"), true);
                    }
                }
                Action::HostsBlock { path, .. } => {
                    harness
                        .files
                        .put(path, "127.0.0.1 localhost\r\n# user line\r\n");
                }
                _ => {}
            }
        }
    }
}

/// Full acceptance loop for one catalog item.
pub fn check_item(section_id: &str, id: &str) {
    let harness = Harness::new(id);
    seed(&harness, &section(section_id));
    let tweak = item(section_id, id);
    let engine = harness.engine();

    assert_eq!(
        engine.detect(&tweak),
        TweakState::NotApplied,
        "{id}: the seeded machine must look untouched"
    );

    let plan = engine.plan(&tweak).expect("plan");
    assert_eq!(plan.steps.len(), tweak.actions.len(), "{id}: plan size");
    assert!(plan.skipped.is_empty(), "{id}: nothing is applied yet");
    for step in &plan.steps {
        match (&step.action, &step.before) {
            (
                Action::RegistrySet { .. } | Action::RegistryDelete { .. },
                Snapshot::Registry(None),
            ) => {}
            (
                Action::ServiceStart { .. }
                | Action::ServiceStop { .. }
                | Action::ServiceStartMode { .. },
                Snapshot::Service(Some(state)),
            ) => {
                assert_eq!(state.status, ServiceStatus::Running, "{id}: seeded service");
                assert_eq!(
                    state.start_mode,
                    ServiceStartMode::Automatic,
                    "{id}: seeded start mode"
                );
            }
            (Action::TaskSetEnabled { path, .. }, Snapshot::Tasks(tasks)) => {
                assert_eq!(tasks.len(), 1, "{id}: exact task snapshot");
                assert_eq!(&tasks[0].path, path, "{id}: snapshot path");
                assert!(tasks[0].enabled, "{id}: the seeded task is enabled");
            }
            (Action::TaskSetEnabledMatching { .. }, Snapshot::Tasks(tasks)) => {
                assert!(!tasks.is_empty(), "{id}: matching tasks were seeded");
                assert!(
                    tasks.iter().all(|task| task.enabled),
                    "{id}: the seeded tasks are enabled"
                );
            }
            (Action::HostsBlock { .. }, Snapshot::TextFile(Some(text))) => {
                assert!(text.contains("user line"), "{id}: hosts snapshot: {text:?}");
            }
            (Action::FileWrite { .. } | Action::FileDelete { .. }, Snapshot::TextFile(_)) => {}
            other => panic!("{id}: unexpected state before the action: {other:?}"),
        }
    }

    let report = engine.apply_tweak(&tweak);
    assert!(report.success(), "{id}: apply failed: {report:?}");
    assert!(report.failed.is_empty(), "{id}: {report:?}");
    assert_eq!(report.applied.len(), tweak.actions.len(), "{id}: applied");
    assert_eq!(
        engine.detect(&tweak),
        TweakState::Applied,
        "{id}: detect after apply"
    );

    let records = harness.journal.records().expect("journal records");
    assert_eq!(
        records.len(),
        tweak.actions.len() * 2,
        "{id}: every action needs a started and an applied record"
    );
    assert!(
        records.iter().all(|record| record.phase != Phase::Failed),
        "{id}: no failed records expected"
    );
    assert!(
        records.iter().any(|record| record.phase == Phase::Applied),
        "{id}: applied records expected"
    );
    assert!(
        records
            .iter()
            .filter(|record| record.phase == Phase::Started)
            .all(|record| record.before.is_some()),
        "{id}: every started record carries the state before"
    );

    let again = engine.plan(&tweak).expect("second plan");
    assert!(
        again.steps.is_empty(),
        "{id}: applying twice must not plan anything: {again:?}"
    );
    assert_eq!(again.skipped.len(), tweak.actions.len(), "{id}: skipped");

    let revert = engine.revert(id);
    assert!(revert.success(), "{id}: revert failed: {revert:?}");
    assert_eq!(revert.reverted.len(), tweak.actions.len(), "{id}: reverted");
    assert!(
        !revert.nothing_to_revert,
        "{id}: there was something to revert"
    );
    assert_eq!(
        engine.detect(&tweak),
        TweakState::NotApplied,
        "{id}: detect after revert"
    );
    assert!(
        harness
            .journal
            .applied_ops(Some(id))
            .expect("applied ops")
            .is_empty(),
        "{id}: nothing stays applied"
    );

    let twice = engine.revert(id);
    assert!(
        twice.nothing_to_revert && twice.success(),
        "{id}: the second revert is a no-op: {twice:?}"
    );
}

#[macro_export]
macro_rules! item_test {
    ($name:ident, $section:literal, $id:literal) => {
        #[test]
        fn $name() {
            $crate::common::check_item($section, $id);
        }
    };
}
