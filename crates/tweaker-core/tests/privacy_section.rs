//! Per-item acceptance tests for the privacy catalog (TESTPLAN §4).

mod common;

use common::{Harness, item, section, seed};
use tweaker_core::journal::Phase;
use tweaker_core::model::TweakState;

item_test!(priv_01_advertising_id, "privacy", "PRIV-01");
item_test!(priv_02_setting_sync, "privacy", "PRIV-02");
item_test!(
    priv_03_telemetry_service_and_office_tasks,
    "privacy",
    "PRIV-03"
);
item_test!(priv_04_nvidia_telemetry, "privacy", "PRIV-04");
item_test!(priv_05_scheduled_task_collection, "privacy", "PRIV-05");
item_test!(priv_06_app_inventory, "privacy", "PRIV-06");
item_test!(priv_07_app_usage_statistics, "privacy", "PRIV-07");
item_test!(priv_08_handwriting_data, "privacy", "PRIV-08");
item_test!(priv_09_hosts_block, "privacy", "PRIV-09");
item_test!(priv_10_feedback_notifications, "privacy", "PRIV-10");
item_test!(priv_11_location, "privacy", "PRIV-11");
item_test!(priv_12_siuf, "privacy", "PRIV-12");
item_test!(priv_13_speech_model_updates, "privacy", "PRIV-13");
item_test!(priv_14_activity_history, "privacy", "PRIV-14");
item_test!(priv_15_experimentation, "privacy", "PRIV-15");
item_test!(priv_16_diagnostics_services, "privacy", "PRIV-16");
item_test!(priv_17_diagnostics_hub, "privacy", "PRIV-17");

#[test]
fn every_privacy_action_is_reversible() {
    for tweak in section("privacy") {
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
        let tweak = item("privacy", id);
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
    seed(&harness, &section("privacy"));
    let tweak = item("privacy", "PRIV-09");
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
    seed(&harness, &section("privacy"));
    let tweak = item("privacy", "PRIV-01");
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
