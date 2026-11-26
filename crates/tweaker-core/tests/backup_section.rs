//! Per-item acceptance tests for the backup catalog.

mod common;

item_test!(backup_01_restore_point, "backup", "BKP-01");
item_test!(backup_02_system_restore, "backup", "BKP-02");
item_test!(backup_03_export_drivers, "backup", "BKP-03");
item_test!(backup_04_import_drivers, "backup", "BKP-04");
