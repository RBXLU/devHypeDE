//! Боковая панель: домашний каталог и пользовательские папки.

use std::path::{Path, PathBuf};

/// Запись в боковой панели.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
    pub icon: &'static str,
}

/// Стандартные папки пользователя в порядке показа.
///
/// Имена берутся из `~/.config/user-dirs.dirs` — того же файла, которым
/// пользуются GNOME и KDE, поэтому «Загрузки» окажутся там же, куда их кладёт
/// браузер. Отсутствующие папки в список не попадают: пустой пункт, ведущий в
/// никуда, хуже его отсутствия.
pub fn standard_places(home: &Path) -> Vec<Place> {
    let user_dirs = read_user_dirs(home);

    let mut places = vec![Place {
        label: "Домашняя папка".into(),
        path: home.to_path_buf(),
        icon: "user-home",
    }];

    let candidates: [(&str, &str, &str); 6] = [
        ("XDG_DESKTOP_DIR", "Рабочий стол", "user-desktop"),
        ("XDG_DOWNLOAD_DIR", "Загрузки", "folder-download"),
        ("XDG_DOCUMENTS_DIR", "Документы", "folder-documents"),
        ("XDG_PICTURES_DIR", "Изображения", "folder-pictures"),
        ("XDG_MUSIC_DIR", "Музыка", "folder-music"),
        ("XDG_VIDEOS_DIR", "Видео", "folder-videos"),
    ];

    for (key, label, icon) in candidates {
        let Some(path) = user_dirs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
        else {
            continue;
        };
        if path != home && path.is_dir() {
            places.push(Place {
                label: label.into(),
                path,
                icon,
            });
        }
    }

    places.push(Place {
        label: "Файловая система".into(),
        path: PathBuf::from("/"),
        icon: "drive-harddisk",
    });

    places
}

/// Читает `user-dirs.dirs` и разворачивает пути.
fn read_user_dirs(home: &Path) -> Vec<(String, PathBuf)> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));

    let Ok(text) = std::fs::read_to_string(config.join("user-dirs.dirs")) else {
        return Vec::new();
    };
    parse_user_dirs(&text, home)
}

/// Разбирает содержимое `user-dirs.dirs`.
pub fn parse_user_dirs(text: &str, home: &Path) -> Vec<(String, PathBuf)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim().to_string();
            let value = value.trim().trim_matches('"');

            // В файле пути записаны как "$HOME/Загрузки".
            let path = match value.strip_prefix("$HOME/") {
                Some(rest) => home.join(rest),
                None if value == "$HOME" => home.to_path_buf(),
                None => PathBuf::from(value),
            };
            Some((key, path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_are_parsed_and_expanded() {
        let home = Path::new("/home/максим");
        let text = r#"
            # Создано xdg-user-dirs-update
            XDG_DOWNLOAD_DIR="$HOME/Загрузки"
            XDG_MUSIC_DIR="$HOME/Музыка"
            XDG_DESKTOP_DIR="$HOME"
            XDG_PUBLICSHARE_DIR="/srv/общее"
        "#;

        let dirs = parse_user_dirs(text, home);
        assert_eq!(dirs.len(), 4);
        assert_eq!(
            dirs.iter().find(|(k, _)| k == "XDG_DOWNLOAD_DIR").unwrap().1,
            home.join("Загрузки")
        );
        assert_eq!(
            dirs.iter().find(|(k, _)| k == "XDG_DESKTOP_DIR").unwrap().1,
            home
        );
        assert_eq!(
            dirs.iter().find(|(k, _)| k == "XDG_PUBLICSHARE_DIR").unwrap().1,
            Path::new("/srv/общее")
        );
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        assert!(parse_user_dirs("# только комментарий\n\n   \n", Path::new("/home/u")).is_empty());
    }

    #[test]
    fn a_malformed_line_does_not_break_the_rest() {
        let dirs = parse_user_dirs(
            "мусор без равно\nXDG_MUSIC_DIR=\"$HOME/Музыка\"",
            Path::new("/home/u"),
        );
        assert_eq!(dirs.len(), 1);
    }

    #[test]
    fn the_list_always_has_home_and_the_filesystem_root() {
        let places = standard_places(Path::new("/home/несуществующий"));
        assert_eq!(places.first().unwrap().path, Path::new("/home/несуществующий"));
        assert_eq!(places.last().unwrap().path, Path::new("/"));
    }
}
