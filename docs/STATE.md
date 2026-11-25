# Состояние работ

Обновлено: 2026-11-25 (этап 5, часть 3: раздел «Система», 18 пунктов; добавлены обратимые внешние команды).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 5 — остальные разделы: части 1–3 (`services` 24/24, `ui` 17/17, `system` 18/18); дальше `context`, `autorun`, `apps` |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Git | `03138aa` (этап 5, часть 2); часть 3 — коммит после аудита |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно |
| Артефакт | `target/release/wintweaker.exe` — 7 012 352 байт, sha256 `F6D867DC6C536357B447D9CBCDEAB8DD8F0378D5DFFC4EF288372F165448D29C` |
| Тесты | `cargo test --workspace` — 230 passed, 1 ignored (app 20, core 96, core-tests 21+26+18+22, windows 27) |
| Проверки | clippy/fmt чисто; снимки `services.png`, `ui.png`, `system.png` — 24, 17 и 18 пунктов с реальными состояниями; `--selftest` rc=0 |
| Запуск | debug: ≈1,8 с до первых кадров; release-замер — этап 8 |
| Блокирует | нет; открытые дефекты D-2…D-8 прежние |

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
| `crates/tweaker-core` | модель `TweakDefinition/Action/Plan`, каталог разделов (`data/sections.json`) и разделы `privacy` (17), `services` (24), `ui` (17), движок apply/detect/revert, журнал JSONL с двумя фазами, трейты адаптеров и фейки, действия hosts-блока, задач, удаления ключей и записи бинарных файлов, пути, логгер, `Lang`; приёмочные тесты каталога в `tests/` |
| `crates/tweaker-windows` | адаптеры реестра, файлов, процессов, служб и планировщика задач (через `schtasks.exe`); права администратора и перезапуск, язык системы, сведения об ОС, защита от второй копии (`single_instance`), системное сообщение (`message`) |
| `crates/tweaker-app` | окно egui, меню 14 разделов, универсальный экран каталога (любой раздел с данными), ru/en, режимы `--selftest`, `--smoke`, `--screenshot`, `--section <id>`, проба `tweak_probe` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 245 строк, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | завершён |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | завершён |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | завершён |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | в работе: `services` — 24/24, `ui` — 17/17, `system` — 18/18; остались `context`, `autorun`, `apps` |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | не начат |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | не начат |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | не начат |

## Артефакты этапов 1–5

| Артефакт | Где | Проверка |
|---|---|---|
| Каталог `privacy` | `crates/tweaker-core/data/tweaks/privacy.json` | 17 пунктов, ids PRIV-01…PRIV-17, тесты каталога |
| Каталог `services` | `crates/tweaker-core/data/tweaks/services.json` | 24 пункта, ids SVC-01…SVC-24, стоп + `Start=4`, откат по снимку |
| Каталог `ui` | `crates/tweaker-core/data/tweaks/ui.json` | 17 пунктов, ids UI-01…UI-17, реестр, удаление ключей, пустая иконка для стрелок ярлыков |
| Каталог `system` | `crates/tweaker-core/data/tweaks/system.json` | 18 пунктов, ids SYS-01…SYS-AZ, реестр, службы, задачи, обратимые команды netsh/PowerShell |
| Универсальный экран каталога | `crates/tweaker-app/src/app.rs` | любой раздел с данными: список, состояния, риск, предпросмотр, подтверждение, apply/revert; `--section <id>` |
| Раздел `privacy` в окне | `crates/tweaker-app/src/app.rs` | список 17 пунктов, состояния, предпросмотр, подтверждение, отчёт; PNG `stage3-full.png` |
| Раздел `services` в окне | `crates/tweaker-app/src/app.rs` | 24 пункта, реальные состояния, риск, пометка перезапуска; PNG `services.png` |
| Живой прогон | `crates/tweaker-app/examples/tweak_probe.rs` | detect всех 17 пунктов; PRIV-13 apply/revert на этом ПК |
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` |
| Движок и журнал | `crates/tweaker-core/src/{action,adapters,catalog,model,journal,engine,fake}.rs` | apply/detect/revert/журнал/сбой/восстановление/hosts/задачи |
| Приёмочные тесты разделов | `crates/tweaker-core/tests/{common,privacy_section,services_section,ui_section,system_section}.rs` | 21 + 26 + 18 + 22 теста: полный цикл по каждому пункту, ошибки адаптеров, отсутствие служб |
| Адаптеры Windows | `crates/tweaker-windows/src/{registry,files,process,services,scheduler,single_instance,message}.rs` | чтение HKLM, служб и задач; защита от второй копии; запись HKCU проверена standalone-запуском вне песочницы |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 230 passed, 1 ignored |
| Отчёт о тестировании | `docs/TEST_REPORT.md` | 31/46 проверок TESTPLAN пройдено, 15 отложено, 0 провалов (этап 4) |
| Карта кода | `docs/CODE_MAP.md` | обновлена на этапе 5, часть 1 |
| Отчёт самопроверки | `%LocalAppData%\WinTweaker\logs\wintweaker.log` | `--selftest` rc=0 |
| Снимки окна | `%TEMP%\wintweaker-run\{services,ui,system}.png` (вне git) | 1200×780, разделы `services`, `ui`, `system`, сняты самим приложением |
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
| 2026-11-25 | 5 (часть 1) | Универсальный экран каталога вместо жёстко привязанного `privacy`: загрузка любого раздела по id, переключение в меню, флаг `--section <id>`; раздел `services` — 24 пункта (стоп + `Start=4`, откат по снимку); i18n 175 строк; тесты `common`/`services_section` | `cargo test` 178 passed (app 20, core 87, core-tests 21+26, windows 24), 1 ignored; clippy/fmt чисто; снимок `services.png` — 24 пункта с реальными состояниями; `--section` проверен |
| 2026-11-25 | 5 (часть 2) | Новые действия `RegistryDeleteKey` (снимок всего дерева ключа) и `FileWriteBytes` (бинарный файл); раздел `ui` — 17 пунктов (цвета, шрифты, кнопки, папки «Этот компьютер», стрелки ярлыков, Проводник, классическое меню, прокрутка); пустая иконка 16×16 в данных; i18n 209 строк | `cargo test` 204 passed (app 20, core 92, core-tests 21+26+18, windows 27), 1 ignored; снимок `ui.png` — 17 пунктов с реальными состояниями |
| 2026-11-25 | 5 (часть 3) | Действие `ProcessRunReversible` (команда + обратная команда, состояние «применено» по записи журнала); раздел `system` — 18 пунктов: Уведомления, IE, Защитник/SmartScreen, UAC, раскладка, электропитание, диагностика памяти, Teredo/IPv6, Cortana, IE, ярлыки, кэш ФС, задержка запуска, быстрый доступ, смайлики, просмотрщик фото, OneDrive, PAC-прокси; i18n 245 строк | `cargo test` 230 passed (app 20, core 96, core-tests 21+26+18+22, windows 27), 1 ignored; снимок `system.png` — 18 пунктов |
