# Состояние работ

Обновлено: 2026-11-25 (этап 3 завершён: раздел `privacy` — все 17 пунктов).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 3 — раздел `privacy`: 17 из 17 пунктов (реестр, файлы, hosts, службы, задачи планировщика) |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Git | `af4531e` (этап 3, часть 1; этап 3 решения — `061b853`, этап 2 — `1e05d14`, этап 1 — `88de898`); часть 2 — коммит после аудита |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно (28,5 с) |
| Артефакт | `target/release/wintweaker.exe` — 6 735 360 байт, sha256 `9728675A9C833CF7251A01874318709095D56421AAD4FDCD566B3161809FD6F8` |
| Тесты | `cargo test --workspace` — 120 passed, 1 ignored (app 17, core 82, windows 21) |
| Проверки | `--selftest` rc=0; `--screenshot` — PNG 1200×780 со всеми 17 пунктами и счётчиком; clippy/fmt чисто (`%TEMP%\wintweaker-stage3-lint.txt`); живой detect всех 17 пунктов (F-18); живой apply/revert PRIV-13 |
| Запуск | debug-сборка: от старта до первых кадров ≈ 1,8 с (с чтением 17 состояний и установкой шрифтов); release — замер на этапе 8 |
| Блокирует | нет; открытые вопросы D-04…D-06 не мешают этапу 4 |

## Окружение сборки

| Компонент | Версия |
|---|---|
| ОС | Windows 10 Pro 22H2, сборка 19045.6456 |
| rustc | 1.100.0 (0ef1bc386 2026-11-09), target `x86_64-pc-windows-msvc` |
| cargo | 1.100.0 |
| egui / eframe | 0.37.0 |
| windows | 0.62.2 |
| winreg | 0.56.0 |
| serde / serde_json | 1.0.229 / 1.0.154 |
| chrono | 0.4.45 |
| image | 0.25.10 (только png) |
| embed-manifest | 1.5.1 |

`Cargo.lock` включён в поставку (первый коммит этапа): версии зависимостей зафиксированы;
побайтовая воспроизводимость сборки проверяется на этапе 8.

## Структура решения

| Путь | Что внутри |
|---|---|
| `Cargo.toml` | workspace из трёх crates, resolver 3, edition 2024 |
| `crates/tweaker-core` | модель `TweakDefinition/Action/Plan`, каталог разделов (`data/sections.json`) и раздел `privacy` (17 пунктов), движок apply/detect/revert, журнал JSONL с двумя фазами, трейты адаптеров и фейки, действия hosts-блока и задач планировщика, пути, логгер, `Lang` |
| `crates/tweaker-windows` | адаптеры реестра, файлов, процессов, служб и планировщика задач (через `schtasks.exe`); права администратора и перезапуск, язык системы, сведения об ОС |
| `crates/tweaker-app` | окно egui, меню 14 разделов, ru/en, режимы `--selftest`, `--smoke`, `--screenshot`, проба `tweak_probe` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 125 строк, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | завершён |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | завершён |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | не начат |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | не начат |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | не начат |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | не начат |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | не начат |

## Артефакты этапов 1–3

| Артефакт | Где | Проверка |
|---|---|---|
| Каталог `privacy` | `crates/tweaker-core/data/tweaks/privacy.json` | 17 пунктов, ids PRIV-01…PRIV-17, тесты каталога |
| Раздел `privacy` в окне | `crates/tweaker-app/src/app.rs` | список 17 пунктов, состояния, предпросмотр, подтверждение, отчёт; PNG `stage3-full.png` |
| Живой прогон | `crates/tweaker-app/examples/tweak_probe.rs` | detect всех 17 пунктов; PRIV-13 apply/revert на этом ПК |
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` |
| Движок и журнал | `crates/tweaker-core/src/{action,adapters,catalog,model,journal,engine,fake}.rs` | 117 тестов; apply/detect/revert/журнал/сбой/восстановление/hosts/задачи |
| Адаптеры Windows | `crates/tweaker-windows/src/{registry,files,process,services,scheduler}.rs` | чтение HKLM, служб и задач; запись HKCU проверена standalone-запуском вне песочницы |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 120 passed, 1 ignored |
| Карта кода | `docs/CODE_MAP.md` | обновлена на этапе 3 |
| Отчёт самопроверки | `%LocalAppData%\WinTweaker\logs\wintweaker.log` | `--selftest` rc=0 |
| Снимок окна | `%TEMP%\wintweaker-run\stage3-full.png` (вне git) | 1200×780, снят самим приложением |
| Release-бинарник | `target/release/wintweaker.exe` (не в git) | см. «Артефакт» выше |

## Журнал этапов

| Дата | Этап | Что сделано | Чем проверено |
|---|---|---|---|
| 2026-11-25 | 0 | Стек Rust + egui, всё бесплатно, только Windows 10/11; решения D-01…D-03, D-07 | `DECISIONS.md` |
| 2026-11-25 | 1 | Каркас workspace, окно с меню 14 разделов, ru/en, лог с ротацией, манифест, `--selftest`, `--smoke`, `--screenshot` | `cargo build`, 44 теста, clippy/fmt, `--selftest` rc=0, PNG 1200×780, release 6,19 МБ и манифест `requireAdministrator` |
| 2026-11-25 | 2 | Движок: `Action`/`TweakDefinition`/`Plan`, 8 трейтов адаптеров, журнал JSONL с двумя фазами и восстановлением, apply/detect/revert/идемпотентность; реальные адаптеры реестра, файлов, процессов; карта кода | `cargo test` 92 passed (core 63, windows 13, app 16), 1 ignored; clippy/fmt чисто; запись в HKCU проверена standalone-запуском вне песочницы |
| 2026-11-25 | 3 (часть 1) | Каталог `privacy` (11 пунктов: реестр и hosts), блок hosts с маркерами, адаптер служб, раздел `privacy` в окне (состояния, предпросмотр, подтверждение, apply/revert, отчёт), проба `tweak_probe`, i18n 114 строк | `cargo test` 109 passed (core 76, windows 16, app 17), 1 ignored; clippy/fmt чисто; живой PRIV-13 apply/revert; PNG `stage3-privacy.png` |
| 2026-11-25 | 3 (часть 2) | Пункты 03/04/05/16/17: действия `TaskSetEnabled`/`TaskSetEnabledMatching` (с `optional` для необязательного ПО), адаптер планировщика на `schtasks.exe`, службы через `IServiceControl`; каталог 17/17, i18n 125 строк, пометка «11 из 17» снята, в окне счётчик пунктов и предупреждение о риске | `cargo test` 120 passed (core 82, windows 21, app 17), 1 ignored; clippy/fmt чисто; живой detect 17 пунктов; сверка префиксов задач с реальным списком; PNG `stage3-full.png`; release 6,74 МБ |
