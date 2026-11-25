# Состояние работ

Обновлено: 2026-11-25 (этап 3 в работе: раздел `privacy`, часть 1 — 11 из 17 пунктов).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 3 — раздел `privacy` (в работе: 11 из 17 пунктов; остались службы и задачи) |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Git | `061b853` (решения этапа 3; этап 2 — `1e05d14`, этап 1 — `88de898`); срез этапа 3, часть 1 — ещё не закоммичен |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно (чистая сборка 1 мин 11 с, повторная 20 с) |
| Артефакт | `target/release/wintweaker.exe` — 6 671 872 байт, sha256 `B9673D03E6BA2AD107657C92A0C07EF0FDE7DDDAB23B9EC94F36D5C80B7A16AD` |
| Тесты | `cargo test --workspace` — 109 passed, 1 ignored (app 17, core 76, windows 16) |
| Проверки | `--selftest` rc=0; `--screenshot` — PNG 1200×780 с разделом `privacy`; clippy/fmt чисто; живая проверка PRIV-13 (apply → значение есть, detect Applied → revert → значения нет, detect NotApplied); манифест release `requireAdministrator` |
| Запуск | release: от старта до первых кадров ≈ 0,30 с |
| Блокирует | нет; открытые вопросы D-04…D-06 не мешают этапу 3 |

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

`Cargo.lock` включён в поставку (первый коммит этапа): версии зависимостей зафиксированы; побайтовая воспроизводимость сборки проверяется на этапе 8.

## Структура решения

| Путь | Что внутри |
|---|---|
| `Cargo.toml` | workspace из трёх crates, resolver 3, edition 2024 |
| `crates/tweaker-core` | модель `TweakDefinition/Action/Plan`, каталог разделов (`data/tweaks/privacy.json`), движок apply/detect/revert, журнал JSONL с двумя фазами, трейты адаптеров и фейки, действия hosts-блока, пути, логгер, `Lang` |
| `crates/tweaker-windows` | адаптеры реестра, файлов, процессов и служб; права администратора и перезапуск, язык системы, сведения об ОС |
| `crates/tweaker-app` | окно egui, меню 14 разделов, ru/en, режимы `--selftest`, `--smoke`, `--screenshot` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 114 строк, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | завершён |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | в работе: 11/17 (реестр и hosts); службы и задачи — часть 2 |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | не начат |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | не начат |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | не начат |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | не начат |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | не начат |

## Артефакты этапов 1–3

| Артефакт | Где | Проверка |
|---|---|---|
| Каталог `privacy` | `crates/tweaker-core/data/tweaks/privacy.json` | 11 пунктов, ids PRIV-01…PRIV-15, тесты каталога |
| Раздел `privacy` в окне | `crates/tweaker-app/src/app.rs` | список, состояния, предпросмотр, подтверждение, отчёт; PNG `stage3-privacy.png` |
| Живой прогон | `crates/tweaker-app/examples/tweak_probe.rs` | PRIV-13: apply/revert на этом ПК, реестр проверен снаружи |
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` |
| Движок и журнал | `crates/tweaker-core/src/{action,adapters,catalog,model,journal,engine,fake}.rs` | 109 тестов, apply/detect/revert/журнал/сбой/восстановление/hosts |
| Адаптеры Windows | `crates/tweaker-windows/src/{registry,files,process,services}.rs` | чтение HKLM и служб; запись HKCU проверена standalone-запуском вне песочницы |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 109 passed, 1 ignored |
| Карта кода | `docs/CODE_MAP.md` | обновлена на этапе 3 |
| Отчёт самопроверки | `%LocalAppData%\WinTweaker\logs\wintweaker.log` | `--selftest` rc=0 |
| Снимок окна | `%TEMP%\wintweaker-run\stage1-window.png` (вне git) | 1200×780, снят самим приложением |
| Release-бинарник | `target/release/wintweaker.exe` (не в git) | см. «Артефакт» выше |

## Журнал этапов

| Дата | Этап | Что сделано | Чем проверено |
|---|---|---|---|
| 2026-11-25 | 0 | Стек Rust + egui, всё бесплатно, только Windows 10/11; решения D-01…D-03, D-07 | `DECISIONS.md` |
| 2026-11-25 | 1 | Каркас workspace, окно с меню 14 разделов, ru/en, лог с ротацией, манифест, `--selftest`, `--smoke`, `--screenshot` | `cargo build`, 44 теста, clippy/fmt, `--selftest` rc=0, PNG 1200×780, release 6,19 МБ и манифест `requireAdministrator` |
| 2026-11-25 | 2 | Движок: `Action`/`TweakDefinition`/`Plan`, 8 трейтов адаптеров, журнал JSONL с двумя фазами и восстановлением, apply/detect/revert/идемпотентность; реальные адаптеры реестра, файлов, процессов; карта кода | `cargo test` 92 passed (core 63, windows 13, app 16), 1 ignored; clippy/fmt чисто; запись в HKCU проверена standalone-запуском вне песочницы |
| 2026-11-25 | 3 (часть 1) | Каталог `privacy` (11 пунктов: реестр и hosts), блок hosts с маркерами, адаптер служб, раздел `privacy` в окне (состояния, предпросмотр, подтверждение, apply/revert, отчёт), проба `tweak_probe`, i18n 114 строк | `cargo test` 109 passed (core 76, windows 16, app 17), 1 ignored; clippy/fmt чисто; живой PRIV-13 apply/revert; PNG `stage3-privacy.png` |
