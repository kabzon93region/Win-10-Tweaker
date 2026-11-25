//! Per-item acceptance tests for the application catalog (TESTPLAN §4).

mod common;

item_test!(app_01_store, "apps", "APP-01");
item_test!(app_02_photos, "apps", "APP-02");
item_test!(app_03_solitaire, "apps", "APP-03");
item_test!(app_04_onenote, "apps", "APP-04");
item_test!(app_05_paint3d, "apps", "APP-05");
item_test!(app_06_3dbuilder, "apps", "APP-06");
item_test!(app_07_sticky_notes, "apps", "APP-07");
item_test!(app_08_xbox, "apps", "APP-08");
item_test!(app_09_alarms, "apps", "APP-09");
item_test!(app_10_recorder, "apps", "APP-10");
item_test!(app_11_camera, "apps", "APP-11");
item_test!(app_12_maps, "apps", "APP-12");
item_test!(app_13_groove, "apps", "APP-13");
item_test!(app_14_people, "apps", "APP-14");
item_test!(app_15_music, "apps", "APP-15");
item_test!(app_16_oneconnect, "apps", "APP-16");
item_test!(app_17_weather, "apps", "APP-17");
item_test!(app_18_gethelp, "apps", "APP-18");
item_test!(app_19_communications, "apps", "APP-19");
item_test!(app_20_skype, "apps", "APP-20");
item_test!(app_21_getstarted, "apps", "APP-21");
item_test!(app_22_messaging, "apps", "APP-22");
item_test!(app_23_officehub, "apps", "APP-23");
item_test!(app_24_feedback, "apps", "APP-24");

#[test]
fn every_app_package_family_is_a_dotted_name() {
    for tweak in common::section("apps") {
        let actions = tweak.actions.clone();
        let family = actions
            .iter()
            .find_map(|action| match action {
                tweaker_core::action::Action::AppxRemove { family } => Some(family),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{} has no appx action", tweak.id));
        assert!(family.contains('.'), "{}: {family}", tweak.id);
    }
}
