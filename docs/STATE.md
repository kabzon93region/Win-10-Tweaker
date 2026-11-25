# Состояние работ

Обновлено: 2026-11-25 (этап 4 завершён: приёмочные тесты раздела `privacy`, отчёт о тестировании).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 4 — тесты: 17 приёмочных тестов каталога, смоук и негативные сценарии, отчёт `TEST_REPORT.md` |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Git | `e2bba3b` (этап 4; этап 3 — `e1237a8`, этап 2 — `1e05d14`, этап 1 — `88de898`) |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно |
| Артефакт | `target/release/wintweaker.exe` — 6 740 480 байт, sha256 `AAA0B000602A010DC0A4558C31D2412EFA986A978AB9BAD12F9B3C48F41374A2` |
| Тесты | `cargo test --workspace` — 143 passed, 1 ignored (app 17, core 82, core-tests 21, windows 23) |
| Проверки | смоук `--selftest`/`--smoke`/`--screenshot` rc=0; живой прогон второй копии; отчёт `docs/TEST_REPORT.md` (31 из 46 проверок пройдено, 15 отложено, 0 провалов); clippy/fmt чисто (`%TEMP%\wintweaker-stage4-lint.txt`) |
| Запуск | debug: 1,78 с до первых кадров, пик памяти 75,4 МБ; release-замер — этап 8 |
| Блокирует | нет; открытые дефекты D-2…D-8 (отмена/фон, экспорт журнала, живые записи, hosts UTF-8, пустой ключ, границы сессии, pending при старте) запланированы на этапы 6–8 |

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
| `crates/tweaker-core` | модель `TweakDefinition/Action/Plan`, каталог разделов (`data/sections.json`) и раздел `privacy` (17 пунктов), движок apply/detect/revert, журнал JSONL с двумя фазами, трейты адаптеров и фейки, действия hosts-блока и задач планировщика, пути, логгер, `Lang`; приёмочные тесты каталога в `tests/privacy_section.rs` |
| `crates/tweaker-windows` | адаптеры реестра, файлов, процессов, служб и планировщика задач (через `schtasks.exe`); права администратора и перезапуск, язык системы, сведения об ОС, защита от второй копии (`single_instance`), системное сообщение (`message`) |
| `crates/tweaker-app` | окно egui, меню 14 разделов, ru/en, режимы `--selftest`, `--smoke`, `--screenshot`, проба `tweak_probe` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 127 строк, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | завершён |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | завершён |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | завершён |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | не начат |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | не начат |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | не начат |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | не начат |

## Артефакты этапов 1–4

| Артефакт | Где | Проверка |
|---|---|---|
| Каталог `privacy` | `crates/tweaker-core/data/tweaks/privacy.json` | 17 пунктов, ids PRIV-01…PRIV-17, тесты каталога |
| Раздел `privacy` в окне | `crates/tweaker-app/src/app.rs` | список 17 пунктов, состояния, предпросмотр, подтверждение, отчёт; PNG `stage3-full.png` |
| Живой прогон | `crates/tweaker-app/examples/tweak_probe.rs` | detect всех 17 пунктов; PRIV-13 apply/revert на этом ПК |
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` |
| Движок и журнал | `crates/tweaker-core/src/{action,adapters,catalog,model,journal,engine,fake}.rs` | 143 теста; apply/detect/revert/журнал/сбой/восстановление/hosts/задачи |
| Приёмочные тесты раздела | `crates/tweaker-core/tests/privacy_section.rs` | 21 тест: полный цикл по каждому из 17 пунктов, ошибки адаптеров, отсутствие служб |
| Адаптеры Windows | `crates/tweaker-windows/src/{registry,files,process,services,scheduler,single_instance,message}.rs` | чтение HKLM, служб и задач; защита от второй копии; запись HKCU проверена standalone-запуском вне песочницы |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 143 passed, 1 ignored |
| Отчёт о тестировании | `docs/TEST_REPORT.md` | 31/46 проверок TESTPLAN пройдено, 15 отложено, 0 провалов |
| Карта кода | `docs/CODE_MAP.md` | обновлена на этапе 4 |
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
| 2026-11-25 | 4 | Приёмочные тесты: по тесту на каждый из 17 пунктов (план, снимок, apply, detect, идемпотентность, откат, повторный откат, журнал), тесты ошибок адаптеров и отсутствия служб; защита от второй копии (мьютекс + сообщение); отчёт `TEST_REPORT.md`; i18n 127 строк | `cargo test` 143 passed (app 17, core 82, core-tests 21, windows 23), 1 ignored; clippy/fmt чисто; `--smoke` rc=0, пик памяти 75,4 МБ, старт 1,78 с; скан release на секреты |
