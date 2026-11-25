# Карта кода

Навигация по текущему коду. Источник истины — исходники; карта обновляется на каждом этапе,
где меняются публичные типы, интерфейсы или поток данных.

Актуально для этапа 5, часть 3 (разделы `privacy`, `services`, `ui`, `system`).

## Crates

| Crate | Назначение |
|---|---|
| `crates/tweaker-core` | Модель, движок, журнал, адаптерные интерфейсы, фейки. Не знает Win32. |
| `crates/tweaker-windows` | Реальные адаптеры: реестр, файлы, процессы, службы, планировщик задач, права, язык, сведения об ОС. |
| `crates/tweaker-app` | Окно egui, меню разделов, локализация, CLI-режимы, самопроверка. |

## tweaker-core

| Файл | Публичные элементы | Роль |
|---|---|---|
| `src/action.rs` | `RegistryRoot`, `RegistryKey`, `RegistryValue`, `ServiceStartMode`, `Action` (включая `HostsBlock`, `TaskSetEnabled`, `TaskSetEnabledMatching`, `RegistryDeleteKey`, `FileWriteBytes`, `ProcessRunReversible`), `render_hosts_block`/`strip_hosts_block`/`hosts_block_applied` | Декларативные действия; `Action::describe()` — предпросмотр, `is_reversible()` |
| `src/catalog.rs` | `CatalogError`, `load_privacy`, `load_services`, `load_ui`, `load_system`, `load_section` | Embedded-каталоги `data/tweaks/{privacy,services,ui,system}.json`; `load_section` возвращает `None` для инструментов и ещё не заполненных разделов |
| `src/adapters.rs` | трейты `IRegistry`, `IFileSystem`, `IServiceControl`, `IProcessRunner`, `IFirewall`, `IScheduler`, `IAppx`, `ISystemInfo`; `AdapterError`, `ServiceState`, `ServiceStatus`, `TaskInfo`, `RegistryTree`, `ProcessOutput`, `OsVersion`, `FirewallRule`, `FirewallDirection` | Единственный мост к системе; `IRegistry::read_tree`/`delete_tree` — снимок и удаление ключа с поддеревом; `IFileSystem::read_bytes`/`write_bytes` — бинарные файлы |
| `src/model.rs` | `RiskLevel`, `TweakDefinition`, `DefinitionError`, `TweakState`, `PlanStep`, `Plan` | Пункт каталога как данные; валидация id/ключей |
| `src/journal.rs` | `Snapshot` (значение, дерево ключа, текст, бинарный файл, служба, задачи), `Phase`, `JournalRecord`, `AppliedOp`, `Journal` | JSONL-журнал: две фазы, ротация, чтение, поиск неоткаченных и незавершённых операций |
| `src/engine.rs` | `Engine`, `ApplyReport`, `RevertReport`, `FailedAction`, `PlanError`, помощники `registry_set`, `service_start_mode` | detect/plan/apply/revert/revert_pending; `Engine::new(registry, files, services, processes, scheduler, journal)` |
| `src/fake.rs` | `FakeRegistry`, `FakeFileSystem`, `FakeServiceControl`, `FakeProcessRunner`, `FakeFirewall`, `FakeScheduler`, `FakeAppx`, `FakeSystemInfo` | In-memory адаптеры для тестов |
| `src/paths.rs` | `ProductPaths` | `%LocalAppData%\WinTweaker\{logs,journal,config,backups}` |
| `src/logging.rs` | `Level`, `Logger`, `LoggerConfig`, `mask_secret` | Лог с ротацией, буфер строк при недоступном файле |
| `src/sections.rs` | `Section`, `SectionKind` | Каталог разделов из `data/sections.json` |
| `src/lang.rs`, `src/product.rs` | `Lang`, константы продукта | Язык интерфейса, имя/версия |

## tweaker-windows

| Файл | Элементы | Роль |
|---|---|---|
| `src/registry.rs` | `WindowsRegistry` (`IRegistry`) | winreg: чтение/запись/удаление, все типы значений |
| `src/files.rs` | `WindowsFileSystem` (`IFileSystem`) | UTF-8 текст, создание родительских папок |
| `src/process.rs` | `SystemProcessRunner` (`IProcessRunner`) | `std::process::Command`, код возврата и вывод |
| `src/services.rs` | `WindowsServices` (`IServiceControl`) | SCM: состояние, старт/стоп, режим запуска |
| `src/scheduler.rs` | `WindowsScheduler` (`IScheduler`) | `schtasks.exe`: перечисление папки (CSV), состояние из XML, `/Change /ENABLE|/DISABLE`; `CREATE_NO_WINDOW` |
| `src/single_instance.rs` | `SingleInstance` | Именованный мьютекс `Local\WinTweaker.Instance`: второе окно не открывается; освобождается ядром при завершении |
| `src/message.rs` | `info(title, text)` | Модальное информационное окно до создания GUI и логгера |
| `src/elevation.rs` | `is_elevated`, `ensure_elevated` | Токен администратора, перезапуск через `runas` |
| `src/locale.rs` | `system_ui_language` | `GetUserDefaultUILanguage` → `Lang` |
| `src/os.rs` | `os_info` | Версия ОС из реестра (только чтение) |

## tweaker-app

| Файл | Элементы | Роль |
|---|---|---|
| `src/main.rs` | `Mode`, `parse_args` | `--selftest`, `--smoke`, `--screenshot <png>`, `--section <id>`, `--version`, `--help` |
| `src/app.rs` | публичные `RunMode`, `install_style`; приватные `TweakerApp` (реализует `eframe::App`), `CatalogView`, `CatalogAction`, `load_catalog`, `select_section`, `save_screenshot` | Окно, меню 14 разделов, универсальный экран каталога (любой раздел с данными), защита от второй копии окна, статус-бар, режим снимка + watchdog |
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

## Известные пробелы (на этапе 5, часть 2)

- Каталог: `privacy` 17/17, `services` 24/24, `ui` 17/17, `system` 18/18; `context`, `apps`, `autorun` ещё показывают заглушку.
- UI-02/12/13 — документированная часть требований (подмена шрифтов, 8 значений, классическое меню), см. D-25.
- Применение выполняется в UI-потоке: прогресса и отмены нет (дефект D-2 из `TEST_REPORT.md`, этап 6).
- Незавершённая операция не предлагается к откату при старте (D-8, этап 6).
- Защита от второй копии действует в пределах текущей сессии (`Local\`, D-7).
- Экспорт журнала не реализован (D-3, этап 7).
- Реальные адаптеры `IFirewall`, `IAppx`, `ISystemInfo` не реализованы; действий для брандмауэра и AppX в `Action` нет.
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
