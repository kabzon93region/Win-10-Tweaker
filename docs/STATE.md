# Состояние работ

Обновлено: 2026-11-26 (этап 8: итоговая приёмка; автоматические проверки и интерфейс пройдены, ВМ и живые опасные операции отложены).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 8 — приёмка: автопроверки и интерфейс пройдены; ВМ, UAC и живые опасные операции отложены (см. TEST_REPORT) |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Git | `a713a3d` (этап 8) |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно |
| Артефакт | `target/release/wintweaker.exe` — 7 438 336 байт, sha256 `BBD3E4C4062E83E65CCD4DFFC57D154B7C21D785BEF2AAB6A46327D11B59524A` |
| Тесты | `cargo test --workspace` — 331 passed, 1 ignored (app 24, core 105, core-tests 25+4+18+18+6+5+21+26+22+18, windows 39) |
| Проверки | clippy/fmt чисто; снимки `services.png`, `ui.png`, `system.png`, `context.png`, `autorun3.png`, `apps2.png`, `cleaner.png`, `optimization.png`, `sysinfo.png`, `backup.png`, `maintenance.png`; sysinfo — реальный отчёт с копированием и сохранением; `--selftest` rc=0 |
| Запуск | release `--selftest` 414 мс, `--smoke` 3,08 с (с удержанием окна 2,5 с); замер под UAC не выполнялся |
| Блокирует | нет; D-2 и D-8 закрыты на этапе 6, открыты D-3…D-7 |

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
| `crates/tweaker-windows` | адаптеры реестра, файлов, процессов, служб, планировщика задач и автозагрузки; права администратора и перезапуск, язык системы, сведения об ОС, защита от второй копии, системное сообщение |
| `crates/tweaker-app` | окно egui, меню 14 разделов, универсальный экран каталога (любой раздел с данными), ru/en, режимы `--selftest`, `--smoke`, `--screenshot`, `--section <id>`, проба `tweak_probe` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 392 строки, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | завершён |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | завершён |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | завершён |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | завершён: `services` 24/24, `ui` 17/17, `system` 18/18, `context` 15/17, `autorun` (5 источников), `apps` 24/24 |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | завершён с исключениями: `cleaner` 16/18, `optimization` 4/6, см. D-39 |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | частично: `sysinfo` (отчёт), `backup` 4/4, `maintenance` 6/6, `about` (сведения); профиль/обновления/VT — D-46…D-48 |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | завершён: автопроверки/интерфейс пройдены, ВМ и живые опасные операции отложены |

## Артефакты этапов 1–5

| Артефакт | Где | Проверка |
|---|---|---|
| Каталог `privacy` | `crates/tweaker-core/data/tweaks/privacy.json` | 17 пунктов, ids PRIV-01…PRIV-17, тесты каталога |
| Каталог `services` | `crates/tweaker-core/data/tweaks/services.json` | 24 пункта, ids SVC-01…SVC-24, стоп + `Start=4`, откат по снимку |
| Каталог `ui` | `crates/tweaker-core/data/tweaks/ui.json` | 17 пунктов, ids UI-01…UI-17, реестр, удаление ключей, пустая иконка для стрелок ярлыков |
| Каталог `system` | `crates/tweaker-core/data/tweaks/system.json` | 18 пунктов, ids SYS-01…SYS-AZ, реестр, службы, задачи, обратимые команды netsh/PowerShell |
| Каталог `context` | `crates/tweaker-core/data/tweaks/context.json` | 15 пунктов (CTX-07/08 исключены), пункты меню, скрипты, правила брандмауэра по клику |
| Раздел `autorun` | `crates/tweaker-windows/src/startup.rs`, `crates/tweaker-app/src/app.rs` | 5 источников (Run HKCU/HKLM/HKLM32, папка, задачи), переключение и удаление через журналируемые действия |
| Каталог `apps` | `crates/tweaker-core/data/tweaks/apps.json` | 24 пункта, ids APP-01…APP-24, действия `AppxRemove`, снимок пакета и манифеста |
| Каталог `cleaner` | `crates/tweaker-core/data/tweaks/cleaner.json` | 16 пунктов (CLN-17/START исключены, D-39), `CleanPaths`/`RegistryDeleteKey`/команды, опасные пункты помечены критическими |
| Каталог `optimization` | `crates/tweaker-core/data/tweaks/optimization.json` | 4 пункта (OPT-05/06 исключены, D-39), `compact`/`dism` через обратимые и обычные команды |
| Каталог `backup` | `crates/tweaker-core/data/tweaks/backup.json` | 4 пункта: точка восстановления, rstrui, экспорт/импорт драйверов через документированные команды |
| Каталог `maintenance` | `crates/tweaker-core/data/tweaks/maintenance.json` | 6 пунктов FIX-01…FIX-06: bcdedit, MSI Extract, Edge prelaunch, cmd в меню, NumLock, битые ярлыки |
| Системная информация | `crates/tweaker-windows/src/os.rs`, `crates/tweaker-app/src/app.rs` | CIM-отчёт (ОС, CPU, RAM, GPU, мониторы, диски, сеть), копирование и сохранение в `config\sysinfo.txt` |
| Фоновое выполнение | `crates/tweaker-app/src/app.rs` (`ApplyJob`, `start_apply_jobs`, `poll_job`) | прогресс по действиям, отмена между действиями, отчёт по каждому пункту, «освобождено N» |
| Восстановление после сбоя | `crates/tweaker-app/src/app.rs` (`pending_recovery`, `revert_pending_ops`) | баннер о незавершённых операциях при старте и откат по кнопке |
| Универсальный экран каталога | `crates/tweaker-app/src/app.rs` | любой раздел с данными: список, состояния, риск, предпросмотр, подтверждение, apply/revert; `--section <id>` |
| Раздел `privacy` в окне | `crates/tweaker-app/src/app.rs` | список 17 пунктов, состояния, предпросмотр, подтверждение, отчёт; PNG `stage3-full.png` |
| Раздел `services` в окне | `crates/tweaker-app/src/app.rs` | 24 пункта, реальные состояния, риск, пометка перезапуска; PNG `services.png` |
| Живой прогон | `crates/tweaker-app/examples/tweak_probe.rs` | detect всех 17 пунктов; PRIV-13 apply/revert на этом ПК |
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` |
| Движок и журнал | `crates/tweaker-core/src/{action,adapters,catalog,model,journal,engine,fake}.rs` | apply/detect/revert/журнал/сбой/восстановление/hosts/задачи |
| Приёмочные тесты разделов | `crates/tweaker-core/tests/{common,*_section}.rs` | 163 теста в 10 файлах: apps 25, backup 4, cleaner 18, context 18, maintenance 6, optimization 5, privacy 21, services 26, system 22, ui 18 |
| Адаптеры Windows | `crates/tweaker-windows/src/{registry,files,process,services,scheduler,startup,single_instance,message}.rs` | чтение HKLM, служб, задач и автозагрузки; защита от второй копии; запись HKCU проверена standalone-запуском вне песочницы |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 168 passed, 1 ignored (app 24, core 105, windows 39) |
| Отчёт о тестировании | `docs/TEST_REPORT.md` | итоговая приёмка этапа 8: 331 тест, selftest/smoke rc=0, воспроизводимая сборка; ВМ/UAC/живые опасные операции отложены |
| Карта кода | `docs/CODE_MAP.md` | обновлена на этапе 7 |
| Отчёт самопроверки | `%LocalAppData%\WinTweaker\logs\wintweaker.log` | `--selftest` rc=0 |
| Снимки окна | `%TEMP%\wintweaker-run\{services,ui,system,context,autorun3,apps2,cleaner,optimization,sysinfo,backup,maintenance}.png` (вне git) | 1200×780, сняты самим приложением |
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
| 2026-11-25 | 5 (часть 4) | Раздел `context` — 15 пунктов: владение, меню «Компьютер», cmd, ShellNew, скрытые файлы, копирование картинки, брандмауэр по клику, очистка папки, символические ссылки, .nfo, WMP, задержка меню, лишние пункты, «Создать», персонализация; CTX-07/08 исключены D-31; i18n 275 строк | `cargo test` 249 passed (app 20, core 97, core-tests 21+26+18+22+18, windows 27), 1 ignored; снимок `context.png` — 15 пунктов |
| 2026-11-25 | 5 (часть 5) | Раздел `autorun`: адаптер `IStartup` (Run HKCU/HKLM/HKLM32, папка «Автозагрузка», задачи вне `\Microsoft\Windows`), состояние по `StartupApproved`; в окне список, переключение через движок (`RegistrySet` binary / `TaskSetEnabled`), удаление через `RegistryDelete`/`FileDeleteAny` с подтверждением, открытие расположения; i18n 295 строк | `cargo test` 258 passed (app 23, core 98, core-tests 21+26+18+22+18, windows 32), 1 ignored; живой список — 37 записей, включая HKLM Run; снимок `autorun3.png` |
| 2026-11-26 | 5 (часть 6) | Раздел `apps`: 24 плитки, действие `AppxRemove` (снимок семейства и манифеста, откат регистрацией), адаптер `IAppx` через реестр пакетов пользователя и `Remove-AppxPackage`/`Add-AppxPackage`, экран с плитками, состояниями «установлено/удалено», подтверждением, «Удалить все»/«Восстановить все», подсказкой про Магазин; i18n 337 строк | `cargo test` 289 passed (app 24, core 100, core-tests 25+21+26+18+22+18, windows 35), 1 ignored; clippy/fmt чисто; снимок `apps2.png` — 24 плитки, 6 установленных; `--section apps` rc=0 |
| 2026-11-26 | 6 | Движок: `apply_with` с отменой между действиями и `ApplyReport.cancelled/freed_bytes/skipped_files`; действие `CleanPaths` со снимком `None`, измерением объёма и пропуском занятых файлов; фоновое выполнение в UI с прогресс-баром и кнопкой отмены; баннер незавершённых операций при старте; разделы `cleaner` (16 пунктов) и `optimization` (4 пункта); i18n 373 строки | `cargo test` 319 passed (app 24, core 105, core-tests 25+18+5+21+26+22+18, windows 37), 1 ignored; clippy/fmt чисто; снимки `cleaner.png`, `optimization.png`; исключения CLN-17/START, OPT-05/06 — D-39 |
| 2026-11-26 | 7 (часть 1) | `sysinfo`: отчёт через CIM с UTF-8, копирование и сохранение без отправки; каталоги `backup` (4) и `maintenance` (6) на существующих действиях; about со сведениями и путями; профиль, обновления и VirusTotal отложены до ответов D-46…D-48; i18n 392 строки | `cargo test` 331 passed (app 24, core 105, core-tests 25+4+18+18+6+5+21+26+22+18, windows 39), 1 ignored; clippy/fmt чисто; снимки `sysinfo.png`, `backup.png`, `maintenance.png` |
| 2026-11-26 | 8 | Итоговая приёмка: 331 тест, clippy/fmt, `--locked` сборка с тем же sha256, `--selftest` 414 мс, `--smoke` 3,08 с, скан release на секреты, снимки всех экранов; `TEST_REPORT.md` дополнен итоговым разделом; ВМ, UAC и живые опасные операции отложены | 331 passed, 1 ignored; release 7 438 336 байт, sha256 `BBD3E4C4062E83E65CCD4DFFC57D154B7C21D785BEF2AAB6A46327D11B59524A` |
