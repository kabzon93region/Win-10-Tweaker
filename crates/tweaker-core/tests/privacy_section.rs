//! Per-item acceptance tests for the privacy catalog (TESTPLAN §4).
//!
//! Every catalog item gets its own test: the machine is seeded from the item's
//! own action list, so a new tweak is covered by adding data, not by editing this
//! file. Each test walks the full loop: detect → apply → verify → idempotent
//! re-plan → revert → verify → repeated revert.
//!
//! Everything runs on fake adapters: no registry, files, services or tasks of the
//! real machine are touched.

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

struct Harness {
    registry: FakeRegistry,
    files: FakeFileSystem,
    services: FakeServiceControl,
    processes: FakeProcessRunner,
    scheduler: FakeScheduler,
    journal: Journal,
    dir: PathBuf,
}

impl Harness {
    fn new(tag: &str) -> Self {
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

    fn engine(&self) -> Engine<'_> {
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
        .join(format!("privacy-{tag}-{unique}"))
}

fn privacy() -> Vec<TweakDefinition> {
    catalog::load_privacy().expect("catalog must load")
}

fn item(id: &str) -> TweakDefinition {
    privacy()
        .into_iter()
        .find(|tweak| tweak.id == id)
        .unwrap_or_else(|| panic!("{id} is missing from the catalog"))
}

/// Puts the fake machine into a state where every action of the catalog still
/// has work to do: services run automatically, tasks are enabled, hosts exists
/// without the managed block. Registry values are absent by default.
fn seed(harness: &Harness, tweaks: &[TweakDefinition]) {
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

fn check_item(id: &str) {
    let harness = Harness::new(id);
    seed(&harness, &privacy());
    let tweak = item(id);
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

macro_rules! item_test {
    ($name:ident, $id:literal) => {
        #[test]
        fn $name() {
            check_item($id);
        }
    };
}

item_test!(priv_01_advertising_id, "PRIV-01");
item_test!(priv_02_setting_sync, "PRIV-02");
item_test!(priv_03_telemetry_service_and_office_tasks, "PRIV-03");
item_test!(priv_04_nvidia_telemetry, "PRIV-04");
item_test!(priv_05_scheduled_task_collection, "PRIV-05");
item_test!(priv_06_app_inventory, "PRIV-06");
item_test!(priv_07_app_usage_statistics, "PRIV-07");
item_test!(priv_08_handwriting_data, "PRIV-08");
item_test!(priv_09_hosts_block, "PRIV-09");
item_test!(priv_10_feedback_notifications, "PRIV-10");
item_test!(priv_11_location, "PRIV-11");
item_test!(priv_12_siuf, "PRIV-12");
item_test!(priv_13_speech_model_updates, "PRIV-13");
item_test!(priv_14_activity_history, "PRIV-14");
item_test!(priv_15_experimentation, "PRIV-15");
item_test!(priv_16_diagnostics_services, "PRIV-16");
item_test!(priv_17_diagnostics_hub, "PRIV-17");

#[test]
fn every_privacy_action_is_reversible() {
    for tweak in privacy() {
        for action in &tweak.actions {
            assert!(
                action.is_reversible(),
                "{}: {:?} cannot be reverted",
                tweak.id,
                action
            );
        }
    }
}

#[test]
fn every_service_item_is_unavailable_without_its_service() {
    for id in ["PRIV-03", "PRIV-04", "PRIV-14", "PRIV-16", "PRIV-17"] {
        let harness = Harness::new(&format!("{id}-missing"));
        let tweak = item(id);
        let engine = harness.engine();
        match engine.detect(&tweak) {
            TweakState::Unavailable { reason } => {
                assert!(
                    reason.contains("service"),
                    "{id}: reason must name the service: {reason}"
                );
            }
            other => panic!("{id}: expected unavailable, got {other:?}"),
        }
    }
}

#[test]
fn hosts_write_failure_is_reported() {
    let harness = Harness::new("hosts-failure");
    seed(&harness, &privacy());
    let tweak = item("PRIV-09");
    *harness.files.fail.borrow_mut() = Some("disk is read-only".to_owned());
    let engine = harness.engine();
    let report = engine.apply_tweak(&tweak);
    assert!(!report.success(), "{report:?}");
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(
        report.failed[0].error.contains("disk is read-only"),
        "{report:?}"
    );
    assert_eq!(
        engine.detect(&tweak),
        TweakState::NotApplied,
        "a failed write must not count as applied"
    );
    let records = harness.journal.records().expect("records");
    assert!(
        records.iter().any(|record| {
            record.phase == Phase::Failed
                && record
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("disk is read-only"))
        }),
        "the journal must keep the failure text"
    );
}

#[test]
fn registry_write_failure_is_reported() {
    let harness = Harness::new("registry-failure");
    seed(&harness, &privacy());
    let tweak = item("PRIV-01");
    *harness.registry.fail.borrow_mut() = Some("access is denied".to_owned());
    let engine = harness.engine();
    let report = engine.apply_tweak(&tweak);
    assert!(!report.success(), "{report:?}");
    assert_eq!(report.failed.len(), tweak.actions.len(), "{report:?}");
    assert!(
        report
            .failed
            .iter()
            .all(|failure| failure.error.contains("access is denied")),
        "{report:?}"
    );
    let records = harness.journal.records().expect("records");
    assert!(
        records.iter().any(|record| {
            record.phase == Phase::Failed
                && record
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("access is denied"))
        }),
        "the journal must keep the failure text"
    );
    assert_eq!(engine.detect(&tweak), TweakState::NotApplied);
}
