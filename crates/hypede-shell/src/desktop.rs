//! Чтение `.desktop`-файлов для поиска приложений.
//!
//! Формат описан в спецификации XDG; здесь разобрана та его часть, что нужна
//! лаунчеру: имя, команда, значок и признаки «не показывать».

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Приложение, найденное в системе.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopApp {
    pub name: String,
    /// Команда запуска, уже очищенная от подстановок вида `%U`.
    pub exec: String,
    pub icon: String,
    pub comment: String,
    /// Ключевые слова для поиска.
    pub keywords: Vec<String>,
    /// Требуется ли запуск в терминале.
    pub terminal: bool,
    pub path: PathBuf,
}

/// Разбирает содержимое `.desktop`-файла.
///
/// Возвращает `None`, если это не приложение, если оно помечено `NoDisplay`
/// или `Hidden`, или если у него нет ни имени, ни команды — показывать такую
/// запись в лаунчере бессмысленно.
pub fn parse_desktop_entry(text: &str, path: &Path, locale: &str) -> Option<DesktopApp> {
    let mut values: HashMap<String, String> = HashMap::new();
    let mut in_main_section = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            // Нас интересует только основная секция: у действий (Desktop
            // Action) свои имена и команды, и путать их с приложением нельзя.
            in_main_section = line == "[Desktop Entry]";
            continue;
        }
        if !in_main_section {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    if values.get("Type").map(String::as_str) != Some("Application") {
        return None;
    }
    if is_true(values.get("NoDisplay")) || is_true(values.get("Hidden")) {
        return None;
    }

    // Локализованное имя предпочитается: `Name[ru]` важнее `Name`.
    let localised = |key: &str| -> Option<String> {
        values
            .get(&format!("{key}[{locale}]"))
            .or_else(|| values.get(key))
            .cloned()
    };

    let name = localised("Name")?;
    let exec = values.get("Exec")?.clone();
    if name.is_empty() || exec.is_empty() {
        return None;
    }

    Some(DesktopApp {
        name,
        exec: clean_exec(&exec),
        icon: values
            .get("Icon")
            .cloned()
            .unwrap_or_else(|| "application-x-executable".into()),
        comment: localised("Comment").unwrap_or_default(),
        keywords: localised("Keywords")
            .unwrap_or_default()
            .split(';')
            .filter(|k| !k.is_empty())
            .map(str::to_string)
            .collect(),
        terminal: is_true(values.get("Terminal")),
        path: path.to_path_buf(),
    })
}

fn is_true(value: Option<&String>) -> bool {
    value
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Убирает из команды подстановки спецификации (`%U`, `%f`, `%i` и прочие).
///
/// Без этого в командной строке оказался бы литерал `%U`, и приложение
/// получило бы его как имя файла.
pub fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // `%%` — это экранированный знак процента.
            Some('%') => out.push('%'),
            Some(_) => {}
            None => {}
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ищет все приложения в стандартных каталогах.
///
/// При совпадении имени файла побеждает каталог с большим приоритетом:
/// пользовательский `.desktop` должен перекрывать системный.
pub fn find_applications(dirs: &[PathBuf], locale: &str) -> Vec<DesktopApp> {
    let mut seen: Vec<String> = Vec::new();
    let mut apps = Vec::new();

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().map(|e| e != "desktop").unwrap_or(true) {
                continue;
            }

            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if seen.contains(&file_name) {
                continue;
            }
            seen.push(file_name);

            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse_desktop_entry(&text, &path, locale) {
                apps.push(app);
            }
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Оценивает, насколько приложение подходит запросу.
///
/// Чем больше число, тем выше в списке. `None` означает, что не подходит.
/// Точное начало имени ценится выше вхождения в середину, а имя — выше
/// описания: так первым в списке оказывается то, что человек и набирал.
pub fn match_score(app: &DesktopApp, query: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }

    let name = app.name.to_lowercase();
    if name == query {
        return Some(1000);
    }
    if name.starts_with(&query) {
        return Some(500 - name.len() as i32);
    }
    if name.contains(&query) {
        return Some(300 - name.len() as i32);
    }
    if app
        .keywords
        .iter()
        .any(|k| k.to_lowercase().contains(&query))
    {
        return Some(200);
    }
    if app.comment.to_lowercase().contains(&query) {
        return Some(100);
    }
    // Команда запуска — последняя надежда: так находится `foot` по запросу
    // «терминал», если в файле указан только Exec.
    if app.exec.to_lowercase().contains(&query) {
        return Some(50);
    }

    None
}

/// Отбирает и упорядочивает приложения по запросу.
pub fn search<'a>(apps: &'a [DesktopApp], query: &str) -> Vec<&'a DesktopApp> {
    let mut scored: Vec<(i32, &DesktopApp)> = apps
        .iter()
        .filter_map(|app| match_score(app, query).map(|score| (score, app)))
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, app)| app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Option<DesktopApp> {
        parse_desktop_entry(text, Path::new("/test.desktop"), "ru")
    }

    #[test]
    fn parses_a_normal_application() {
        let app = entry(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Text Editor\n\
             Name[ru]=Текстовый редактор\n\
             Comment[ru]=Правка текстовых файлов\n\
             Exec=gedit %U\n\
             Icon=accessories-text-editor\n\
             Keywords=text;editor;\n",
        )
        .unwrap();

        assert_eq!(app.name, "Текстовый редактор");
        assert_eq!(app.exec, "gedit", "подстановка %U должна исчезнуть");
        assert_eq!(app.icon, "accessories-text-editor");
        assert_eq!(app.comment, "Правка текстовых файлов");
        assert_eq!(app.keywords, vec!["text", "editor"]);
        assert!(!app.terminal);
    }

    #[test]
    fn falls_back_to_the_untranslated_name() {
        let app = entry("[Desktop Entry]\nType=Application\nName=Foot\nExec=foot\n").unwrap();
        assert_eq!(app.name, "Foot");
        assert_eq!(app.icon, "application-x-executable", "значок по умолчанию");
    }

    #[test]
    fn hidden_entries_are_skipped() {
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=Скрытое\nExec=x\nNoDisplay=true\n")
                .is_none()
        );
        assert!(
            entry("[Desktop Entry]\nType=Application\nName=Скрытое\nExec=x\nHidden=TRUE\n")
                .is_none()
        );
    }

    #[test]
    fn non_applications_are_skipped() {
        assert!(entry("[Desktop Entry]\nType=Link\nName=Ссылка\nURL=http://x\n").is_none());
        assert!(entry("[Desktop Entry]\nName=Без типа\nExec=x\n").is_none());
    }

    #[test]
    fn entries_without_a_command_are_skipped() {
        assert!(entry("[Desktop Entry]\nType=Application\nName=Пусто\n").is_none());
    }

    #[test]
    fn extra_sections_do_not_leak_into_the_entry() {
        let app = entry(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Браузер\n\
             Exec=browser\n\
             \n\
             [Desktop Action new-window]\n\
             Name=Новое окно\n\
             Exec=browser --new-window\n",
        )
        .unwrap();

        assert_eq!(app.name, "Браузер");
        assert_eq!(
            app.exec, "browser",
            "команда действия не должна подменять основную"
        );
    }

    #[test]
    fn exec_placeholders_are_removed_but_percent_signs_survive() {
        assert_eq!(clean_exec("app %U %i %c"), "app");
        assert_eq!(clean_exec("app --opt %f file"), "app --opt file");
        assert_eq!(clean_exec("app 100%%"), "app 100%");
        assert_eq!(
            clean_exec("app  --много   пробелов"),
            "app --много пробелов"
        );
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let app = entry(
            "# комментарий\n\n[Desktop Entry]\n# ещё один\nType=Application\nName=X\nExec=x\n",
        );
        assert!(app.is_some());
    }

    fn app(name: &str, exec: &str, comment: &str, keywords: &[&str]) -> DesktopApp {
        DesktopApp {
            name: name.into(),
            exec: exec.into(),
            icon: String::new(),
            comment: comment.into(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            terminal: false,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn search_puts_the_obvious_answer_first() {
        let apps = vec![
            app("Настройки системы", "settings", "", &[]),
            app("Настройки", "hype-settings", "", &[]),
            app("Диспетчер файлов", "files", "Настройки папок", &[]),
        ];

        let found = search(&apps, "настройки");
        assert_eq!(
            found[0].name, "Настройки",
            "точное совпадение должно быть первым"
        );
        assert_eq!(found[1].name, "Настройки системы");
        assert_eq!(found[2].name, "Диспетчер файлов");
    }

    #[test]
    fn search_finds_by_keyword_and_by_command() {
        let apps = vec![app("Foot", "foot", "", &["terminal", "терминал"])];
        assert_eq!(search(&apps, "терминал").len(), 1);
        assert_eq!(search(&apps, "foo").len(), 1);
        assert!(search(&apps, "таблица").is_empty());
    }

    #[test]
    fn search_ignores_case_and_surrounding_spaces() {
        let apps = vec![app("Калькулятор", "calc", "", &[])];
        assert_eq!(search(&apps, "  КАЛЬКУЛЯТОР  ").len(), 1);
    }

    #[test]
    fn an_empty_query_returns_everything() {
        let apps = vec![app("А", "a", "", &[]), app("Б", "b", "", &[])];
        assert_eq!(search(&apps, "").len(), 2);
    }

    #[test]
    fn a_shorter_name_wins_when_both_start_with_the_query() {
        let apps = vec![
            app("Терминал расширенный", "x", "", &[]),
            app("Терминал", "y", "", &[]),
        ];
        assert_eq!(search(&apps, "терм")[0].name, "Терминал");
    }
}
