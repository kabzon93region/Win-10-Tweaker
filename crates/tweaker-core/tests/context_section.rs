//! Per-item acceptance tests for the context menu catalog (TESTPLAN §4).

mod common;

item_test!(ctx_01_take_ownership, "context", "CTX-01");
item_test!(ctx_02_computer_menu, "context", "CTX-02");
item_test!(ctx_03_command_prompt, "context", "CTX-03");
item_test!(ctx_04_shell_new, "context", "CTX-04");
item_test!(ctx_05_hidden_files, "context", "CTX-05");
item_test!(ctx_06_copy_image, "context", "CTX-06");
item_test!(ctx_09_firewall_menu, "context", "CTX-09");
item_test!(ctx_10_empty_folder, "context", "CTX-10");
item_test!(ctx_11_symlink, "context", "CTX-11");
item_test!(ctx_12_nfo_notepad, "context", "CTX-12");
item_test!(ctx_13_wmp_items, "context", "CTX-13");
item_test!(ctx_14_menu_delay, "context", "CTX-14");
item_test!(ctx_15_extra_menu_items, "context", "CTX-15");
item_test!(ctx_16_new_items, "context", "CTX-16");
item_test!(ctx_17_personalize_menu, "context", "CTX-17");

#[test]
fn every_context_action_is_reversible() {
    for tweak in common::section("context") {
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
fn shell_commands_do_not_rely_on_cmd_modifiers() {
    for tweak in common::section("context") {
        for action in &tweak.actions {
            if let tweaker_core::action::Action::RegistrySet { name, value, .. } = action
                && let tweaker_core::action::RegistryValue::String(text) = value
                && (name.is_empty() || name == "MUIVerb")
            {
                assert!(
                    !text.contains("%~"),
                    "{}: Explorer does not expand cmd modifiers: {text}",
                    tweak.id
                );
            }
        }
    }
}

#[test]
fn firewall_and_link_commands_use_helper_scripts() {
    let harness = common::Harness::new("context-scripts");
    common::seed(&harness, &common::section("context"));
    let engine = harness.engine();

    let firewall = common::item("context", "CTX-09");
    assert!(engine.apply_tweak(&firewall).success());
    let block = harness
        .files
        .get(std::path::Path::new(
            r"C:\Windows\WinTweaker\block-internet.ps1",
        ))
        .expect("block script");
    assert!(block.contains("dir=out"), "{block}");
    assert!(
        block.contains("dir=in"),
        "both directions must be blocked: {block}"
    );
    let unblock = harness
        .files
        .get(std::path::Path::new(
            r"C:\Windows\WinTweaker\unblock-internet.ps1",
        ))
        .expect("unblock script");
    assert!(
        unblock.contains("dir=out") && unblock.contains("dir=in"),
        "{unblock}"
    );

    let link = common::item("context", "CTX-11");
    assert!(engine.apply_tweak(&link).success());
    let create = harness
        .files
        .get(std::path::Path::new(
            r"C:\Windows\WinTweaker\create-link.ps1",
        ))
        .expect("link script");
    assert!(create.contains("mklink"), "{create}");
    assert!(
        create.contains("mklink /D"),
        "folders need a directory link: {create}"
    );
    assert!(create.contains("Split-Path -Leaf"), "{create}");
    assert!(create.contains("Desktop"), "{create}");

    let player = common::item("context", "CTX-13");
    assert_eq!(player.actions.len(), 10, "audio, video and WMP types");
}
