//! Поиск и запуск приложений.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Image, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, SearchEntry, SelectionMode,
};

use crate::desktop::{find_applications, search, DesktopApp};

/// Сколько совпадений показывать. Длинный список бесполезен: нужное либо в
/// первых строках, либо запрос стоит уточнить.
const MAX_RESULTS: usize = 12;

/// Собирает и показывает окно поиска.
pub fn build(app: &Application) {
    crate::theme::load();

    let apps = Rc::new(find_applications(
        &hype_config::paths::application_dirs(),
        &locale(),
    ));
    tracing::info!("найдено приложений: {}", apps.len());

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Запуск приложения")
        .default_width(620)
        .default_height(480)
        .resizable(false)
        .build();

    let entry = SearchEntry::new();
    entry.set_placeholder_text(Some("Название приложения"));
    entry.set_margin_start(12);
    entry.set_margin_end(12);
    entry.set_margin_top(12);

    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("navigation-sidebar");

    let scroller = ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(8)
        .build();

    let hint = Label::new(Some("Enter — запустить · Esc — закрыть"));
    hint.add_css_class("hype-dim");
    hint.set_halign(Align::Center);
    hint.set_margin_bottom(8);

    let layout = GtkBox::new(Orientation::Vertical, 8);
    layout.append(&entry);
    layout.append(&scroller);
    layout.append(&hint);
    window.set_child(Some(&layout));

    // Список видимых сейчас приложений — по нему запускается выбранное.
    let visible: Rc<RefCell<Vec<DesktopApp>>> = Rc::new(RefCell::new(Vec::new()));

    let refill = {
        let apps = Rc::clone(&apps);
        let visible = Rc::clone(&visible);
        let list = list.clone();
        move |query: &str| {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            let found: Vec<DesktopApp> = search(&apps, query)
                .into_iter()
                .take(MAX_RESULTS)
                .cloned()
                .collect();

            for app in &found {
                list.append(&build_row(app));
            }
            *visible.borrow_mut() = found;

            // Первая строка выделена сразу: Enter должен работать без единого
            // нажатия стрелки.
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        }
    };
    refill("");

    entry.connect_search_changed({
        let refill = refill.clone();
        move |entry| refill(&entry.text())
    });

    let launch = {
        let visible = Rc::clone(&visible);
        let window = window.clone();
        move |index: Option<usize>| {
            let visible = visible.borrow();
            let Some(app) = index.and_then(|index| visible.get(index)) else {
                return;
            };
            launch_app(app);
            window.close();
        }
    };

    list.connect_row_activated({
        let launch = launch.clone();
        move |_, row| launch(Some(row.index() as usize))
    });

    entry.connect_activate({
        let launch = launch.clone();
        let list = list.clone();
        move |_| launch(list.selected_row().map(|row| row.index() as usize))
    });

    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed({
        let window = window.clone();
        let list = list.clone();
        let launch = launch.clone();
        move |_, key, _, _| match key {
            gdk::Key::Escape => {
                window.close();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                launch(list.selected_row().map(|row| row.index() as usize));
                glib::Propagation::Stop
            }
            // Стрелки отдаём списку, чтобы выбор ходил по нему, пока курсор
            // остаётся в поле поиска.
            gdk::Key::Down => {
                move_selection(&list, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                move_selection(&list, -1);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);

    window.present();
    entry.grab_focus();
}

fn move_selection(list: &ListBox, delta: i32) {
    let current = list.selected_row().map(|row| row.index()).unwrap_or(0);
    let next = (current + delta).max(0);
    if let Some(row) = list.row_at_index(next) {
        list.select_row(Some(&row));
    }
}

fn build_row(app: &DesktopApp) -> ListBoxRow {
    let row = ListBoxRow::new();
    let line = GtkBox::new(Orientation::Horizontal, 12);
    line.set_margin_start(12);
    line.set_margin_end(12);
    line.set_margin_top(8);
    line.set_margin_bottom(8);

    let icon = Image::from_icon_name(&app.icon);
    icon.set_pixel_size(28);
    line.append(&icon);

    let text = GtkBox::new(Orientation::Vertical, 2);
    let name = Label::new(Some(&app.name));
    name.set_halign(Align::Start);
    text.append(&name);

    if !app.comment.is_empty() {
        let comment = Label::new(Some(&app.comment));
        comment.set_halign(Align::Start);
        comment.add_css_class("hype-dim");
        comment.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        text.append(&comment);
    }

    line.append(&text);
    row.set_child(Some(&line));
    row
}

/// Запускает приложение.
fn launch_app(app: &DesktopApp) {
    let parts: Vec<&str> = app.exec.split_whitespace().collect();
    let Some((program, args)) = parts.split_first() else {
        return;
    };

    let mut command = if app.terminal {
        // Приложению для терминала нужен терминал — иначе оно запустится в
        // никуда и тут же закроется.
        let mut command = std::process::Command::new("foot");
        command.arg("-e").arg(program).args(args);
        command
    } else {
        let mut command = std::process::Command::new(program);
        command.args(args);
        command
    };

    match command.spawn() {
        Ok(child) => tracing::info!("{} запущено (pid {})", app.name, child.id()),
        Err(err) => tracing::warn!("не удалось запустить {}: {err}", app.name),
    }
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
