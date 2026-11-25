//! Per-item acceptance tests for the interface catalog (TESTPLAN §4).

mod common;

item_test!(ui_01_tooltip_color, "ui", "UI-01");
item_test!(ui_02_classic_fonts, "ui", "UI-02");
item_test!(ui_03_title_buttons, "ui", "UI-03");
item_test!(ui_04_3d_objects, "ui", "UI-04");
item_test!(ui_05_this_pc_folders, "ui", "UI-05");
item_test!(ui_06_shortcut_arrows, "ui", "UI-06");
item_test!(ui_07_shortcut_prefix, "ui", "UI-07");
item_test!(ui_08_cursor_blink, "ui", "UI-08");
item_test!(ui_09_taskbar_preview, "ui", "UI-09");
item_test!(ui_10_taskbar_transparency, "ui", "UI-10");
item_test!(ui_11_tray_icons, "ui", "UI-11");
item_test!(ui_12_explorer_settings, "ui", "UI-12");
item_test!(ui_13_classic_menu, "ui", "UI-13");
item_test!(ui_14_scrollbars, "ui", "UI-14");
item_test!(ui_15_shift_animation, "ui", "UI-15");
item_test!(ui_16_this_pc_icon, "ui", "UI-16");
item_test!(ui_17_folder_windows, "ui", "UI-17");

#[test]
fn every_ui_action_is_reversible() {
    for tweak in common::section("ui") {
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
