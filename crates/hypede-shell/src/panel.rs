//! Панель: рабочие столы, заголовок окна, часы, индикаторы.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Image, Label, Orientation,
};
use hype_config::Config;
use hype_ipc::{Client, Event, EventKind, Request, Response};

use crate::status::{read_battery, shorten_title};

/// Сколько символов заголовка помещается на панели.
const TITLE_CHARS: usize = 48;

struct PanelWidgets {
    workspaces: GtkBox,
    title: Label,
    clock: Label,
    battery: GtkBox,
}

/// Собирает и показывает панель.
pub fn build(app: &Application, config: &Config) {
    crate::theme::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Панель HypeDE")
        .default_width(1280)
        .default_height(config.panel.height as i32)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("hype-panel");

    let logo = Button::new();
    logo.set_child(Some(&Image::from_icon_name("start-here-symbolic")));
    logo.add_css_class("flat");
    logo.set_tooltip_text(Some("Поиск приложений (Super+Space)"));
    logo.connect_clicked(|_| {
        // Лаунчер — отдельный процесс: так его падение не уносит панель.
        if let Err(err) = std::process::Command::new(std::env::current_exe().unwrap_or_default())
            .arg("--launcher")
            .spawn()
        {
            tracing::warn!("не удалось открыть поиск приложений: {err}");
        }
    });

    let workspaces = GtkBox::new(Orientation::Horizontal, 4);
    let title = Label::new(None);
    title.add_css_class("hype-dim");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    let left = GtkBox::new(Orientation::Horizontal, 8);
    left.append(&logo);
    left.append(&workspaces);
    left.append(&title);

    let clock = Label::new(None);
    clock.set_halign(Align::Center);
    clock.set_hexpand(true);

    let battery = GtkBox::new(Orientation::Horizontal, 6);

    let power = Button::new();
    power.set_child(Some(&Image::from_icon_name("system-shutdown-symbolic")));
    power.add_css_class("flat");
    power.set_tooltip_text(Some("Завершить сеанс"));
    power.connect_clicked(|_| {
        if let Ok(mut client) = Client::connect_default() {
            let _ = client.request(Request::Dispatch {
                action: hype_config::Action::Quit,
            });
        }
    });

    let right = GtkBox::new(Orientation::Horizontal, 10);
    right.set_halign(Align::End);
    right.append(&battery);
    right.append(&power);

    let bar = GtkBox::new(Orientation::Horizontal, 12);
    bar.set_margin_start(8);
    bar.set_margin_end(8);
    bar.append(&left);
    bar.append(&clock);
    bar.append(&right);
    window.set_child(Some(&bar));

    let widgets = Rc::new(PanelWidgets {
        workspaces,
        title,
        clock,
        battery,
    });

    start_clock(&widgets);
    start_battery(&widgets);
    start_ipc(&widgets, config.layout.workspaces);

    window.present();
}

/// Обновляет часы раз в секунду.
fn start_clock(widgets: &Rc<PanelWidgets>) {
    let clock = widgets.clock.clone();
    let update = move || {
        // Время берётся у GLib: она знает часовой пояс системы и переход на
        // летнее время, чего не даёт голый SystemTime.
        if let Some(now) = glib::DateTime::now_local().ok() {
            let text = now
                .format("%a, %e %B · %H:%M")
                .map(|s| s.to_string())
                .unwrap_or_default();
            clock.set_text(&text);
        }
    };
    update();
    glib::timeout_add_local(Duration::from_secs(1), move || {
        update();
        glib::ControlFlow::Continue
    });
}

/// Обновляет индикатор батареи раз в полминуты.
fn start_battery(widgets: &Rc<PanelWidgets>) {
    let container = widgets.battery.clone();
    let update = move || {
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        if let Some(battery) = read_battery(Path::new("/sys")) {
            container.append(&Image::from_icon_name(battery.icon_name()));
            let label = Label::new(Some(&battery.label()));
            label.add_css_class("hype-dim");
            container.append(&label);
        }
    };
    update();
    glib::timeout_add_local(Duration::from_secs(30), move || {
        update();
        glib::ControlFlow::Continue
    });
}

/// Держит связь с композитором: сначала полное состояние, дальше события.
fn start_ipc(widgets: &Rc<PanelWidgets>, workspace_count: u8) {
    let state = Rc::new(RefCell::new(PanelState {
        active: 1,
        count: workspace_count,
        title: String::new(),
    }));

    // Первое заполнение — по запросу, чтобы панель не была пустой до первого
    // события.
    if let Ok(mut client) = Client::connect_default() {
        if let Ok(Response::State(compositor)) = client.request(Request::GetState) {
            let mut state = state.borrow_mut();
            state.count = compositor.workspaces.len() as u8;
            state.active = compositor
                .workspaces
                .iter()
                .find(|w| w.active)
                .map(|w| w.index)
                .unwrap_or(1);
            state.title = compositor
                .focused_window
                .and_then(|id| compositor.windows.iter().find(|w| w.id == id))
                .map(|w| w.title.clone())
                .unwrap_or_default();
        }
    }
    render(widgets, &state.borrow());

    let (sender, receiver) = async_channel::unbounded::<Event>();

    // Подписка живёт в отдельном потоке: чтение из сокета блокирующее, а
    // блокировать поток интерфейса нельзя.
    std::thread::Builder::new()
        .name("hype-panel-ipc".into())
        .spawn(move || {
            let Ok(client) = Client::connect_default() else {
                tracing::warn!("панель не нашла композитор; события недоступны");
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
                            // Панель закрылась — поток больше не нужен.
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
    glib::spawn_future_local(async move {
        while let Ok(event) = receiver.recv().await {
            {
                let mut state = state.borrow_mut();
                match event {
                    Event::WorkspaceChanged { index } => state.active = index,
                    Event::FocusChanged { id: None } => state.title.clear(),
                    Event::FocusChanged { id: Some(id) } => {
                        // Заголовок запрашивается отдельно: событие несёт
                        // только идентификатор окна.
                        if let Ok(mut client) = Client::connect_default() {
                            if let Ok(Response::Windows { windows }) =
                                client.request(Request::ListWindows)
                            {
                                state.title = windows
                                    .iter()
                                    .find(|w| w.id == id)
                                    .map(|w| w.title.clone())
                                    .unwrap_or_default();
                            }
                        }
                    }
                    Event::WindowChanged { window } if window.focused => {
                        state.title = window.title.clone();
                    }
                    _ => {}
                }
            }
            render(&widgets, &state.borrow());
        }
    });
}

struct PanelState {
    active: u8,
    count: u8,
    title: String,
}

fn render(widgets: &Rc<PanelWidgets>, state: &PanelState) {
    while let Some(child) = widgets.workspaces.first_child() {
        widgets.workspaces.remove(&child);
    }

    for index in 1..=state.count.max(1) {
        let button = Button::with_label(&index.to_string());
        button.add_css_class("flat");
        button.add_css_class("hype-pill");
        if index == state.active {
            button.add_css_class("hype-selected");
            button.add_css_class("hype-accent");
        }
        button.connect_clicked(move |_| {
            if let Ok(mut client) = Client::connect_default() {
                let _ = client.request(Request::Dispatch {
                    action: hype_config::Action::Workspace { index },
                });
            }
        });
        widgets.workspaces.append(&button);
    }

    widgets
        .title
        .set_text(&shorten_title(&state.title, TITLE_CHARS));
}
