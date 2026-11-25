# Состояние работ

Обновлено: 2026-11-25 (этап 1 завершён, следующий — этап 2).

## Текущий статус

| Параметр | Значение |
|---|---|
| Этап | 1 — каркас решения (завершён); следующий — 2 (движок apply/detect/revert) |
| Код | `Cargo.toml`, `crates/tweaker-core`, `crates/tweaker-windows`, `crates/tweaker-app` |
| Версия | 0.1.0 |
| Сборка | `cargo build --workspace` — успешно; `cargo build --release -p tweaker-app` — успешно (чистая сборка 1 мин 11 с, повторная 20 с) |
| Артефакт | `target/release/wintweaker.exe` — 6 187 520 байт, sha256 `38C953E9CE9D34E6D79B38F4911A880783094C6C98750351B0532D1A794CFF3C` |
| Тесты | `cargo test --workspace` — 44 passed, 0 failed |
| Проверки | `--selftest` rc=0 (debug и release); `--screenshot` — PNG 1200×780; `clippy -D warnings` и `fmt --check` чисто; манифест release `requireAdministrator` / debug `asInvoker` подтверждён поиском по байтам |
| Запуск | release: от старта до первых кадров ≈ 0,30 с |
| Блокирует | нет; открытые вопросы D-04…D-06 не мешают этапу 2 |

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
| `crates/tweaker-core` | пути `%LocalAppData%\WinTweaker`, логгер с ротацией, каталог разделов (embedded JSON), `Lang`, метаданные продукта |
| `crates/tweaker-windows` | права администратора и перезапуск с правами, язык системы, сведения об ОС (только чтение) |
| `crates/tweaker-app` | окно egui, меню 14 разделов, ru/en, режимы `--selftest`, `--smoke`, `--screenshot` |
| `crates/tweaker-app/i18n` | `ru.json`, `en.json` — по 67 строк, полнота проверяется тестом |
| `crates/tweaker-app/build.rs` | манифест: release — `requireAdministrator`, debug — `asInvoker` |

## План этапов

| Этап | Содержание | Критерий готовности | Статус |
|---|---|---|---|
| 0 | Согласование стека, платной версии, поддержки Windows 7 | решения записаны в `DECISIONS.md` | завершён |
| 1 | Каркас решения: workspace, окно, меню разделов, ru/en, логи, манифест, `--selftest` | сборка, тесты, `--selftest` rc=0 | завершён |
| 2 | Движок: модели `Tweak/Plan/Action`, адаптеры-интерфейсы, журнал JSONL, apply/detect/revert на одном пункте | unit-тесты на фейковых адаптерах зелёные | не начат |
| 3 | Первый раздел каталога — `privacy` (17 пунктов) | все 17 пунктов применяются и откатываются | не начат |
| 4 | Тесты: unit по разделу, смоук и негативные сценарии по `TESTPLAN.md` | отчёт о тестировании | не начат |
| 5 | Остальные разделы настроек (context, ui, system, services, autorun, apps) | по критериям `TESTPLAN.md` | не начат |
| 6 | Очистка и оптимизация (опасные операции, предпросмотр, отмена) | 🔴-пункты только с подтверждением | не начат |
| 7 | Сервисные разделы: sysinfo, backup, профиль, обновления, VirusTotal, about, maintenance | по решению владельца | не начат |
| 8 | Приёмка: чек-лист `TESTPLAN.md`, воспроизводимая сборка, документация | приёмочный отчёт | не начат |

## Артефакты этапа 1

| Артефакт | Где | Проверка |
|---|---|---|
| Исходники каркаса | `crates/` | `cargo build --workspace` rc=0 |
| Зафиксированные зависимости | `Cargo.lock` | `cargo build --locked` (после коммита) |
| Юнит-тесты | `crates/*/src/**` (`#[cfg(test)]`) | 44 passed, 0 failed |
| Отчёт самопроверки | `%LocalAppData%\WinTweaker\logs\wintweaker.log` | `--selftest` rc=0 |
| Снимок окна | `%TEMP%\wintweaker-run\stage1-window.png` (вне git) | 1200×780, снят самим приложением |
| Release-бинарник | `target/release/wintweaker.exe` (не в git) | 6,19 МБ, `--selftest` rc=0, манифест `requireAdministrator` |

## Журнал этапов

| Дата | Этап | Что сделано | Чем проверено |
|---|---|---|---|
| 2026-11-25 | 0 | Стек Rust + egui, всё бесплатно, только Windows 10/11; решения D-01…D-03, D-07 | `DECISIONS.md` |
| 2026-11-25 | 1 | Каркас workspace, окно с меню 14 разделов, ru/en, лог с ротацией, манифест, `--selftest`, `--smoke`, `--screenshot` | `cargo build`, 44 теста, clippy/fmt, `--selftest` rc=0, PNG 1200×780, release 6,19 МБ и манифест `requireAdministrator` |
