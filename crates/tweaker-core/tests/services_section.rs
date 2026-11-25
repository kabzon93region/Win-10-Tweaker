//! Per-item acceptance tests for the services catalog (TESTPLAN §4).
//!
//! Every item is "stop the service and disable its startup"; reverting restores the
//! snapshot taken before the change.

mod common;

use common::{Harness, item, section};
use tweaker_core::adapters::IServiceControl;
use tweaker_core::model::TweakState;

item_test!(svc_01_ipsec_policy_agent, "services", "SVC-01");
item_test!(svc_02_ipsec_keys, "services", "SVC-02");
item_test!(svc_03_diagnostic_host, "services", "SVC-03");
item_test!(svc_04_diagnostic_system, "services", "SVC-04");
item_test!(svc_05_security_center, "services", "SVC-05");
item_test!(svc_06_defender_center, "services", "SVC-06");
item_test!(svc_07_themes, "services", "SVC-07");
item_test!(svc_08_font_cache, "services", "SVC-08");
item_test!(svc_09_web_client, "services", "SVC-09");
item_test!(svc_10_dns_client, "services", "SVC-10");
item_test!(svc_11_server, "services", "SVC-11");
item_test!(svc_12_netbios, "services", "SVC-12");
item_test!(svc_13_windows_update, "services", "SVC-13");
item_test!(svc_14_update_orchestrator, "services", "SVC-14");
item_test!(svc_15_touch_keyboard, "services", "SVC-15");
item_test!(svc_16_wmi_performance, "services", "SVC-16");
item_test!(svc_17_error_reporting, "services", "SVC-17");
item_test!(svc_18_system_events, "services", "SVC-18");
item_test!(svc_19_search, "services", "SVC-19");
item_test!(svc_20_superfetch, "services", "SVC-20");
item_test!(svc_21_print_spooler, "services", "SVC-21");
item_test!(svc_22_image_service, "services", "SVC-22");
item_test!(svc_23_sensor_services, "services", "SVC-23");
item_test!(svc_24_location, "services", "SVC-24");

#[test]
fn every_service_item_needs_existing_services() {
    for tweak in section("services") {
        let harness = Harness::new(&format!("{}-missing", tweak.id));
        let engine = harness.engine();
        match engine.detect(&tweak) {
            TweakState::Unavailable { reason } => {
                assert!(
                    reason.contains("service"),
                    "{}: reason must name the service: {reason}",
                    tweak.id
                );
            }
            other => panic!("{}: expected unavailable, got {other:?}", tweak.id),
        }
    }
}

#[test]
fn service_item_snapshot_restores_start_mode() {
    let harness = Harness::new("service-restore");
    common::seed(&harness, &section("services"));
    let tweak = item("services", "SVC-21");
    let engine = harness.engine();
    assert!(engine.apply_tweak(&tweak).success());
    assert_eq!(engine.detect(&tweak), TweakState::Applied);
    let revert = engine.revert("SVC-21");
    assert!(revert.success(), "{revert:?}");
    assert_eq!(
        harness
            .services
            .query("Spooler")
            .expect("query")
            .expect("service")
            .start_mode,
        tweaker_core::action::ServiceStartMode::Automatic,
        "the original start mode must be back"
    );
    assert_eq!(
        harness
            .services
            .query("Spooler")
            .expect("query")
            .expect("service")
            .status,
        tweaker_core::adapters::ServiceStatus::Running,
        "the service must run again"
    );
}
