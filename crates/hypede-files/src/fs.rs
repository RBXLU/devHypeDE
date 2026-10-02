//! Чтение каталогов, сортировка и человеческие подписи.
//!
//! Весь этот код не знает про GTK и проверяется обычными тестами на настоящих
//! файлах во временном каталоге.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Что это за элемент.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Каталоги идут первыми, поэтому Directory меньше остальных.
    Directory,
    File,
    /// Ссылка, ведущая в никуда.
    BrokenLink,
}

/// Элемент каталога.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Размер в байтах. Для каталогов — 0: считать размер каталога дорого.
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Имя начинается с точки.
    pub hidden: bool,
    /// Элемент является символической ссылкой.
    pub symlink: bool,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Directory
    }

    /// Расширение в нижнем регистре, без точки.
    pub fn extension(&self) -> Option<String> {
        if self.is_dir() {
            return None;
        }
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
    }

    /// Имя значка из стандартной темы значков.
    pub fn icon_name(&self) -> &'static str {
        match self.kind {
            Kind::Directory => return "folder",
            Kind::BrokenLink => return "dialog-warning",
            Kind::File => {}
        }

        let Some(extension) = self.extension() else {
            return "text-x-generic";
        };

        match extension.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" | "heic" => {
                "image-x-generic"
            }
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" => "video-x-generic",
            "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" => "audio-x-generic",
            "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" => "package-x-generic",
            "pdf" => "x-office-document",
            "rs" | "py" | "sh" | "c" | "cpp" | "h" | "go" | "js" | "ts" | "lua" | "java" => {
                "text-x-script"
            }
            "toml" | "json" | "yaml" | "yml" | "ini" | "conf" | "cfg" => "text-x-generic-template",
            "desktop" => "application-x-executable",
            _ => "text-x-generic",
        }
    }

    /// Подпись размера: для каталогов её нет.
    pub fn size_label(&self) -> String {
        if self.is_dir() {
            "—".to_string()
        } else {
            human_size(self.size)
        }
    }
}

/// По какому полю сортировать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
    Modified,
    Kind,
}

/// Правило сортировки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sort {
    pub by: SortBy,
    pub descending: bool,
}

impl Sort {
    pub fn by(by: SortBy) -> Self {
        Self {
            by,
            descending: false,
        }
    }

    /// Переключает поле сортировки, а при повторном выборе того же поля —
    /// направление. Так ведут себя заголовки столбцов во всех файловых
    /// менеджерах, и ломать эту привычку незачем.
    pub fn toggle(self, by: SortBy) -> Self {
        if self.by == by {
            Self {
                by,
                descending: !self.descending,
            }
        } else {
            Self::by(by)
        }
    }
}

/// Ошибка чтения каталога.
#[derive(Debug)]
pub struct ReadError {
    pub path: PathBuf,
    pub source: std::io::Error,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self.source.kind() {
            std::io::ErrorKind::NotFound => "каталог не найден".to_string(),
            std::io::ErrorKind::PermissionDenied => "нет доступа".to_string(),
            _ => self.source.to_string(),
        };
        write!(f, "{}: {reason}", self.path.display())
    }
}

impl std::error::Error for ReadError {}

/// Читает каталог и возвращает отсортированный список.
///
/// Элементы, которые не удалось прочитать (исчезли между чтением каталога и
/// запросом свойств), пропускаются: каталог должен открыться даже если один
/// файл в нём испортился.
pub fn list_dir(path: &Path, show_hidden: bool, sort: Sort) -> Result<Vec<Entry>, ReadError> {
    let iter = fs::read_dir(path).map_err(|source| ReadError {
        path: path.to_path_buf(),
        source,
    })?;

    let mut entries: Vec<Entry> = iter
        .filter_map(Result::ok)
        .filter_map(|dir_entry| entry_from(&dir_entry.path()))
        .filter(|entry| show_hidden || !entry.hidden)
        .collect();

    sort_entries(&mut entries, sort);
    Ok(entries)
}

/// Собирает сведения об одном пути.
pub fn entry_from(path: &Path) -> Option<Entry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    // symlink_metadata не идёт по ссылке — так видно саму ссылку.
    let link_meta = fs::symlink_metadata(path).ok()?;
    let symlink = link_meta.file_type().is_symlink();

    // Для отображения важны свойства цели ссылки, а не самой ссылки.
    let meta = fs::metadata(path).ok();

    let kind = match &meta {
        Some(meta) if meta.is_dir() => Kind::Directory,
        Some(_) => Kind::File,
        // Ссылка есть, а цели нет.
        None if symlink => Kind::BrokenLink,
        None => return None,
    };

    Some(Entry {
        hidden: name.starts_with('.'),
        name,
        path: path.to_path_buf(),
        kind,
        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modified: meta.as_ref().and_then(|m| m.modified().ok()),
        symlink,
    })
}

/// Сортирует список на месте.
pub fn sort_entries(entries: &mut [Entry], sort: Sort) {
    entries.sort_by(|a, b| {
        // Каталоги всегда сверху, в любом порядке сортировки: смешивать их с
        // файлами неудобно, и так не делает ни один файловый менеджер.
        let by_kind = a.is_dir().cmp(&b.is_dir()).reverse();
        if by_kind != std::cmp::Ordering::Equal {
            return by_kind;
        }

        let ordering = match sort.by {
            SortBy::Name => natural_cmp(&a.name, &b.name),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Modified => a.modified.cmp(&b.modified),
            SortBy::Kind => a
                .extension()
                .cmp(&b.extension())
                .then_with(|| natural_cmp(&a.name, &b.name)),
        };

        if sort.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

/// Сравнение имён так, как ожидает человек: `файл2` идёт перед `файл10`.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();

    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(lc), Some(rc)) => {
                if lc.is_ascii_digit() && rc.is_ascii_digit() {
                    let left_number = take_number(&mut left);
                    let right_number = take_number(&mut right);
                    match left_number.cmp(&right_number) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }

                let lk = lc.to_lowercase().next().unwrap_or(lc);
                let rk = rc.to_lowercase().next().unwrap_or(rc);
                match lk.cmp(&rk) {
                    std::cmp::Ordering::Equal => {
                        left.next();
                        right.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut value: u128 = 0;
    while let Some(c) = chars.peek().copied() {
        if !c.is_ascii_digit() {
            break;
        }
        // Абсурдно длинные цифры в имени не должны переполнять счётчик.
        value = value
            .saturating_mul(10)
            .saturating_add(c as u128 - '0' as u128);
        chars.next();
    }
    value
}

/// Размер файла человеческими словами.
///
/// Используются десятичные приставки (1 КБ = 1000 Б) — так же считают
/// производители дисков и системный монитор, и так подпись совпадает с тем,
/// что пользователь видит в других местах.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["Б", "КБ", "МБ", "ГБ", "ТБ", "ПБ", "ЭБ"];
    if bytes < 1000 {
        return format!("{bytes} Б");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }

    // Один знак после запятой до 10, дальше он не несёт смысла.
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("hype-files-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, contents).unwrap();
            path
        }

        fn dir(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lists_files_and_directories() {
        let temp = TempDir::new("list");
        temp.file("заметки.txt", "привет");
        temp.dir("картинки");

        let entries = list_dir(&temp.0, false, Sort::default()).unwrap();
        assert_eq!(entries.len(), 2);
        // Каталог всегда выше файла.
        assert_eq!(entries[0].name, "картинки");
        assert_eq!(entries[0].kind, Kind::Directory);
        assert_eq!(entries[1].name, "заметки.txt");
        assert_eq!(entries[1].size, "привет".len() as u64);
    }

    #[test]
    fn hidden_files_are_skipped_unless_asked_for() {
        let temp = TempDir::new("hidden");
        temp.file("видимый", "");
        temp.file(".скрытый", "");

        assert_eq!(list_dir(&temp.0, false, Sort::default()).unwrap().len(), 1);

        let all = list_dir(&temp.0, true, Sort::default()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|e| e.hidden));
    }

    #[test]
    fn a_missing_directory_reports_a_readable_error() {
        let err = list_dir(Path::new("/нет/такого/пути"), false, Sort::default()).unwrap_err();
        assert!(err.to_string().contains("каталог не найден"), "{err}");
    }

    #[test]
    fn a_broken_symlink_is_shown_not_hidden() {
        let temp = TempDir::new("broken");
        let target = temp.0.join("исчез");
        std::os::unix::fs::symlink(&target, temp.0.join("ссылка")).unwrap();

        let entries = list_dir(&temp.0, false, Sort::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, Kind::BrokenLink);
        assert!(entries[0].symlink);
        assert_eq!(entries[0].icon_name(), "dialog-warning");
    }

    #[test]
    fn a_symlink_to_a_directory_behaves_like_a_directory() {
        let temp = TempDir::new("dirlink");
        let target = temp.dir("настоящий");
        std::os::unix::fs::symlink(&target, temp.0.join("ярлык")).unwrap();

        let entries = list_dir(&temp.0, false, Sort::default()).unwrap();
        let link = entries.iter().find(|e| e.name == "ярлык").unwrap();
        assert!(link.is_dir());
        assert!(link.symlink);
    }

    #[test]
    fn sorting_by_name_is_natural() {
        let mut entries: Vec<Entry> = ["файл10", "файл2", "файл1"]
            .iter()
            .map(|name| Entry {
                name: (*name).into(),
                path: PathBuf::from(name),
                kind: Kind::File,
                size: 0,
                modified: None,
                hidden: false,
                symlink: false,
            })
            .collect();

        sort_entries(&mut entries, Sort::default());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["файл1", "файл2", "файл10"]);
    }

    #[test]
    fn natural_comparison_handles_the_awkward_cases() {
        use std::cmp::Ordering;
        assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("A", "a"), Ordering::Equal, "регистр не важен");
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
        // Абсурдно длинное число не должно ронять сравнение.
        let huge = "9".repeat(80);
        assert_ne!(natural_cmp(&huge, "1"), Ordering::Less);
    }

    #[test]
    fn sorting_by_size_puts_directories_first_anyway() {
        let temp = TempDir::new("size");
        temp.file("большой", &"a".repeat(1000));
        temp.file("маленький", "a");
        temp.dir("каталог");

        let entries = list_dir(&temp.0, false, Sort::by(SortBy::Size)).unwrap();
        assert_eq!(entries[0].name, "каталог");
        assert_eq!(entries[1].name, "маленький");
        assert_eq!(entries[2].name, "большой");
    }

    #[test]
    fn descending_sort_reverses_files_but_not_the_directory_rule() {
        let temp = TempDir::new("desc");
        temp.file("а", "");
        temp.file("б", "");
        temp.dir("каталог");

        let sort = Sort {
            by: SortBy::Name,
            descending: true,
        };
        let entries = list_dir(&temp.0, false, sort).unwrap();
        assert_eq!(entries[0].name, "каталог");
        assert_eq!(entries[1].name, "б");
        assert_eq!(entries[2].name, "а");
    }

    #[test]
    fn toggling_the_same_column_flips_the_direction() {
        let sort = Sort::default();
        assert!(!sort.descending);

        let flipped = sort.toggle(SortBy::Name);
        assert!(flipped.descending);

        let other = flipped.toggle(SortBy::Size);
        assert_eq!(other.by, SortBy::Size);
        assert!(!other.descending, "смена столбца начинает с возрастания");
    }

    #[test]
    fn icons_match_the_file_type() {
        let icon_for = |name: &str| {
            Entry {
                name: name.into(),
                path: PathBuf::from(name),
                kind: Kind::File,
                size: 0,
                modified: None,
                hidden: false,
                symlink: false,
            }
            .icon_name()
        };

        assert_eq!(icon_for("фото.JPG"), "image-x-generic", "регистр не важен");
        assert_eq!(icon_for("клип.mkv"), "video-x-generic");
        assert_eq!(icon_for("песня.flac"), "audio-x-generic");
        assert_eq!(icon_for("архив.tar"), "package-x-generic");
        assert_eq!(icon_for("main.rs"), "text-x-script");
        assert_eq!(icon_for("config.toml"), "text-x-generic-template");
        assert_eq!(icon_for("безымянный"), "text-x-generic");
    }

    #[test]
    fn human_sizes_read_the_way_people_expect() {
        assert_eq!(human_size(0), "0 Б");
        assert_eq!(human_size(999), "999 Б");
        assert_eq!(human_size(1000), "1.0 КБ");
        assert_eq!(human_size(1536), "1.5 КБ");
        assert_eq!(human_size(15_000), "15 КБ");
        assert_eq!(human_size(1_500_000), "1.5 МБ");
        assert_eq!(human_size(2_000_000_000), "2.0 ГБ");
        // Приставок хватает до предела u64 — иначе размер выглядел бы как
        // «18446744 ТБ».
        assert_eq!(human_size(u64::MAX), "18 ЭБ");
    }

    #[test]
    fn directories_show_a_dash_instead_of_a_size() {
        let temp = TempDir::new("dash");
        temp.dir("каталог");
        let entries = list_dir(&temp.0, false, Sort::default()).unwrap();
        assert_eq!(entries[0].size_label(), "—");
    }
}
