//! Настройки HypeDE: файл конфигурации, горячие клавиши, пути XDG.
//!
//! Крейт общий для композитора, панели и приложений среды — все читают один и
//! тот же файл и понимают его одинаково.
//!
//! # Пример
//!
//! ```
//! use hype_config::{Config, Action};
//!
//! let config = Config::default();
//! let shortcut = "Super+Q".parse().unwrap();
//! assert_eq!(config.action_for(&shortcut), Some(&Action::CloseWindow));
//! ```

#![deny(rust_2018_idioms)]

mod action;
mod config;
pub mod paths;
mod shortcut;

use std::path::{Path, PathBuf};

pub use action::{Action, Direction, ScreenshotTarget};
pub use config::{
    default_keybinds, AnimSpec, AnimationConfig, Binding, Config, InputConfig, LayoutConfig,
    LayoutMode, PanelConfig, PanelPosition,
};
pub use shortcut::{Key, Mods, Shortcut, ShortcutParseError};

/// Ошибки чтения и записи настроек.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("не удалось определить каталог настроек: не задан ни XDG_CONFIG_HOME, ни HOME")]
    NoConfigDir,
    #[error("ошибка чтения {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("ошибка записи {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("ошибка в файле {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("не удалось сохранить настройки: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Результат загрузки настроек вместе с замечаниями к ним.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    /// Предупреждения проверки: их стоит показать пользователю, но они не
    /// мешают запуску.
    pub warnings: Vec<String>,
    /// Откуда прочитан файл. `None` означает, что файла не было и взяты
    /// настройки по умолчанию.
    pub source: Option<PathBuf>,
}

/// Читает настройки из конкретного файла.
pub fn load_from(path: &Path) -> Result<LoadedConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let config: Config = toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(LoadedConfig {
        warnings: config.validate(),
        config,
        source: Some(path.to_path_buf()),
    })
}

/// Читает настройки из стандартного места.
///
/// Если файла нет, возвращает значения по умолчанию: свежеустановленная среда
/// обязана запускаться без единого файла настроек. А вот сломанный файл —
/// это ошибка: подменить его умолчаниями значило бы молча выбросить всё, что
/// пользователь настроил.
pub fn load() -> Result<LoadedConfig, ConfigError> {
    let path = paths::config_file().ok_or(ConfigError::NoConfigDir)?;

    if !path.exists() {
        let config = Config::default();
        return Ok(LoadedConfig {
            warnings: config.validate(),
            config,
            source: None,
        });
    }

    load_from(&path)
}

/// Записывает настройки в файл, создавая каталоги при необходимости.
pub fn save_to(config: &Config, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let text = toml::to_string_pretty(config)?;

    // Пишем во временный файл рядом и переименовываем: если питание пропадёт
    // посреди записи, пользователь останется со старым конфигом, а не с
    // обрезанным.
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, text).map_err(|source| ConfigError::Write {
        path: temp.clone(),
        source,
    })?;
    std::fs::rename(&temp, path).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Записывает настройки в стандартное место.
pub fn save(config: &Config) -> Result<PathBuf, ConfigError> {
    let path = paths::config_file().ok_or(ConfigError::NoConfigDir)?;
    save_to(config, &path)?;
    Ok(path)
}

/// Выгружает CSS текущей темы туда, откуда его читают GTK-приложения среды.
pub fn export_theme_css(config: &Config) -> Result<PathBuf, ConfigError> {
    let path = paths::generated_css_file().ok_or(ConfigError::NoConfigDir)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&path, config.theme.to_gtk_css()).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hype-config-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saved_config_reads_back_identically() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("config.toml");

        let mut config = Config::default();
        config.theme.name = "Мой стиль".into();
        config.layout.gaps_inner = 16.0;

        save_to(&config, &path).unwrap();
        let loaded = load_from(&path).unwrap();

        assert_eq!(loaded.config, config);
        assert_eq!(loaded.source.as_deref(), Some(path.as_path()));
        assert!(loaded.warnings.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_creates_missing_directories() {
        let dir = temp_dir("mkdir");
        let path = dir.join("a/b/c/config.toml");
        save_to(&Config::default(), &path).unwrap();
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = temp_dir("atomic");
        let path = dir.join("config.toml");
        save_to(&Config::default(), &path).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "остался мусор: {leftovers:?}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_broken_file_is_an_error_not_a_silent_reset() {
        let dir = temp_dir("broken");
        let path = dir.join("config.toml");
        std::fs::write(&path, "это [не TOML").unwrap();

        let err = load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
        assert!(err.to_string().contains("config.toml"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_reported_with_its_path() {
        let err = load_from(Path::new("/nope/not/here/config.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
        assert!(err.to_string().contains("/nope/not/here/config.toml"));
    }

    #[test]
    fn warnings_come_back_with_the_loaded_config() {
        let dir = temp_dir("warnings");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            r#"
            [[keybind]]
            keys = "Super+Q"
            do = "close-window"

            [[keybind]]
            keys = "Super+Q"
            do = "quit"
        "#,
        )
        .unwrap();

        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("Super+Q"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
