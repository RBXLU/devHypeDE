//! История навигации — кнопки «назад» и «вперёд».

use std::path::{Path, PathBuf};

/// Журнал посещённых каталогов.
///
/// Ведёт себя как история браузера: переход из середины журнала отсекает всё,
/// что было «вперёд».
#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<PathBuf>,
    position: usize,
}

/// Сколько шагов хранится. Дальше история обрезается с начала: миллион
/// переходов за сеанс не должен превращаться в миллион путей в памяти.
const LIMIT: usize = 256;

impl History {
    pub fn new(start: impl Into<PathBuf>) -> Self {
        Self {
            entries: vec![start.into()],
            position: 0,
        }
    }

    pub fn current(&self) -> &Path {
        &self.entries[self.position]
    }

    pub fn can_go_back(&self) -> bool {
        self.position > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.position + 1 < self.entries.len()
    }

    /// Запоминает переход в новый каталог.
    ///
    /// Повторный переход в тот же каталог не засоряет историю: иначе кнопка
    /// «назад» перестала бы работать после обновления вида.
    pub fn push(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        if self.current() == path {
            return;
        }

        self.entries.truncate(self.position + 1);
        self.entries.push(path);

        if self.entries.len() > LIMIT {
            let excess = self.entries.len() - LIMIT;
            self.entries.drain(0..excess);
        }

        self.position = self.entries.len() - 1;
    }

    /// Шаг назад. Возвращает новый текущий каталог.
    pub fn back(&mut self) -> Option<&Path> {
        if !self.can_go_back() {
            return None;
        }
        self.position -= 1;
        Some(self.current())
    }

    /// Шаг вперёд.
    pub fn forward(&mut self) -> Option<&Path> {
        if !self.can_go_forward() {
            return None;
        }
        self.position += 1;
        Some(self.current())
    }
}

/// Разбивает путь на «хлебные крошки»: подпись и путь, куда ведёт.
///
/// Домашний каталог показывается одним элементом `~`, а не цепочкой
/// `/ home имя`: пользователю важно начало своего дерева, а не устройство
/// файловой системы.
pub fn breadcrumbs(path: &Path, home: Option<&Path>) -> Vec<(String, PathBuf)> {
    if let Some(home) = home {
        if let Ok(relative) = path.strip_prefix(home) {
            let mut crumbs = vec![("~".to_string(), home.to_path_buf())];
            let mut current = home.to_path_buf();
            for part in relative.components() {
                current = current.join(part);
                crumbs.push((
                    part.as_os_str().to_string_lossy().into_owned(),
                    current.clone(),
                ));
            }
            return crumbs;
        }
    }

    let mut crumbs = vec![("/".to_string(), PathBuf::from("/"))];
    let mut current = PathBuf::from("/");
    for part in path.components().skip(1) {
        current = current.join(part);
        crumbs.push((
            part.as_os_str().to_string_lossy().into_owned(),
            current.clone(),
        ));
    }
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_history_cannot_move_anywhere() {
        let history = History::new("/home/user");
        assert_eq!(history.current(), Path::new("/home/user"));
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn walking_back_and_forth_returns_the_right_directories() {
        let mut history = History::new("/a");
        history.push("/a/b");
        history.push("/a/b/c");

        assert_eq!(history.back().unwrap(), Path::new("/a/b"));
        assert_eq!(history.back().unwrap(), Path::new("/a"));
        assert!(history.back().is_none());

        assert_eq!(history.forward().unwrap(), Path::new("/a/b"));
        assert_eq!(history.forward().unwrap(), Path::new("/a/b/c"));
        assert!(history.forward().is_none());
    }

    #[test]
    fn a_new_direction_discards_the_forward_branch() {
        let mut history = History::new("/a");
        history.push("/a/b");
        history.back();
        history.push("/a/другой");

        assert_eq!(history.current(), Path::new("/a/другой"));
        assert!(
            !history.can_go_forward(),
            "старая ветка должна была отпасть"
        );
        assert_eq!(history.back().unwrap(), Path::new("/a"));
    }

    #[test]
    fn revisiting_the_same_directory_does_not_pile_up() {
        let mut history = History::new("/a");
        history.push("/a");
        history.push("/a");
        assert!(!history.can_go_back());
    }

    #[test]
    fn the_history_stops_growing_forever() {
        let mut history = History::new("/0");
        for i in 1..(LIMIT * 2) {
            history.push(format!("/{i}"));
        }
        assert!(history.entries.len() <= LIMIT);
        // Текущий каталог сохраняется при обрезке.
        assert_eq!(history.current(), Path::new(&format!("/{}", LIMIT * 2 - 1)));
    }

    #[test]
    fn breadcrumbs_shorten_the_home_directory() {
        let home = Path::new("/home/максим");
        let crumbs = breadcrumbs(Path::new("/home/максим/Загрузки/архив"), Some(home));

        let labels: Vec<&str> = crumbs.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["~", "Загрузки", "архив"]);
        assert_eq!(crumbs[0].1, home);
        assert_eq!(crumbs[2].1, Path::new("/home/максим/Загрузки/архив"));
    }

    #[test]
    fn breadcrumbs_outside_home_start_from_the_root() {
        let crumbs = breadcrumbs(Path::new("/usr/share"), Some(Path::new("/home/user")));
        let labels: Vec<&str> = crumbs.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["/", "usr", "share"]);
        assert_eq!(crumbs[1].1, Path::new("/usr"));
    }

    #[test]
    fn the_root_itself_is_a_single_crumb() {
        let crumbs = breadcrumbs(Path::new("/"), None);
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].0, "/");
    }

    #[test]
    fn the_home_directory_itself_is_a_single_crumb() {
        let home = Path::new("/home/user");
        let crumbs = breadcrumbs(home, Some(home));
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].0, "~");
    }
}
