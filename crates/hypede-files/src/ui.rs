//! Окно файлового менеджера.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, HeaderBar, Image, Label,
    ListBox, Orientation, ScrolledWindow, SelectionMode, Separator,
};

use crate::fs::{self, Entry, Sort, SortBy};
use crate::history::{breadcrumbs, History};
use crate::places::standard_places;

/// Состояние окна.
struct AppState {
    history: History,
    sort: Sort,
    show_hidden: bool,
}

/// Виджеты, которые приходится обновлять при переходе.
struct Widgets {
    window: ApplicationWindow,
    entries: ListBox,
    crumbs: GtkBox,
    back: Button,
    forward: Button,
    up: Button,
    status: Label,
}

/// Собирает и показывает окно.
pub fn build(app: &Application) {
    load_theme_css();

    let home = home_dir();
    let state = Rc::new(RefCell::new(AppState {
        history: History::new(home.clone()),
        sort: Sort::default(),
        show_hidden: false,
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Файлы")
        .default_width(1040)
        .default_height(660)
        .build();

    let back = Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Назад"));
    let forward = Button::from_icon_name("go-next-symbolic");
    forward.set_tooltip_text(Some("Вперёд"));
    let up = Button::from_icon_name("go-up-symbolic");
    up.set_tooltip_text(Some("На уровень выше"));

    let navigation = GtkBox::new(Orientation::Horizontal, 0);
    navigation.add_css_class("linked");
    navigation.append(&back);
    navigation.append(&forward);
    navigation.append(&up);

    let crumbs = GtkBox::new(Orientation::Horizontal, 2);
    crumbs.add_css_class("hype-pill");

    let hidden_toggle = Button::from_icon_name("view-reveal-symbolic");
    hidden_toggle.set_tooltip_text(Some("Показывать скрытые файлы (Ctrl+H)"));

    let sort_button = Button::from_icon_name("view-sort-ascending-symbolic");
    sort_button.set_tooltip_text(Some("Сортировка по имени, размеру, дате"));

    let header = HeaderBar::new();
    header.pack_start(&navigation);
    header.set_title_widget(Some(&crumbs));
    header.pack_end(&sort_button);
    header.pack_end(&hidden_toggle);
    window.set_titlebar(Some(&header));

    let entries = ListBox::new();
    entries.set_selection_mode(SelectionMode::Single);
    entries.add_css_class("navigation-sidebar");

    let scroller = ScrolledWindow::builder()
        .child(&entries)
        .hexpand(true)
        .vexpand(true)
        .build();

    let status = Label::new(None);
    status.add_css_class("hype-dim");
    status.set_halign(gtk4::Align::Start);
    status.set_margin_start(12);
    status.set_margin_top(4);
    status.set_margin_bottom(4);

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.append(&scroller);
    content.append(&Separator::new(Orientation::Horizontal));
    content.append(&status);

    let sidebar = build_sidebar(&home);
    let layout = GtkBox::new(Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&Separator::new(Orientation::Vertical));
    layout.append(&content);
    window.set_child(Some(&layout));

    let widgets = Rc::new(Widgets {
        window: window.clone(),
        entries: entries.clone(),
        crumbs: crumbs.clone(),
        back: back.clone(),
        forward: forward.clone(),
        up: up.clone(),
        status: status.clone(),
    });

    // Боковая панель: переход по щелчку.
    sidebar.connect_row_activated({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_, row| {
            if let Some(path) = unsafe { row.data::<PathBuf>("hype-path") } {
                let path = unsafe { path.as_ref() }.clone();
                navigate_to(&state, &widgets, &path);
            }
        }
    });

    entries.connect_row_activated({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_, row| {
            let Some(path) = (unsafe { row.data::<PathBuf>("hype-path") }) else {
                return;
            };
            let path = unsafe { path.as_ref() }.clone();
            if path.is_dir() {
                navigate_to(&state, &widgets, &path);
            } else {
                open_externally(&path);
            }
        }
    });

    back.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_| {
            let path = state.borrow_mut().history.back().map(Path::to_path_buf);
            if let Some(path) = path {
                refresh(&state, &widgets, &path);
            }
        }
    });

    forward.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_| {
            let path = state.borrow_mut().history.forward().map(Path::to_path_buf);
            if let Some(path) = path {
                refresh(&state, &widgets, &path);
            }
        }
    });

    up.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_| {
            let parent = state
                .borrow()
                .history
                .current()
                .parent()
                .map(Path::to_path_buf);
            if let Some(parent) = parent {
                navigate_to(&state, &widgets, &parent);
            }
        }
    });

    hidden_toggle.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_| {
            let path = {
                let mut state = state.borrow_mut();
                state.show_hidden = !state.show_hidden;
                state.history.current().to_path_buf()
            };
            refresh(&state, &widgets, &path);
        }
    });

    sort_button.connect_clicked({
        let state = Rc::clone(&state);
        let widgets = Rc::clone(&widgets);
        move |_| {
            // Кнопка перебирает способы сортировки по кругу: имя → размер →
            // дата. Полноценное меню появится вместе с режимом сетки.
            let path = {
                let mut state = state.borrow_mut();
                let next = match state.sort.by {
                    SortBy::Name => SortBy::Size,
                    SortBy::Size => SortBy::Modified,
                    _ => SortBy::Name,
                };
                state.sort = state.sort.toggle(next);
                state.history.current().to_path_buf()
            };
            refresh(&state, &widgets, &path);
        }
    });

    install_shortcuts(&window, &state, &widgets);

    let start = home.clone();
    refresh(&state, &widgets, &start);
    window.present();
}

fn build_sidebar(home: &Path) -> ListBox {
    let sidebar = ListBox::new();
    sidebar.set_selection_mode(SelectionMode::Single);
    sidebar.add_css_class("navigation-sidebar");
    sidebar.set_width_request(200);

    for place in standard_places(home) {
        let row = gtk4::ListBoxRow::new();
        let line = GtkBox::new(Orientation::Horizontal, 10);
        line.set_margin_start(10);
        line.set_margin_end(10);
        line.set_margin_top(7);
        line.set_margin_bottom(7);
        line.append(&Image::from_icon_name(place.icon));
        line.append(&Label::new(Some(&place.label)));
        row.set_child(Some(&line));

        unsafe { row.set_data("hype-path", place.path.clone()) };
        sidebar.append(&row);
    }

    sidebar
}

/// Переходит в каталог, запоминая шаг в истории.
fn navigate_to(state: &Rc<RefCell<AppState>>, widgets: &Rc<Widgets>, path: &Path) {
    state.borrow_mut().history.push(path.to_path_buf());
    refresh(state, widgets, path);
}

/// Перечитывает каталог и перерисовывает список.
fn refresh(state: &Rc<RefCell<AppState>>, widgets: &Rc<Widgets>, path: &Path) {
    let (sort, show_hidden) = {
        let state = state.borrow();
        (state.sort, state.show_hidden)
    };

    while let Some(child) = widgets.entries.first_child() {
        widgets.entries.remove(&child);
    }

    match fs::list_dir(path, show_hidden, sort) {
        Ok(entries) => {
            for entry in &entries {
                widgets.entries.append(&build_row(entry));
            }
            widgets.status.set_text(&summary(&entries));
        }
        Err(err) => {
            // Ошибку показываем прямо в списке: модальное окно на каждый
            // недоступный каталог утомляет сильнее, чем строка на его месте.
            let label = Label::new(Some(&err.to_string()));
            label.add_css_class("hype-dim");
            label.set_margin_top(24);
            let row = gtk4::ListBoxRow::new();
            row.set_activatable(false);
            row.set_child(Some(&label));
            widgets.entries.append(&row);
            widgets.status.set_text("Каталог не прочитан");
        }
    }

    update_crumbs(state, widgets, path);

    let state = state.borrow();
    widgets.back.set_sensitive(state.history.can_go_back());
    widgets.forward.set_sensitive(state.history.can_go_forward());
    widgets.up.set_sensitive(path.parent().is_some());
    widgets.window.set_title(Some(&window_title(path)));
}

fn build_row(entry: &Entry) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    let line = GtkBox::new(Orientation::Horizontal, 12);
    line.set_margin_start(12);
    line.set_margin_end(12);
    line.set_margin_top(6);
    line.set_margin_bottom(6);

    line.append(&Image::from_icon_name(entry.icon_name()));

    let name = Label::new(Some(&entry.name));
    name.set_halign(gtk4::Align::Start);
    name.set_hexpand(true);
    name.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    if entry.hidden {
        name.add_css_class("hype-dim");
    }
    line.append(&name);

    if entry.symlink {
        let link = Image::from_icon_name("emblem-symbolic-link");
        link.set_tooltip_text(Some("Символическая ссылка"));
        line.append(&link);
    }

    let size = Label::new(Some(&entry.size_label()));
    size.add_css_class("hype-dim");
    size.set_width_chars(10);
    size.set_halign(gtk4::Align::End);
    line.append(&size);

    row.set_child(Some(&line));
    unsafe { row.set_data("hype-path", entry.path.clone()) };
    row
}

fn update_crumbs(state: &Rc<RefCell<AppState>>, widgets: &Rc<Widgets>, path: &Path) {
    while let Some(child) = widgets.crumbs.first_child() {
        widgets.crumbs.remove(&child);
    }

    let home = home_dir();
    for (index, (label, target)) in breadcrumbs(path, Some(&home)).into_iter().enumerate() {
        if index > 0 {
            let separator = Label::new(Some("›"));
            separator.add_css_class("hype-dim");
            widgets.crumbs.append(&separator);
        }

        let button = Button::with_label(&label);
        button.add_css_class("flat");
        button.connect_clicked({
            let state = Rc::clone(state);
            let widgets = Rc::clone(widgets);
            let target = target.clone();
            move |_| navigate_to(&state, &widgets, &target)
        });
        widgets.crumbs.append(&button);
    }
}

fn install_shortcuts(
    window: &ApplicationWindow,
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<Widgets>,
) {
    let controller = gtk4::EventControllerKey::new();
    let state = Rc::clone(state);
    let widgets = Rc::clone(widgets);

    controller.connect_key_pressed(move |_, key, _, modifiers| {
        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

        match key {
            gdk::Key::h if ctrl => {
                let path = {
                    let mut state = state.borrow_mut();
                    state.show_hidden = !state.show_hidden;
                    state.history.current().to_path_buf()
                };
                refresh(&state, &widgets, &path);
                glib::Propagation::Stop
            }
            gdk::Key::Left if alt => {
                let path = state.borrow_mut().history.back().map(Path::to_path_buf);
                if let Some(path) = path {
                    refresh(&state, &widgets, &path);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Right if alt => {
                let path = state.borrow_mut().history.forward().map(Path::to_path_buf);
                if let Some(path) = path {
                    refresh(&state, &widgets, &path);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Up if alt => {
                let parent = state
                    .borrow()
                    .history
                    .current()
                    .parent()
                    .map(Path::to_path_buf);
                if let Some(parent) = parent {
                    navigate_to(&state, &widgets, &parent);
                }
                glib::Propagation::Stop
            }
            gdk::Key::F5 => {
                let path = state.borrow().history.current().to_path_buf();
                refresh(&state, &widgets, &path);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });

    window.add_controller(controller);
}

/// Подпись внизу окна: сколько там всего.
fn summary(entries: &[Entry]) -> String {
    let dirs = entries.iter().filter(|e| e.is_dir()).count();
    let files = entries.len() - dirs;
    let bytes: u64 = entries.iter().filter(|e| !e.is_dir()).map(|e| e.size).sum();

    if entries.is_empty() {
        return "Пусто".to_string();
    }

    format!(
        "{} {}, {} {} · {}",
        dirs,
        plural(dirs, "папка", "папки", "папок"),
        files,
        plural(files, "файл", "файла", "файлов"),
        fs::human_size(bytes)
    )
}

/// Русское склонение после числительного.
pub fn plural<'a>(count: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let tens = count % 100;
    if (11..=14).contains(&tens) {
        return many;
    }
    match count % 10 {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

/// Заголовок окна: имя текущего каталога.
pub fn window_title(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

fn open_externally(path: &Path) {
    let uri = format!("file://{}", path.display());
    if let Err(err) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
        tracing::warn!("не удалось открыть {}: {err}", path.display());
    }
}

/// Подключает CSS темы HypeDE, если он выгружен композитором.
fn load_theme_css() {
    let Some(path) = hype_config::paths::generated_css_file() else {
        return;
    };
    if !path.exists() {
        // Файла нет — значит, приложение запущено вне сеанса HypeDE. Это
        // штатная ситуация: окно просто возьмёт системную тему.
        return;
    }

    let provider = CssProvider::new();
    provider.load_from_file(&gio::File::for_path(&path));

    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Kind;

    fn entry(name: &str, kind: Kind, size: u64) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            kind,
            size,
            modified: None,
            hidden: false,
            symlink: false,
        }
    }

    #[test]
    fn russian_plurals_follow_the_rules() {
        let word = |n| plural(n, "файл", "файла", "файлов");
        assert_eq!(word(1), "файл");
        assert_eq!(word(2), "файла");
        assert_eq!(word(5), "файлов");
        assert_eq!(word(11), "файлов", "одиннадцать — исключение");
        assert_eq!(word(21), "файл");
        assert_eq!(word(112), "файлов");
        assert_eq!(word(0), "файлов");
    }

    #[test]
    fn the_summary_counts_both_kinds_and_the_total_size() {
        let entries = vec![
            entry("папка", Kind::Directory, 0),
            entry("а.txt", Kind::File, 1000),
            entry("б.txt", Kind::File, 500),
        ];
        assert_eq!(summary(&entries), "1 папка, 2 файла · 1.5 КБ");
    }

    #[test]
    fn an_empty_directory_says_so() {
        assert_eq!(summary(&[]), "Пусто");
    }

    #[test]
    fn the_window_title_is_the_directory_name() {
        assert_eq!(window_title(Path::new("/home/user/Загрузки")), "Загрузки");
        assert_eq!(window_title(Path::new("/")), "/");
    }
}
