//! Поиск и запуск приложений.
//!
//! Окно устроено как сетка: строка поиска сверху, крупные значки под ней.
//! Такой вид читается быстрее списка — глаз находит знакомую иконку раньше,
//! чем успевает прочитать название.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label,
    Orientation, ScrolledWindow, SearchEntry, SelectionMode,
};

use crate::desktop::{find_applications, search, DesktopApp};

/// Сколько значков помещается в строку.
const COLUMNS: u32 = 5;
/// Сколько совпадений показывать: нужное либо в первых рядах, либо запрос
/// стоит уточнить.
const MAX_RESULTS: usize = 30;
/// Размер значка приложения.
const ICON_SIZE: i32 = 40;

/// Собирает и показывает окно поиска отдельным приложением.
///
/// Используется запасным путём, когда полка не запущена.
pub fn build(app: &Application) {
    open(app);
}

/// Создаёт и показывает окно поиска.
///
/// Полка держит это окно у себя и переключает его: так повторное нажатие
/// закрывает поиск, а не открывает второй поверх первого.
pub fn open(app: &Application) -> ApplicationWindow {
    crate::theme::load();

    let apps = Rc::new(find_applications(
        &hype_config::paths::application_dirs(),
        &crate::locale(),
    ));
    tracing::info!("найдено приложений: {}", apps.len());

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Приложения")
        .default_width(560)
        .default_height(620)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("hype-shell");
    window.add_css_class("hype-launcher");

    let entry = SearchEntry::new();
    entry.add_css_class("hype-search");
    entry.set_placeholder_text(Some("Поиск приложений"));
    entry.set_margin_start(16);
    entry.set_margin_end(16);
    entry.set_margin_top(16);

    let grid = FlowBox::new();
    grid.set_selection_mode(SelectionMode::Single);
    grid.set_max_children_per_line(COLUMNS);
    grid.set_min_children_per_line(COLUMNS);
    grid.set_homogeneous(true);
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    grid.set_margin_start(16);
    grid.set_margin_end(16);
    grid.set_valign(Align::Start);

    let scroller = ScrolledWindow::builder()
        .child(&grid)
        .vexpand(true)
        .margin_top(16)
        .margin_bottom(8)
        .build();

    let empty = Label::new(Some("Ничего не найдено"));
    empty.add_css_class("hype-dim");
    empty.set_visible(false);

    let hint = Label::new(Some("Enter — запустить · Esc — закрыть"));
    hint.add_css_class("hype-dim");
    hint.set_halign(Align::Center);
    hint.set_margin_bottom(16);

    let layout = GtkBox::new(Orientation::Vertical, 8);
    layout.append(&entry);
    layout.append(&scroller);
    layout.append(&empty);
    layout.append(&hint);
    window.set_child(Some(&layout));

    // Список видимых сейчас приложений — по нему запускается выбранное.
    let visible: Rc<RefCell<Vec<DesktopApp>>> = Rc::new(RefCell::new(Vec::new()));

    let refill = {
        let apps = Rc::clone(&apps);
        let visible = Rc::clone(&visible);
        let grid = grid.clone();
        let empty = empty.clone();
        move |query: &str| {
            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }

            let found: Vec<DesktopApp> = search(&apps, query)
                .into_iter()
                .take(MAX_RESULTS)
                .cloned()
                .collect();

            for app in &found {
                grid.insert(&build_tile(app), -1);
            }
            empty.set_visible(found.is_empty());
            *visible.borrow_mut() = found;

            // Первый значок выделен сразу: Enter должен работать без единого
            // нажатия стрелки.
            if let Some(first) = grid.child_at_index(0) {
                grid.select_child(&first);
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
            let apps = visible.borrow();
            let Some(app) = index.and_then(|index| apps.get(index)) else {
                return;
            };
            launch_app(app);
            window.close();
        }
    };

    grid.connect_child_activated({
        let launch = launch.clone();
        move |_, child| launch(Some(child.index() as usize))
    });

    let selected = {
        let grid = grid.clone();
        move || {
            grid.selected_children()
                .first()
                .map(|child| child.index() as usize)
        }
    };

    entry.connect_activate({
        let launch = launch.clone();
        let selected = selected.clone();
        move |_| launch(selected())
    });

    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed({
        let window = window.clone();
        let grid = grid.clone();
        let launch = launch.clone();
        let selected = selected.clone();
        move |_, key, _, _| match key {
            gdk::Key::Escape => {
                window.close();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                launch(selected());
                glib::Propagation::Stop
            }
            // Стрелки двигают выбор по сетке, пока курсор остаётся в поиске.
            gdk::Key::Right => move_selection(&grid, 1),
            gdk::Key::Left => move_selection(&grid, -1),
            gdk::Key::Down => move_selection(&grid, COLUMNS as i32),
            gdk::Key::Up => move_selection(&grid, -(COLUMNS as i32)),
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);

    // Окно закрывается, когда теряет фокус, — как и любое всплывающее меню.
    // Флаг нужен, чтобы оно не закрылось до того, как фокус вообще получен.
    let was_active = std::cell::Cell::new(false);
    window.connect_is_active_notify(move |window| {
        if window.is_active() {
            was_active.set(true);
        } else if was_active.get() {
            window.close();
        }
    });

    // Момент закрытия запоминается для полки: щелчок по её кнопке сначала
    // отнимает фокус и закрывает окно, и без этой отметки тот же щелчок
    // немедленно открыл бы его заново.
    window.connect_close_request(|_| {
        crate::note_launcher_closed();
        glib::Propagation::Proceed
    });

    // GTK берёт идентификатор окна из имени программы, а полка и поиск
    // приложений живут в одном процессе. На время показа окна имя подменяется,
    // иначе композитор примет поиск за полку и положит его в её полосу.
    let previous = glib::prgname();
    glib::set_prgname(Some(crate::LAUNCHER_APP_ID));
    window.present();
    glib::set_prgname(previous.as_deref());

    entry.grab_focus();
    window
}

fn move_selection(grid: &FlowBox, delta: i32) -> glib::Propagation {
    let current = grid
        .selected_children()
        .first()
        .map(|child| child.index())
        .unwrap_or(0);

    let next = (current + delta).max(0);
    if let Some(child) = grid.child_at_index(next) {
        grid.select_child(&child);
    }
    glib::Propagation::Stop
}

fn build_tile(app: &DesktopApp) -> FlowBoxChild {
    let tile = GtkBox::new(Orientation::Vertical, 8);
    tile.set_margin_top(12);
    tile.set_margin_bottom(12);

    // Круглая подложка под значком: именно она делает сетку узнаваемой.
    let circle = GtkBox::new(Orientation::Horizontal, 0);
    circle.add_css_class("hype-icon-circle");
    circle.set_halign(Align::Center);
    let icon = Image::from_icon_name(&app.icon);
    icon.set_pixel_size(ICON_SIZE);
    circle.append(&icon);
    tile.append(&circle);

    let name = Label::new(Some(&app.name));
    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name.set_max_width_chars(12);
    name.set_justify(gtk4::Justification::Center);
    tile.append(&name);

    let child = FlowBoxChild::new();
    child.add_css_class("hype-app-tile");
    child.set_child(Some(&tile));
    if !app.comment.is_empty() {
        child.set_tooltip_text(Some(&app.comment));
    }
    child
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
