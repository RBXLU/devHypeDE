//! Где HypeDE хранит свои файлы.
//!
//! Всё подчиняется спецификации XDG Base Directory: настройки — в
//! `$XDG_CONFIG_HOME`, производные файлы — в `$XDG_CACHE_HOME`, сокеты — в
//! `$XDG_RUNTIME_DIR`. Никаких точечных каталогов прямо в домашней папке.

use std::path::PathBuf;

/// Имя каталога среды внутри стандартных путей XDG.
pub const APP_DIR: &str = "hypede";

/// Каталог настроек, обычно `~/.config/hypede`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join(APP_DIR));
    }
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config").join(APP_DIR))
}

/// Основной файл настроек.
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.toml"))
}

/// Каталог производных файлов, обычно `~/.cache/hypede`.
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join(APP_DIR));
    }
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".cache").join(APP_DIR))
}

/// Сгенерированный из темы CSS, который подхватывают GTK-приложения среды.
///
/// Файл производный: его переписывает композитор при каждой смене темы,
/// поэтому ему место в кэше, а не в настройках.
pub fn generated_css_file() -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join("theme.css"))
}

/// Сокет управления композитором.
///
/// Имя включает `$WAYLAND_DISPLAY`, чтобы два сеанса на одной машине не дрались
/// за один файл.
pub fn control_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    Some(PathBuf::from(runtime).join(format!("hypede-{display}.sock")))
}

/// Каталоги, где лежат `.desktop`-файлы приложений, в порядке приоритета.
pub fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.home_dir().join(".local/share/applications"));
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());

    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir).join("applications"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Переменные окружения — общее состояние процесса, поэтому тесты, которые
    /// их трогают, идут под одним замком и всегда возвращают всё на место.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn config_path_follows_xdg_config_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::set("XDG_CONFIG_HOME", "/tmp/hype-test-config");
        assert_eq!(
            config_file().unwrap(),
            PathBuf::from("/tmp/hype-test-config/hypede/config.toml")
        );
    }

    #[test]
    fn empty_xdg_variable_is_treated_as_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::set("XDG_CONFIG_HOME", "");
        let path = config_dir().unwrap();
        assert!(
            path.ends_with(".config/hypede"),
            "ожидался запасной путь, получено {path:?}"
        );
    }

    #[test]
    fn socket_name_includes_the_wayland_display() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _runtime = EnvGuard::set("XDG_RUNTIME_DIR", "/run/user/1000");
        let _display = EnvGuard::set("WAYLAND_DISPLAY", "wayland-3");
        assert_eq!(
            control_socket().unwrap(),
            PathBuf::from("/run/user/1000/hypede-wayland-3.sock")
        );
    }

    #[test]
    fn application_dirs_put_the_user_first_and_include_the_system() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _data_home = EnvGuard::set("XDG_DATA_HOME", "/home/test/.local/share");
        let _data_dirs =
            EnvGuard::set("XDG_DATA_DIRS", "/usr/share:/var/lib/flatpak/exports/share");

        let dirs = application_dirs();
        assert_eq!(
            dirs[0],
            PathBuf::from("/home/test/.local/share/applications")
        );
        assert!(dirs.contains(&PathBuf::from(
            "/var/lib/flatpak/exports/share/applications"
        )));
    }

    #[test]
    fn application_dirs_fall_back_to_the_standard_system_paths() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _data_dirs = EnvGuard::set("XDG_DATA_DIRS", "");
        let dirs = application_dirs();
        assert!(dirs.contains(&PathBuf::from("/usr/share/applications")));
    }
}
