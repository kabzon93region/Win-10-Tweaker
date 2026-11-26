//! Per-item acceptance tests for the maintenance catalog.

mod common;

item_test!(fix_01_f8_menu, "maintenance", "FIX-01");
item_test!(fix_02_msi_extract, "maintenance", "FIX-02");
item_test!(fix_03_edge_prelaunch, "maintenance", "FIX-03");
item_test!(fix_04_folder_cmd, "maintenance", "FIX-04");
item_test!(fix_05_numlock, "maintenance", "FIX-05");
item_test!(fix_06_broken_shortcuts, "maintenance", "FIX-06");
