# Карта кода

Навигация по текущему коду. Источник истины — исходники; карта обновляется на каждом этапе,
где меняются публичные типы, интерфейсы или поток данных.

Актуально для этапа 2 (движок apply/detect/revert + журнал).

## Crates

| Crate | Назначение |
|---|---|
| `crates/tweaker-core` | Модель, движок, журнал, адаптерные интерфейсы, фейки. Не знает Win32. |
| `crates/tweaker-windows` | Реальные адаптеры: реестр, файлы, процессы, права, язык, сведения об ОС. |
| `crates/tweaker-app` | Окно egui, меню разделов, локализация, CLI-режимы, самопроверка. |

## tweaker-core

| Файл | Публичные элементы | Роль |
|---|---|---|
| `src/action.rs` | `RegistryRoot`, `RegistryKey`, `RegistryValue`, `ServiceStartMode`, `Action` | Декларативные действия; `Action::describe()` — предпросмотр, `is_reversible()` |
| `src/adapters.rs` | трейты `IRegistry`, `IFileSystem`, `IServiceControl`, `IProcessRunner`, `IFirewall`, `IScheduler`, `IAppx`, `ISystemInfo`; `AdapterError`, `ServiceState`, `ServiceStatus`, `ProcessOutput`, `OsVersion`, `FirewallRule`, `FirewallDirection` | Единственный мост к системе |
| `src/model.rs` | `RiskLevel`, `TweakDefinition`, `DefinitionError`, `TweakState`, `PlanStep`, `Plan` | Пункт каталога как данные; валидация id/ключей |
| `src/journal.rs` | `Snapshot`, `Phase`, `JournalRecord`, `AppliedOp`, `Journal` | JSONL-журнал: две фазы, ротация, чтение, поиск неоткаченных и незавершённых операций |
| `src/engine.rs` | `Engine`, `ApplyReport`, `RevertReport`, `FailedAction`, `PlanError`, помощники `registry_set`, `service_start_mode` | detect/plan/apply/revert/revert_pending |
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
| `src/elevation.rs` | `is_elevated`, `ensure_elevated` | Токен администратора, перезапуск через `runas` |
| `src/locale.rs` | `system_ui_language` | `GetUserDefaultUILanguage` → `Lang` |
| `src/os.rs` | `os_info` | Версия ОС из реестра (только чтение) |

## tweaker-app

| Файл | Элементы | Роль |
|---|---|---|
| `src/main.rs` | `Mode`, `parse_args` | `--selftest`, `--smoke`, `--screenshot <png>`, `--version`, `--help` |
| `src/app.rs` | `RunMode`, `TweakerApp`, `install_style`, `save_screenshot` | Окно, меню 14 разделов, статус-бар, режим снимка + watchdog |
| `src/i18n.rs` | `Localizer`, `detect_lang` | ru/en из embedded JSON, форматирование `{placeholders}` |
| `src/selftest.rs` | `run` | Проверки без окна: пути, лог, локализация, разделы, права, ОС |
| `build.rs` | — | Манифест: release `requireAdministrator`, debug `asInvoker` |

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

## Известные пробелы (на этапе 2)

- Каталог `TweakDefinition` ещё не наполнен; UI не подключён к движку — этап 3.
- Реальные адаптеры `IServiceControl`, `IFirewall`, `IScheduler`, `IAppx`, `ISystemInfo` не реализованы.
- В `Action` пока нет действий для hosts, брандмауэра, планировщика, AppX, задач.
- Файловый адаптер читает только UTF-8 (для `hosts` с иной кодировкой понадобится отдельное решение).
- Тест записи в HKCU помечен `#[ignore]`: из папки проекта запись в реестр запрещена средой;
  отдельно запущенный из `%TEMP%` бинарник теста проходит.

## Проверка

```
cargo test --workspace
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
