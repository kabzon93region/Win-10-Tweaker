# Карта кода

Навигация по текущему коду. Источник истины — исходники; карта обновляется на каждом этапе,
где меняются публичные типы, интерфейсы или поток данных.

Актуально для этапа 7 (каталоги, очистка/оптимизация, фон, системная информация, сервис, обслуживание).

## Crates

| Crate | Назначение |
|---|---|
| `crates/tweaker-core` | Модель, движок, журнал, адаптерные интерфейсы, фейки. Не знает Win32. |
| `crates/tweaker-windows` | Реальные адаптеры: реестр, файлы, процессы, службы, планировщик задач, права, язык, сведения об ОС. |
| `crates/tweaker-app` | Окно egui, меню разделов, локализация, CLI-режимы, самопроверка. |

## tweaker-core

| Файл | Публичные элементы | Роль |
|---|---|---|
| `src/action.rs` | `RegistryRoot`, `RegistryKey`, `RegistryValue`, `ServiceStartMode`, `Action` (включая `HostsBlock`, `TaskSetEnabled`, `TaskSetEnabledMatching`, `RegistryDeleteKey`, `FileWriteBytes`, `FileDeleteAny`, `ProcessRunReversible`, `AppxRemove`, `CleanPaths`), `render_hosts_block`/`strip_hosts_block`/`hosts_block_applied` | Декларативные действия; `Action::describe()` — предпросмотр, `is_reversible()` |
| `src/catalog.rs` | `CatalogError`, `load_privacy`, `load_services`, `load_ui`, `load_system`, `load_context`, `load_apps`, `load_cleaner`, `load_optimization`, `load_maintenance`, `load_backup`, `load_section` | Embedded-каталоги `data/tweaks/{privacy,services,ui,system,context,apps,cleaner,optimization,maintenance,backup}.json`; `load_section` возвращает `None` для инструментов |
| `src/adapters.rs` | трейты `IRegistry`, `IFileSystem`, `IServiceControl`, `IProcessRunner`, `IFirewall`, `IScheduler`, `IStartup`, `IAppx`, `ISystemInfo`; `AdapterError`, `ServiceState`, `ServiceStatus`, `TaskInfo`, `RegistryTree`, `StartupEntry`, `StartupSource`, `AppxPackage`, `CleanStats`, `ProcessOutput`, `OsVersion`, `FirewallRule`, `FirewallDirection` | Единственный мост к системе; `IRegistry::read_tree`/`delete_tree` — снимок и удаление ключа с поддеревом; `IFileSystem::read_bytes`/`write_bytes` — бинарные файлы |
| `src/model.rs` | `RiskLevel`, `TweakDefinition`, `DefinitionError`, `TweakState`, `PlanStep`, `Plan` | Пункт каталога как данные; валидация id/ключей |
| `src/journal.rs` | `Snapshot` (значение, дерево ключа, текст, бинарный файл, служба, задачи, пакет), `Phase`, `JournalRecord`, `AppliedOp`, `Journal` | JSONL-журнал: две фазы, ротация, чтение, поиск неоткаченных и незавершённых операций |
| `src/engine.rs` | `Engine`, `ApplyReport` (включая `cancelled`, `freed_bytes`, `skipped_files`), `RevertReport`, `FailedAction`, `PlanError`, помощники `registry_set`, `service_start_mode` | detect/plan/apply/apply_with/revert/revert_pending; `Engine::new(registry, files, services, processes, scheduler, journal)` + `with_appx(&dyn IAppx)` |
| `src/fake.rs` | `FakeRegistry`, `FakeFileSystem`, `FakeServiceControl`, `FakeProcessRunner`, `FakeFirewall`, `FakeScheduler`, `FakeAppx`, `FakeSystemInfo` | In-memory адаптеры для тестов |
| `src/paths.rs` | `ProductPaths` | `%LocalAppData%\WinTweaker\{logs,journal,config,backups}` |
| `src/logging.rs` | `Level`, `Logger`, `LoggerConfig`, `mask_secret` | Лог с ротацией, буфер строк при недоступном файле |
| `src/sections.rs` | `Section`, `SectionKind` | Каталог разделов из `data/sections.json` |
| `src/lang.rs`, `src/product.rs` | `Lang`, константы продукта | Язык интерфейса, имя/версия |

## tweaker-windows

| Файл | Элементы | Роль |
|---|---|---|
| `src/registry.rs` | `WindowsRegistry` (`IRegistry`) | winreg: чтение/запись/удаление, все типы значений |
| `src/files.rs` | `WindowsFileSystem` (`IFileSystem`) | UTF-8 текст, создание родительских папок, `measure`/`clean` с `%VAR%`, пропуском занятых файлов и сохранением корней |
| `src/process.rs` | `SystemProcessRunner` (`IProcessRunner`) | `std::process::Command`, код возврата и вывод |
| `src/services.rs` | `WindowsServices` (`IServiceControl`) | SCM: состояние, старт/стоп, режим запуска |
| `src/startup.rs` | `WindowsStartup` (`IStartup`) | Пять источников автозагрузки, чтение `StartupApproved`, открытие расположения |
| `src/appx.rs` | `WindowsAppx` (`IAppx`) | Пакеты пользователя из репозитория AppModel; `Remove-AppxPackage` / `Add-AppxPackage -Register`; `CREATE_NO_WINDOW` |
| `src/scheduler.rs` | `WindowsScheduler` (`IScheduler`) | `schtasks.exe`: перечисление папки (CSV), состояние из XML, `/Change /ENABLE|/DISABLE`; `CREATE_NO_WINDOW` |
| `src/single_instance.rs` | `SingleInstance` | Именованный мьютекс `Local\WinTweaker.Instance`: второе окно не открывается; освобождается ядром при завершении |
| `src/message.rs` | `info(title, text)` | Модальное информационное окно до создания GUI и логгера |
| `src/elevation.rs` | `is_elevated`, `ensure_elevated` | Токен администратора, перезапуск через `runas` |
| `src/locale.rs` | `system_ui_language` | `GetUserDefaultUILanguage` → `Lang` |
| `src/os.rs` | `os_info`, `WindowsSystemInfo` (`ISystemInfo`) | Версия ОС и полный отчёт через CIM: ОС, CPU, RAM, GPU, мониторы, диски, сеть; UTF-8 |

## tweaker-app

| Файл | Элементы | Роль |
|---|---|---|
| `src/main.rs` | `Mode`, `parse_args` | `--selftest`, `--smoke`, `--screenshot <png>`, `--section <id>`, `--version`, `--help` |
| `src/app.rs` | публичные `RunMode`, `install_style`; приватные `TweakerApp` (реализует `eframe::App`), `CatalogView`, `CatalogAction`, `StartupView`, `StartupAction`, `AppsView`, `AppsAction`, `AppsPending`, `ApplyJob`, `JobProgress`, `TweakOutcome`, `SysinfoView`, `load_catalog`, `select_section`, `save_screenshot` | Окно, меню 14 разделов, универсальный экран каталога, экран автозагрузки, экран приложений, экран системной информации (копирование/сохранение), фоновое применение с прогрессом и отменой, баннер незавершённых операций, защита от второй копии окна, статус-бар, режим снимка + watchdog |
| `examples/tweak_probe.rs` | `main`, `run` | Живая проба одного пункта каталога вне окна; в релиз не входит |
| `src/i18n.rs` | `Localizer`, `detect_lang` | ru/en из embedded JSON, форматирование `{placeholders}` |
| `src/selftest.rs` | `run` | Проверки без окна: пути, лог, локализация, разделы, права, ОС |
| `build.rs` | — | Манифест: release `requireAdministrator`, debug `asInvoker` |

## Тесты

| Файл | Что проверяет |
|---|---|
| `crates/tweaker-core/tests/common/mod.rs` | Общий harness: фейковая машина, посев состояния по действиям пункта, полный приёмочный цикл (`check_item`) |
| `crates/tweaker-core/tests/privacy_section.rs` | 17 тестов по пунктам `privacy` + обратимость, недоступность без служб, ошибки записи hosts и реестра |
| `crates/tweaker-core/tests/services_section.rs` | 24 теста по пунктам `services` + недоступность без службы + восстановление режима запуска |
| `crates/tweaker-core/tests/ui_section.rs` | 17 тестов по пунктам `ui` + проверка обратимости действий |
| `crates/tweaker-core/tests/system_section.rs` | 18 тестов по пунктам `system` + аргументы команд, недоступность SYS-03/07, обратимость |
| `crates/tweaker-core/tests/context_section.rs` | 15 тестов по пунктам `context` + проверка обратимости, команд без `%~`, скриптов брандмауэра/ссылок |
| Автозагрузка | `startup.rs` (5 тестов), `app.rs` (4 теста действий) | пять источников, состояние `StartupApproved`, адреса ключей удаления |
| `crates/tweaker-core/tests/apps_section.rs` | 24 теста по пунктам `apps` + проверка семейств пакетов |
| `crates/tweaker-core/tests/cleaner_section.rs` | 16 тестов по пунктам `cleaner` + абсолютные пути/переменные среды + критические пункты |
| `crates/tweaker-core/tests/optimization_section.rs` | 4 теста по пунктам `optimization` + белый список программ |
| Приложения | `appx.rs` (3 теста), `engine.rs` (круговой тест `AppxRemove`) | поиск пакета, семейство из полного имени, удаление и регистрация манифеста |
| Обслуживание/сервис | `maintenance_section.rs` (6), `backup_section.rs` (4) | команды FIX и BKP, обратимость, применение только по явной команде |
| Системная информация | `os.rs` (2 теста отчёта) | разбор CIM-JSON, живой отчёт с ОС и CPU |
| `crates/*/src/**` (`#[cfg(test)]`) | Юнит-тесты движка, журнала, логгера, каталога, модели, адаптеров Windows, локализации, CLI-режимов |

## Поток данных

```
TweakDefinition (данные каталога, этап 3)
   └─ Engine::detect → TweakState (applied | notApplied | unavailable)
   └─ Engine::plan   → Plan { steps[action, before: Snapshot, preview], skipped }
   └─ Engine::apply  → для каждого шага:
                          Journal::begin_apply   (фаза started, снимок before)
                          adapter.execute(action)
                          adapter.read → verify
                          Journal::finish_apply  (applied | failed + ошибка)
   └─ Engine::revert → Journal::applied_ops(Some(id)) в обратном порядке:
                          restore(action, before) → verify_restored → Journal::record_revert
   └─ Engine::revert_pending → Journal::pending() → тот же restore
```

Правила, зашитые в движок:

1. Снимок `before` берётся до действия; без снимка действие не выполняется.
2. После записи состояние перечитывается; расхождение — `failed`, а не `applied`.
3. План идемпотентен: действия, уже находящиеся в целевом состоянии, попадают в `skipped`.
4. Журнал двухфазный: `started` до изменения, `applied`/`failed` после.
5. Откат идёт в обратном порядке и тоже проверяется; повторный откат — `nothing_to_revert`.
6. `ProcessRun` необратим и в откате помечается ошибкой.

## Известные пробелы (на этапе 7)

- Подмножество требований очистки/оптимизации зафиксировано в D-39/D-45: OPT-01/03 без выбора алгоритма, CLN-11 — 5 корней, CLN-04 без Firefox/Cent.
- Профиль и проверка обновлений не выведены в меню, VirusTotal показывает заглушку — до ответов D-46…D-48.
- Каталог: `privacy` 17/17, `services` 24/24, `ui` 17/17, `system` 18/18, `context` 15/17 (CTX-07/08 исключены), `apps` 24/24, `cleaner` 16/18, `optimization` 4/6 (D-39), `backup` 4/4, `maintenance` 6/6; `autorun` — динамический экран на 5 источников.
- UI-02/12/13 — документированная часть требований (подмена шрифтов, 8 значений, классическое меню), см. D-25.
- Отмена длительной операции не прерывает уже запущенную команду (`compact`/`dism`), только следующие действия (D-38).
- Баннер незавершённых операций проверен чтением кода; живой сценарий сбоя не воспроизводился.
- Защита от второй копии действует в пределах текущей сессии (`Local\`, D-7).
- Экспорт журнала не реализован (D-3, этап 7).
- Реальный адаптер `IFirewall` не реализован; брандмауэр работает через документированные команды `netsh`.
- Живые удаление и восстановление пакетов Store не выполнялись: проверены чтение реального списка и тесты на фейках (этап 8).
- Планировщик: запись (`/Change`) проверена только на фейках (F-17).
- Службы: запись на живой системе не выполнялась (F-14); `DiagTrack`/`NvTelemetryContainer` на этом ПК отсутствуют (F-18).
- После отката созданный пустой ключ реестра остаётся (значение удаляется); удаление пустых ключей не реализовано (D-6).
- Список доменов hosts собран из публичных источников и требует проверки (F-12).
- Файловый адаптер читает только UTF-8 (для `hosts` с иной кодировкой понадобится отдельное решение, F-11).
- Тест записи в HKCU помечен `#[ignore]`: из папки проекта запись в реестр запрещена средой;
  отдельно запущенный из `%TEMP%` бинарник теста проходит.

## Проверка

```
cargo test --workspace
cargo test -p tweaker-core --test privacy_section
cargo test -p tweaker-core --test services_section
cargo test -p tweaker-core --test ui_section
cargo test -p tweaker-core --test system_section
cargo test -p tweaker-core --test context_section
cargo test -p tweaker-core --test apps_section
cargo test -p tweaker-core --test cleaner_section
cargo test -p tweaker-core --test optimization_section
cargo test -p tweaker-core --test maintenance_section
cargo test -p tweaker-core --test backup_section
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Отдельный запуск ignored-теста записи в HKCU

Из папки проекта запись в реестр запрещена средой (F-10), поэтому тест помечен `#[ignore]`.
Он проверяется копией тест-бинарника вне рабочей папки:

```
cargo test -p tweaker-windows --no-run
# взять путь из строки "Executable unittests ... (путь.exe)", скопировать в %TEMP%\wt-tests\
%TEMP%\wt-tests\tweaker_windows_tests.exe --ignored --exact registry::tests::write_read_delete_under_current_user
```
