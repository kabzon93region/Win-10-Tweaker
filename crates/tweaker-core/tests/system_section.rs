//! Per-item acceptance tests for the system catalog (TESTPLAN §4).

mod common;

use tweaker_core::adapters::IRegistry;
use tweaker_core::model::TweakState;

item_test!(sys_01_security_notification, "system", "SYS-01");
item_test!(sys_02_download_warning, "system", "SYS-02");
item_test!(sys_03_defender_smartscreen, "system", "SYS-03");
item_test!(sys_04_uac, "system", "SYS-04");
item_test!(sys_05_layout_hotkeys, "system", "SYS-05");
item_test!(sys_06_lock_screen_timeout, "system", "SYS-06");
item_test!(sys_07_memory_diagnostics, "system", "SYS-07");
item_test!(sys_08_teredo_ipv6, "system", "SYS-08");
item_test!(sys_09_cortana, "system", "SYS-09");
item_test!(sys_10_internet_explorer, "system", "SYS-10");
item_test!(sys_11_desktop_shortcuts, "system", "SYS-11");
item_test!(sys_12_system_cache, "system", "SYS-12");
item_test!(sys_13_startup_delay, "system", "SYS-13");
item_test!(sys_14_quick_access, "system", "SYS-14");
item_test!(sys_15_emoji_hotkey, "system", "SYS-15");
item_test!(sys_16_photo_viewer, "system", "SYS-16");
item_test!(sys_17_onedrive, "system", "SYS-17");
item_test!(sys_az_proxy_pac, "system", "SYS-AZ");

#[test]
fn every_system_action_is_reversible() {
    for tweak in common::section("system") {
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
fn system_commands_use_the_documented_arguments() {
    let harness = common::Harness::new("system-commands");
    common::seed(&harness, &common::section("system"));
    let engine = harness.engine();

    let netsh = common::item("system", "SYS-08");
    assert!(engine.apply_tweak(&netsh).success());
    let runs = harness.processes.runs.borrow().clone();
    let netsh_runs: Vec<String> = runs
        .iter()
        .filter(|(program, _)| program == "netsh.exe")
        .map(|(_, args)| args.join(" "))
        .collect();
    assert_eq!(netsh_runs.len(), 5, "{netsh_runs:?}");
    assert!(netsh_runs.contains(&"interface teredo set state disabled".to_owned()));
    assert!(netsh_runs.contains(&"interface isatap set state disabled".to_owned()));
    assert!(netsh_runs.contains(&"interface 6to4 set state disabled".to_owned()));
    assert!(
        netsh_runs.contains(&"interface ipv6 set global randomizeidentifiers=disabled".to_owned())
    );
    assert!(netsh_runs.contains(&"interface ipv6 set privacy state=disabled".to_owned()));

    let revert = engine.revert("SYS-08");
    assert!(revert.success(), "{revert:?}");
    let runs = harness.processes.runs.borrow().clone();
    let reverse: Vec<String> = runs
        .iter()
        .skip(5)
        .filter(|(program, _)| program == "netsh.exe")
        .map(|(_, args)| args.join(" "))
        .collect();
    assert_eq!(reverse.len(), 5, "{reverse:?}");
    assert!(reverse.contains(&"interface teredo set state default".to_owned()));
    assert!(reverse.contains(&"interface isatap set state default".to_owned()));
    assert!(reverse.contains(&"interface 6to4 set state default".to_owned()));
    assert!(reverse.contains(&"interface ipv6 set global randomizeidentifiers=enabled".to_owned()));
    assert!(reverse.contains(&"interface ipv6 set privacy state=enabled".to_owned()));

    let shortcuts = common::item("system", "SYS-11");
    assert!(engine.apply_tweak(&shortcuts).success());
    let runs = harness.processes.runs.borrow().clone();
    let powershell = runs
        .iter()
        .filter(|(program, _)| program == "powershell.exe")
        .map(|(_, args)| args.join(" "))
        .collect::<Vec<_>>();
    assert!(
        powershell.iter().any(|args| args.contains("reboot.vbs")
            && args.contains("refresh.vbs")
            && args.contains("Перезагрузка.lnk")
            && args.contains("Обновить оболочку.lnk")
            && args.contains("ErrorActionPreference")),
        "{powershell:?}"
    );
    let revert = engine.revert("SYS-11");
    assert!(revert.success(), "{revert:?}");
    let runs = harness.processes.runs.borrow().clone();
    assert!(
        runs.iter().any(|(program, args)| {
            let text = args.join(" ");
            program == "powershell.exe"
                && text.contains("Remove-Item")
                && text.contains("WinTweaker")
        }),
        "{runs:?}"
    );

    let onedrive = common::item("system", "SYS-17");
    assert!(engine.apply_tweak(&onedrive).success());
    let runs = harness.processes.runs.borrow().clone();
    assert!(
        runs.iter().any(|(_, args)| {
            let text = args.join(" ");
            text.contains("OneDriveSetup.exe")
                && text.contains("/uninstall")
                && text.contains("LASTEXITCODE")
                && text.contains("$folder")
                && text.contains("OneDrive.lnk")
                && text.contains("Test-Path")
                && text.contains("ErrorAction Stop")
                && text.contains("Remove-Item -LiteralPath $p")
        }),
        "{runs:?}"
    );
    let clsid = tweaker_core::action::RegistryKey::new(
        tweaker_core::action::RegistryRoot::Hkcu,
        r"Software\Classes\CLSID\{018D5C66-4533-4307-9B53-224DE2ED1FE6}",
    );
    assert!(
        harness.registry.read_tree(&clsid).expect("tree").is_none(),
        "the CLSID key must be removed after apply"
    );
    let revert = engine.revert("SYS-17");
    assert!(revert.success(), "{revert:?}");
    assert!(
        harness.registry.read_tree(&clsid).expect("tree").is_some(),
        "the CLSID key must be restored"
    );
    let runs = harness.processes.runs.borrow().clone();
    assert!(
        runs.iter().any(|(program, args)| {
            program == "C:\\Windows\\System32\\OneDriveSetup.exe" && args.is_empty()
        }),
        "the revert must run the OneDrive installer: {runs:?}"
    );
}

#[test]
fn defender_item_is_unavailable_without_smartscreen() {
    let harness = common::Harness::new("system-smartscreen-missing");
    common::seed(&harness, &common::section("system"));
    harness.services.remove("smartscreen");
    let tweak = common::item("system", "SYS-03");
    let engine = harness.engine();
    match engine.detect(&tweak) {
        TweakState::Unavailable { reason } => {
            assert!(
                reason.contains("smartscreen"),
                "reason must name the service: {reason}"
            );
        }
        other => panic!("expected unavailable, got {other:?}"),
    }
}

#[test]
fn memory_diagnostics_need_the_tasks() {
    let harness = common::Harness::new("system-memory-missing");
    let tweak = common::item("system", "SYS-07");
    let engine = harness.engine();
    assert!(matches!(
        engine.detect(&tweak),
        TweakState::Unavailable { .. }
    ));
}
