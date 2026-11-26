//! Per-item acceptance tests for the cleanup catalog (TESTPLAN §4).

mod common;

item_test!(cleaner_01_background, "cleaner", "CLN-01");
item_test!(cleaner_02_game_cache, "cleaner", "CLN-02");
item_test!(cleaner_03_installer_cache, "cleaner", "CLN-03");
item_test!(cleaner_04_browser_cache, "cleaner", "CLN-04");
item_test!(cleaner_05_thumbnails, "cleaner", "CLN-05");
item_test!(cleaner_06_defender, "cleaner", "CLN-06");
item_test!(cleaner_07_patch_cache, "cleaner", "CLN-07");
item_test!(cleaner_08_update_cache, "cleaner", "CLN-08");
item_test!(cleaner_09_system_logs, "cleaner", "CLN-09");
item_test!(cleaner_10_registry_traces, "cleaner", "CLN-10");
item_test!(cleaner_11_empty_folders, "cleaner", "CLN-11");
item_test!(cleaner_12_hibernation, "cleaner", "CLN-12");
item_test!(cleaner_13_temporary, "cleaner", "CLN-13");
item_test!(cleaner_14_diagnostics, "cleaner", "CLN-14");
item_test!(cleaner_15_reports, "cleaner", "CLN-15");
item_test!(cleaner_16_languages, "cleaner", "CLN-16");

#[test]
fn cleanup_paths_are_absolute_or_use_environment_variables() {
    for tweak in common::section("cleaner") {
        for action in &tweak.actions {
            if let tweaker_core::action::Action::CleanPaths { paths } = action {
                assert!(!paths.is_empty(), "{}: empty path list", tweak.id);
                for path in paths {
                    let text = path.display().to_string();
                    assert!(
                        text.starts_with("%") || text.contains(":\\"),
                        "{}: relative path {text}",
                        tweak.id
                    );
                }
            }
        }
    }
}

#[test]
fn dangerous_cleanup_items_are_marked_critical() {
    let tweaks = common::section("cleaner");
    let critical: Vec<&str> = tweaks
        .iter()
        .filter(|tweak| tweak.risk == tweaker_core::model::RiskLevel::Critical)
        .map(|tweak| tweak.id.as_str())
        .collect();
    assert_eq!(critical, ["CLN-08", "CLN-11", "CLN-16"]);
}
