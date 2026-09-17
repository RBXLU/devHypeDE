//! Полка — панель среды у нижнего края экрана.
//!
//! Устройство повторяет привычный по Chrome OS порядок: круглая кнопка запуска
//! у левого края, значки приложений по центру, область состояния справа.
//! Полка всегда тёмная, потому что лежит поверх обоев, а не поверх окна.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CenterBox, Image, Label,
    Orientation, Popover, Scale,
};
use hype_config::{Action, Config};
use hype_ipc::{Client, Event, EventKind, Request, Response, WindowInfo};

use crate::desktop::{find_applications, resolve_pinned, DesktopApp};
use crate::sound::{play, Sound};
use crate::status::{read_battery, read_volume, set_volume};

/// Размер значка приложения на полке.
///
/// Вместе с круглой подложкой и точкой запуска значок должен уместиться в
/// высоту полки, иначе точка окажется за её пределами и просто не будет видна.
const ICON_SIZE: i32 = 20;
/// Размер значка в круглой кнопке панели состояния.
const GLYPH_SIZE: i32 = 16;

/// Закреплённое приложение вместе с его значком.
struct ShelfApp {
    name: String,
    icon: String,
    command: String,
    terminal: bool,
    /// Идентификатор `.desktop`-файла без расширения — он же `app_id` окна.
    desktop_id: Option<String>,
}

impl ShelfApp {
    fn from_desktop(app: &DesktopApp) -> Self {
        Self {
            name: app.name.clone(),
            icon: app.icon.clone(),
            command: app.exec.clone(),
            terminal: app.terminal,
            desktop_id: app
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned()),
        }
    }

    /// Запись, для которой не нашлось `.desktop`-файла: команда как есть.
    fn from_command(command: &str) -> Self {
        Self {
            name: command.to_string(),
            icon: "application-x-executable".into(),
            command: command.to_string(),
            terminal: false,
            desktop_id: None,
        }
    }

    /// Имя программы без аргументов — по нему запущенное окно связывается со
    /// значком на полке.
    fn program(&self) -> String {
        self.command
            .split_whitespace()
            .next()
            .map(|program| {
                Path::new(program)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| program.to_string())
            })
            .unwrap_or_default()
    }
}

/// Что полка знает о среде прямо сейчас.
struct ShelfState {
    active_workspace: u8,
    workspaces: u8,
    windows: Vec<WindowInfo>,
}

struct Widgets {
    apps: GtkBox,
    workspaces: GtkBox,
    clock: Label,
    battery: GtkBox,
}

/// Собирает и показывает полку.
pub fn build(app: &Application, config: &Config) {
    crate::theme::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Полка HypeDE")
        .default_width(1280)
        .default_height(config.panel.height as i32)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("hype-shell");
    window.add_css_class("hype-shelf");

    // --- слева: кнопка запуска ---
    let launcher = Button::new();
    launcher.set_child(Some(&icon("view-app-grid-symbolic", GLYPH_SIZE)));
    launcher.add_css_class("hype-launcher-button");
    launcher.set_valign(Align::Center);
    launcher.set_tooltip_text(Some("Приложения (Super+Space)"));
    launcher.connect_clicked(|_| open_launcher());

    let left = GtkBox::new(Orientation::Horizontal, 8);
    left.set_margin_start(8);
    left.append(&launcher);

    // --- по центру: значки приложений ---
    let apps = GtkBox::new(Orientation::Horizontal, 6);
    apps.set_halign(Align::Center);

    // --- справа: состояние ---
    let workspaces = GtkBox::new(Orientation::Horizontal, 2);
    let battery = GtkBox::new(Orientation::Horizontal, 4);
    let clock = Label::new(None);

    let tray = Button::new();
    tray.add_css_class("hype-tray");
    tray.set_valign(Align::Center);
    let tray_content = GtkBox::new(Orientation::Horizontal, 8);
    if config.panel.show_battery {
        tray_content.append(&battery);
    }
    if config.panel.show_clock {
        tray_content.append(&clock);
    }
    tray.set_child(Some(&tray_content));

    let popover = build_quick_settings(config);
    popover.set_parent(&tray);
    popover.set_position(gtk4::PositionType::Top);
    tray.connect_clicked({
        let popover = popover.clone();
        move |_| popover.popup()
    });

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.set_margin_end(8);
    if config.panel.show_workspaces {
        right.append(&workspaces);
    }
    right.append(&tray);

    // CenterBox держит значки ровно по центру экрана независимо от того,
    // насколько широки края.
    let bar = CenterBox::new();
    bar.set_start_widget(Some(&left));
    bar.set_center_widget(Some(&apps));
    bar.set_end_widget(Some(&right));
    window.set_child(Some(&bar));

    let widgets = Rc::new(Widgets {
        apps,
        workspaces,
        clock,
        battery,
    });

    // Закреплённые приложения читаются один раз: список `.desktop`-файлов
    // меняется редко, а обход каталогов заметно дороже отрисовки полки.
    let installed = find_applications(&hype_config::paths::application_dirs(), &crate::locale());
    let shelf_apps: Rc<Vec<ShelfApp>> = Rc::new(
        config
            .panel
            .pinned
            .iter()
            .map(|entry| {
                resolve_pinned(&installed, entry)
                    .map(ShelfApp::from_desktop)
                    .unwrap_or_else(|| ShelfApp::from_command(entry))
            })
            .collect(),
    );

    start_clock(&widgets);
    start_battery(&widgets);
    start_ipc(&widgets, &shelf_apps, config.layout.workspaces);

    // Звук приветствия проигрывается один раз при поднятии полки: для
    // пользователя это и есть момент входа в среду.
    play(&config.sound, Sound::Startup);

    window.present();
}

/// Панель быстрых настроек, раскрывающаяся из области состояния.
fn build_quick_settings(config: &Config) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("hype-shell");
    popover.add_css_class("hype-bubble");
    popover.set_has_arrow(false);

    let content = GtkBox::new(Orientation::Vertical, 14);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_size_request(320, -1);

    content.append(&build_account_row(config));
    content.append(&build_tiles(config));

    if let Some(volume) = read_volume() {
        content.append(&build_volume_row(config, volume.level.min(1.0)));
    }

    // Внизу — дата и заряд, как подпись ко всей панели.
    let footer = GtkBox::new(Orientation::Horizontal, 8);
    let date = Label::new(None);
    date.add_css_class("hype-shell-dim");
    if let Ok(now) = glib::DateTime::now_local() {
        if let Ok(text) = now.format("%a, %e %B") {
            date.set_text(&text);
        }
    }
    footer.append(&date);

    if let Some(battery) = read_battery(Path::new("/sys")) {
        let separator = Label::new(Some("·"));
        separator.add_css_class("hype-shell-dim");
        footer.append(&separator);

        let charge = Label::new(Some(&battery.label()));
        charge.add_css_class("hype-shell-dim");
        footer.append(&charge);
    }
    content.append(&footer);

    popover.set_child(Some(&content));
    popover
}

/// Верхняя строка: кто в системе и что с сеансом.
fn build_account_row(config: &Config) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);

    let avatar = Label::new(Some(&user_initial()));
    avatar.add_css_class("hype-round-button");
    avatar.set_width_request(36);
    avatar.set_height_request(36);
    row.append(&avatar);

    let logout = Button::with_label("Выйти");
    logout.add_css_class("hype-pill-button");
    let logout_sound = config.sound.clone();
    logout.connect_clicked(move |_| {
        play(&logout_sound, Sound::Logout);
        dispatch(Action::Quit);
    });
    row.append(&logout);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);

    let settings = round_button("preferences-system-symbolic", "Параметры");
    settings.connect_clicked(|_| {
        if let Err(err) = crate::command("hype-settings").spawn() {
            tracing::warn!("не удалось открыть параметры: {err}");
        }
    });
    row.append(&settings);

    row
}

/// Плитки быстрых действий.
fn build_tiles(config: &Config) -> GtkBox {
    let tiles = GtkBox::new(Orientation::Horizontal, 12);
    tiles.set_homogeneous(true);

    // Схема оформления.
    let dark = config.theme.variant.is_dark();
    let theme_tile = tile(
        if dark {
            "weather-clear-symbolic"
        } else {
            "weather-clear-night-symbolic"
        },
        if dark {
            "Светлая тема"
        } else {
            "Тёмная тема"
        },
    );
    theme_tile.1.connect_clicked(|_| toggle_variant());
    tiles.append(&theme_tile.0);

    // Обзор окон.
    let overview = tile("view-grid-symbolic", "Обзор окон");
    overview
        .1
        .connect_clicked(|_| dispatch(Action::ToggleOverview));
    tiles.append(&overview.0);

    // Снимок экрана.
    let shot = tile("camera-photo-symbolic", "Снимок");
    shot.1.connect_clicked(|_| {
        dispatch(Action::Screenshot {
            target: hype_config::ScreenshotTarget::Screen,
        })
    });
    tiles.append(&shot.0);

    tiles
}

/// Одна плитка: круглая кнопка и подпись под ней.
fn tile(icon_name: &str, label: &str) -> (GtkBox, Button) {
    let column = GtkBox::new(Orientation::Vertical, 6);
    column.set_halign(Align::Center);

    let button = round_button(icon_name, label);
    column.append(&button);

    let caption = Label::new(Some(label));
    caption.add_css_class("hype-shell-dim");
    caption.set_wrap(true);
    caption.set_max_width_chars(10);
    caption.set_justify(gtk4::Justification::Center);
    column.append(&caption);

    (column, button)
}

fn round_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.add_css_class("hype-round-button");
    button.set_child(Some(&icon(icon_name, GLYPH_SIZE)));
    button.set_tooltip_text(Some(tooltip));
    button.set_halign(Align::Center);
    button
}

/// Строка громкости.
fn build_volume_row(config: &Config, level: f64) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);

    let button = round_button("audio-volume-high-symbolic", "Громкость");
    row.append(&button);

    let scale = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.05);
    scale.set_value(level);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    let sound_config = config.sound.clone();
    scale.connect_value_changed(move |scale| {
        set_volume(scale.value());
        play(&sound_config, Sound::Volume);
    });
    row.append(&scale);

    row
}

/// Первая буква имени пользователя — вместо фотографии.
fn user_initial() -> String {
    std::env::var("USER")
        .ok()
        .and_then(|name| name.chars().next())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// Переключает светлую и тёмную схему окон.
fn toggle_variant() {
    let Ok(loaded) = hype_config::load() else {
        return;
    };
    let mut config = loaded.config;
    config.theme.variant = config.theme.variant.toggled();

    let _ = hype_config::save(&config);
    let _ = hype_config::export_theme_css(&config);
    if let Ok(mut client) = Client::connect_default() {
        let _ = client.request(Request::ApplyConfig {
            config: Box::new(config),
        });
    }
}

fn icon(name: &str, size: i32) -> Image {
    let image = Image::from_icon_name(name);
    image.set_pixel_size(size);
    image
}

/// Открывает поиск приложений отдельным процессом.
fn open_launcher() {
    let program = std::env::current_exe().unwrap_or_else(|_| "hype-shell".into());
    if let Err(err) = std::process::Command::new(program)
        .arg("--launcher")
        .spawn()
    {
        tracing::warn!("не удалось открыть поиск приложений: {err}");
    }
}

/// Отправляет действие композитору.
fn dispatch(action: Action) {
    match Client::connect_default() {
        Ok(mut client) => {
            let _ = client.request(Request::Dispatch { action });
        }
        Err(err) => tracing::warn!("композитор не отвечает: {err}"),
    }
}

/// Обновляет часы раз в секунду.
fn start_clock(widgets: &Rc<Widgets>) {
    let clock = widgets.clock.clone();
    let update = move || {
        // Время берётся у GLib: она знает часовой пояс и переход на летнее
        // время, чего не даёт голый SystemTime.
        if let Ok(now) = glib::DateTime::now_local() {
            if let Ok(text) = now.format("%H:%M") {
                clock.set_text(&text);
            }
            if let Ok(date) = now.format("%A, %e %B") {
                clock.set_tooltip_text(Some(&date));
            }
        }
    };
    update();
    glib::timeout_add_local(Duration::from_secs(1), move || {
        update();
        glib::ControlFlow::Continue
    });
}

/// Обновляет заряд батареи раз в полминуты.
fn start_battery(widgets: &Rc<Widgets>) {
    let container = widgets.battery.clone();
    let update = move || {
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        if let Some(battery) = read_battery(Path::new("/sys")) {
            container.append(&icon(battery.icon_name(), GLYPH_SIZE));
        }
    };
    update();
    glib::timeout_add_local(Duration::from_secs(30), move || {
        update();
        glib::ControlFlow::Continue
    });
}

/// Держит связь с композитором.
fn start_ipc(widgets: &Rc<Widgets>, apps: &Rc<Vec<ShelfApp>>, workspace_count: u8) {
    let state = Rc::new(RefCell::new(ShelfState {
        active_workspace: 1,
        workspaces: workspace_count,
        windows: Vec::new(),
    }));

    refresh_from_compositor(&state);
    render(widgets, apps, &state.borrow());

    let (sender, receiver) = async_channel::unbounded::<Event>();

    // Подписка живёт в отдельном потоке: чтение из сокета блокирующее, а
    // останавливать поток интерфейса нельзя.
    std::thread::Builder::new()
        .name("hype-shelf-ipc".into())
        .spawn(move || {
            let Ok(client) = Client::connect_default() else {
                tracing::warn!("полка не нашла композитор; события недоступны");
                return;
            };
            let events = match client.subscribe(vec![
                EventKind::Workspace,
                EventKind::Focus,
                EventKind::Window,
            ]) {
                Ok(events) => events,
                Err(err) => {
                    tracing::warn!("не удалось подписаться на события: {err}");
                    return;
                }
            };

            for event in events {
                match event {
                    Ok(event) => {
                        if sender.send_blocking(event).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("связь с композитором прервана: {err}");
                        break;
                    }
                }
            }
        })
        .ok();

    let widgets = Rc::clone(widgets);
    let apps = Rc::clone(apps);
    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            if let Event::WorkspaceChanged { index } = event {
                state.borrow_mut().active_workspace = index;
            }
            // Любое событие означает, что список окон мог измениться; проще и
            // надёжнее перечитать его целиком, чем вести свою копию.
            refresh_from_compositor(&state);
            render(&widgets, &apps, &state.borrow());
        }
    });
}

/// Перечитывает состояние среды.
fn refresh_from_compositor(state: &Rc<RefCell<ShelfState>>) {
    let Ok(mut client) = Client::connect_default() else {
        return;
    };
    let Ok(Response::State(compositor)) = client.request(Request::GetState) else {
        return;
    };

    let mut state = state.borrow_mut();
    state.workspaces = compositor.workspaces.len() as u8;
    state.active_workspace = compositor
        .workspaces
        .iter()
        .find(|workspace| workspace.active)
        .map(|workspace| workspace.index)
        .unwrap_or(1);
    state.windows = compositor.windows;
}

fn render(widgets: &Rc<Widgets>, apps: &Rc<Vec<ShelfApp>>, state: &ShelfState) {
    clear(&widgets.apps);
    for app in apps.iter() {
        widgets.apps.append(&build_app_button(app, state));
    }

    clear(&widgets.workspaces);
    for index in visible_workspaces(state) {
        let button = Button::with_label(&index.to_string());
        button.add_css_class("hype-tray");
        button.set_valign(Align::Center);
        if index == state.active_workspace {
            button.add_css_class("hype-workspace-active");
        } else {
            button.add_css_class("hype-shell-dim");
        }
        button.set_tooltip_text(Some(&format!("Рабочий стол {index}")));
        button.connect_clicked(move |_| dispatch(Action::Workspace { index }));
        widgets.workspaces.append(&button);
    }
}

/// Относится ли окно к этому значку на полке.
///
/// Приложение сообщает `app_id`, и совпасть он может по-разному: у наших
/// программ это идентификатор `.desktop`-файла (`dev.hypede.Files`), у многих
/// чужих — просто имя программы (`foot`), а иногда — имя с обратным доменом
/// (`org.gnome.Foot`). Проверяются все три случая, иначе значок не отметится
/// как запущенный и щелчок по нему откроет вторую копию вместо переключения.
fn matches_app(app_id: &str, desktop_id: Option<&str>, program: &str) -> bool {
    let app_id = app_id.trim().to_lowercase();
    if app_id.is_empty() {
        return false;
    }
    let program = program.trim().to_lowercase();

    if let Some(desktop_id) = desktop_id {
        if app_id == desktop_id.to_lowercase() {
            return true;
        }
    }

    if !program.is_empty() && app_id == program {
        return true;
    }

    // Последняя часть обратного доменного имени: org.gnome.Foot -> foot.
    let tail = app_id.rsplit('.').next().unwrap_or(&app_id);
    !program.is_empty() && tail == program
}

/// Какие рабочие столы показывать.
///
/// Пустые столы не показываются: девять одинаковых кнопок занимают полку и
/// ничего не сообщают. Видны занятые, текущий и один следующий — чтобы было
/// куда перейти.
fn visible_workspaces(state: &ShelfState) -> Vec<u8> {
    let total = state.workspaces.max(1);
    let mut occupied: Vec<u8> = (1..=total)
        .filter(|index| {
            *index == state.active_workspace
                || state
                    .windows
                    .iter()
                    .any(|window| window.workspace == *index)
        })
        .collect();

    if let Some(next) = (1..=total).find(|index| !occupied.contains(index)) {
        occupied.push(next);
    }

    occupied.sort_unstable();
    occupied
}

fn build_app_button(app: &ShelfApp, state: &ShelfState) -> GtkBox {
    // Значок и точка запуска под ним живут в одной колонке.
    let column = GtkBox::new(Orientation::Vertical, 2);
    column.set_valign(Align::Center);

    let button = Button::new();
    button.add_css_class("hype-shelf-button");

    let circle = GtkBox::new(Orientation::Horizontal, 0);
    circle.add_css_class("hype-icon-circle");
    circle.append(&icon(&app.icon, ICON_SIZE));
    button.set_child(Some(&circle));

    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.add_css_class("hype-running-dot");
    dot.set_halign(Align::Center);
    // Размер задаётся кодом, а не только стилем: у пустой коробки нет
    // содержимого, и одних min-width в CSS ей не хватает.
    dot.set_size_request(4, 4);

    let program = app.program();
    let window = state
        .windows
        .iter()
        .find(|window| matches_app(&window.app_id, app.desktop_id.as_deref(), &program));

    match window {
        Some(window) => {
            button.set_tooltip_text(Some(&format!("{} — {}", app.name, window.title)));
            let id = window.id;
            button.connect_clicked(move |_| dispatch(Action::FocusWindow { id }));
        }
        None => {
            dot.add_css_class("hype-hidden");
            button.set_tooltip_text(Some(&app.name));
            let command = if app.terminal {
                format!("foot -e {}", app.command)
            } else {
                app.command.clone()
            };
            button.connect_clicked(move |_| {
                dispatch(Action::Spawn {
                    command: command.clone(),
                })
            });
        }
    }

    column.append(&button);
    column.append(&dot);
    column
}

fn clear(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_windows_match_by_desktop_id() {
        assert!(matches_app(
            "dev.hypede.Files",
            Some("dev.hypede.Files"),
            "hype-files"
        ));
    }

    #[test]
    fn plain_programs_match_by_name() {
        assert!(matches_app("foot", Some("foot"), "foot"));
        assert!(matches_app("foot", None, "foot"));
    }

    #[test]
    fn reverse_domain_names_match_by_their_last_part() {
        assert!(matches_app("org.gnome.Foot", None, "foot"));
    }

    #[test]
    fn case_does_not_matter() {
        assert!(matches_app(
            "DEV.HypeDE.Files",
            Some("dev.hypede.files"),
            ""
        ));
    }

    #[test]
    fn unrelated_windows_do_not_match() {
        assert!(!matches_app(
            "firefox",
            Some("dev.hypede.Files"),
            "hype-files"
        ));
        assert!(!matches_app("", Some("dev.hypede.Files"), "hype-files"));
        // Пустая программа не должна совпадать со всем подряд.
        assert!(!matches_app("firefox", None, ""));
    }
}
