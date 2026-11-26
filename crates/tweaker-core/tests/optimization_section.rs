//! Per-item acceptance tests for the optimization catalog (TESTPLAN §4).

mod common;

item_test!(optimization_01_compact_files, "optimization", "OPT-01");
item_test!(optimization_02_compactos, "optimization", "OPT-02");
item_test!(optimization_03_winsxs, "optimization", "OPT-03");
item_test!(optimization_04_component_cleanup, "optimization", "OPT-04");

#[test]
fn optimization_commands_are_documented_tools() {
    for tweak in common::section("optimization") {
        for action in &tweak.actions {
            let program = match action {
                tweaker_core::action::Action::ProcessRun { program, .. }
                | tweaker_core::action::Action::ProcessRunReversible { program, .. } => program,
                _ => panic!("{}: unexpected action", tweak.id),
            };
            assert!(
                matches!(
                    program.as_str(),
                    "compact.exe" | "dism.exe" | "powershell.exe"
                ),
                "{}: {program}",
                tweak.id
            );
        }
    }
}
