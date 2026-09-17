//! Оболочка HypeDE: панель и поиск приложений.

#![deny(rust_2018_idioms)]

use std::path::Path;

pub mod desktop;
pub mod launcher;
pub mod panel;
pub mod sound;
pub mod status;
pub mod theme;

/// Идентификатор панели. Композитор узнаёт по нему окно и кладёт его в
/// отведённую полосу вместо общей раскладки.
pub const PANEL_APP_ID: &str = "dev.hypede.Shell";

/// Идентификатор окна поиска приложений.
pub const LAUNCHER_APP_ID: &str = "dev.hypede.Launcher";

/// Готовит запуск программы среды.
///
/// В начало `PATH` добавляется каталог самой оболочки: при запуске из дерева
/// сборки соседние программы лежат рядом, а не в системных каталогах. После
/// установки пакетом это ничего не меняет.
pub fn command(program: &str) -> std::process::Command {
    let mut command = std::process::Command::new(program);

    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let already_listed = std::env::split_paths(&inherited).any(|entry| entry == directory);

        if !already_listed {
            let mut entries = vec![directory];
            entries.extend(std::env::split_paths(&inherited));
            if let Ok(path) = std::env::join_paths(entries) {
                command.env("PATH", path);
            }
        }
    }

    command
}

/// Язык интерфейса для выбора локализованных имён в `.desktop`-файлах.
fn locale() -> String {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".into());

    // `ru_RU.UTF-8` → `ru`: именно так ключ записан в .desktop-файлах.
    raw.split(['_', '.', '@'])
        .next()
        .unwrap_or("en")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Переменные окружения общие для процесса, поэтому тест один и он всё
    /// возвращает на место.
    #[test]
    fn locale_is_reduced_to_the_language_code() {
        let previous = std::env::var_os("LC_ALL");

        std::env::set_var("LC_ALL", "ru_RU.UTF-8");
        assert_eq!(locale(), "ru");

        std::env::set_var("LC_ALL", "en_GB");
        assert_eq!(locale(), "en");

        std::env::set_var("LC_ALL", "C");
        assert_eq!(locale(), "C");

        match previous {
            Some(value) => std::env::set_var("LC_ALL", value),
            None => std::env::remove_var("LC_ALL"),
        }
    }
}
