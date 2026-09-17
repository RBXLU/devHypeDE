//! Окно настроек.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider,
    DrawingArea, DropDown, Entry, HeaderBar, Label, Orientation, Scale, ScrolledWindow, Separator,
    SpinButton, Stack, StackSidebar, Switch,
};
use hype_config::{Config, LayoutMode, PanelPosition};
use hype_theme::{Color, Palette, Variant};

use crate::presets::{motion_label, parse_accent, PRESETS};

/// Состояние окна: правится копия настроек, на диск она попадает по кнопке.
struct State {
    config: Config,
    status: Label,
}

impl State {
    /// Сообщает, что изменения ещё не применены.
    fn mark_dirty(&self) {
        self.status.set_text("Есть несохранённые изменения");
    }
}

type Shared = Rc<RefCell<State>>;

/// Собирает и показывает окно настроек.
pub fn build(app: &Application) {
    load_theme_css();

    let loaded = hype_config::load().unwrap_or_else(|err| {
        tracing::warn!("не удалось прочитать настройки: {err}");
        hype_config::LoadedConfig {
            config: Config::default(),
            warnings: vec![format!("настройки не прочитаны: {err}")],
            source: None,
        }
    });

    let status = Label::new(Some("Настройки прочитаны"));
    status.add_css_class("hype-dim");
    status.set_halign(Align::Start);

    let state: Shared = Rc::new(RefCell::new(State {
        config: loaded.config,
        status: status.clone(),
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Параметры HypeDE")
        .default_width(920)
        .default_height(660)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);
    stack.set_transition_duration(180);

    let preview = DrawingArea::new();
    preview.set_content_height(64);
    draw_palette_preview(&preview, &state);

    stack.add_titled(
        &page_appearance(&state, &preview),
        Some("appearance"),
        "Оформление",
    );
    stack.add_titled(&page_motion(&state), Some("motion"), "Анимации");
    stack.add_titled(&page_windows(&state), Some("windows"), "Окна");
    stack.add_titled(&page_input(&state), Some("input"), "Ввод и клавиатура");
    stack.add_titled(&page_keys(&state), Some("keys"), "Горячие клавиши");

    let sidebar = StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_width_request(210);

    let apply = Button::with_label("Применить");
    apply.add_css_class("hype-primary");
    apply.add_css_class("suggested-action");

    let header = HeaderBar::new();
    header.pack_end(&apply);
    window.set_titlebar(Some(&header));

    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(6);
    footer.append(&status);

    let right = GtkBox::new(Orientation::Vertical, 0);
    right.append(&stack);
    right.append(&Separator::new(Orientation::Horizontal));
    right.append(&footer);
    stack.set_vexpand(true);

    let layout = GtkBox::new(Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&Separator::new(Orientation::Vertical));
    layout.append(&right);
    right.set_hexpand(true);
    window.set_child(Some(&layout));

    apply.connect_clicked({
        let state = Rc::clone(&state);
        move |_| apply_settings(&state)
    });

    for warning in &loaded.warnings {
        tracing::warn!("настройки: {warning}");
    }
    if !loaded.warnings.is_empty() {
        status.set_text(&format!(
            "Замечаний к настройкам: {}",
            loaded.warnings.len()
        ));
    }

    window.present();
}

// --- страницы ---

fn page_appearance(state: &Shared, preview: &DrawingArea) -> ScrolledWindow {
    let page = page_box();

    page.append(&section_title("Акцентный цвет"));
    page.append(&hint(
        "Из одного цвета строится вся палитра: поверхности, текст, выделение.\n\
         Читаемость проверяется автоматически, поэтому испортить её выбором цвета нельзя.",
    ));

    let swatches = GtkBox::new(Orientation::Horizontal, 8);
    swatches.set_margin_top(4);

    let entry = Entry::new();
    entry.set_text(&state.borrow().config.theme.accent.to_hex());
    entry.set_max_width_chars(10);

    for preset in PRESETS {
        let button = Button::new();
        button.set_tooltip_text(Some(preset.name));
        button.set_size_request(40, 32);
        button.add_css_class("hype-swatch");

        // Цвет кнопки задаётся точечным CSS: так образец показывает ровно тот
        // оттенок, который получит пользователь.
        let provider = CssProvider::new();
        let css = format!(
            ".hype-swatch-{} {{ background-image: none; background-color: {}; border-radius: 8px; }}",
            preset.hex.trim_start_matches('#'),
            preset.hex
        );
        provider.load_from_data(&css);
        button.add_css_class(&format!(
            "hype-swatch-{}",
            preset.hex.trim_start_matches('#')
        ));
        button
            .style_context()
            .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

        button.connect_clicked({
            let state = Rc::clone(state);
            let entry = entry.clone();
            let preview = preview.clone();
            move |_| {
                let color = Color::from_hex(preset.hex).expect("готовый акцент всегда разбирается");
                state.borrow_mut().config.theme.accent = color;
                state.borrow().mark_dirty();
                entry.set_text(preset.hex);
                preview.queue_draw();
            }
        });

        swatches.append(&button);
    }
    page.append(&swatches);

    let custom = GtkBox::new(Orientation::Horizontal, 8);
    custom.set_margin_top(8);
    custom.append(&Label::new(Some("Свой цвет:")));
    custom.append(&entry);
    let error = Label::new(None);
    error.add_css_class("hype-dim");
    custom.append(&error);
    page.append(&custom);

    entry.connect_changed({
        let state = Rc::clone(state);
        let preview = preview.clone();
        let error = error.clone();
        move |entry| match parse_accent(&entry.text()) {
            Ok(color) => {
                error.set_text("");
                state.borrow_mut().config.theme.accent = color;
                state.borrow().mark_dirty();
                preview.queue_draw();
            }
            Err(message) => error.set_text(&message),
        }
    });

    page.append(&section_title("Палитра"));
    page.append(preview);

    page.append(&section_title("Схема"));
    let variants = DropDown::from_strings(&["Тёмная", "Светлая"]);
    variants.set_selected(if state.borrow().config.theme.variant == Variant::Dark {
        0
    } else {
        1
    });
    variants.connect_selected_notify({
        let state = Rc::clone(state);
        let preview = preview.clone();
        move |dropdown| {
            let variant = if dropdown.selected() == 0 {
                Variant::Dark
            } else {
                Variant::Light
            };
            state.borrow_mut().config.theme.variant = variant;
            state.borrow().mark_dirty();
            preview.queue_draw();
        }
    });
    page.append(&labelled("Схема оформления", &variants));

    page.append(&section_title("Шрифт и формы"));
    page.append(&scale_row(
        state,
        "Размер шрифта",
        8.0,
        16.0,
        0.5,
        state.borrow().config.theme.typography.size_pt,
        |config, value| config.theme.typography.size_pt = value,
    ));
    page.append(&scale_row(
        state,
        "Скругление окон",
        0.0,
        24.0,
        1.0,
        state.borrow().config.theme.radii.window,
        |config, value| config.theme.radii.window = value,
    ));
    page.append(&scale_row(
        state,
        "Непрозрачность панели",
        0.3,
        1.0,
        0.02,
        state.borrow().config.theme.effects.panel_opacity,
        |config, value| config.theme.effects.panel_opacity = value,
    ));

    scroll(page)
}

fn page_motion(state: &Shared) -> ScrolledWindow {
    let page = page_box();

    page.append(&section_title("Темп анимаций"));
    page.append(&hint(
        "Ноль полностью выключает движение — это же значение стоит выбрать,\n\
         если от анимаций устают глаза.",
    ));

    let label = Label::new(Some(&motion_label(
        state.borrow().config.theme.motion.scale,
    )));
    label.set_halign(Align::Start);
    label.add_css_class("hype-accent");

    let adjustment = Adjustment::new(
        state.borrow().config.theme.motion.scale,
        0.0,
        3.0,
        0.1,
        0.5,
        0.0,
    );
    let scale = Scale::new(Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.add_mark(0.0, gtk4::PositionType::Bottom, Some("выкл"));
    scale.add_mark(1.0, gtk4::PositionType::Bottom, Some("1×"));
    scale.add_mark(2.0, gtk4::PositionType::Bottom, Some("2×"));

    scale.connect_value_changed({
        let state = Rc::clone(state);
        let label = label.clone();
        move |scale| {
            let value = scale.value();
            state.borrow_mut().config.theme.motion.scale = value;
            state.borrow().mark_dirty();
            label.set_text(&motion_label(value));
        }
    });

    page.append(&scale);
    page.append(&label);

    page.append(&section_title("Отдельные анимации"));
    page.append(&switch_row(
        state,
        "Появление окна",
        state.borrow().config.animations.window_open.enabled,
        |config, on| config.animations.window_open.enabled = on,
    ));
    page.append(&switch_row(
        state,
        "Закрытие окна",
        state.borrow().config.animations.window_close.enabled,
        |config, on| config.animations.window_close.enabled = on,
    ));
    page.append(&switch_row(
        state,
        "Перестроение раскладки",
        state.borrow().config.animations.window_move.enabled,
        |config, on| config.animations.window_move.enabled = on,
    ));
    page.append(&switch_row(
        state,
        "Переключение рабочего стола",
        state.borrow().config.animations.workspace_switch.enabled,
        |config, on| config.animations.workspace_switch.enabled = on,
    ));

    scroll(page)
}

fn page_windows(state: &Shared) -> ScrolledWindow {
    let page = page_box();

    page.append(&section_title("Раскладка"));
    let modes = DropDown::from_strings(&["Плитка", "Свободное размещение"]);
    modes.set_selected(if state.borrow().config.layout.mode == LayoutMode::Tiling {
        0
    } else {
        1
    });
    modes.connect_selected_notify({
        let state = Rc::clone(state);
        move |dropdown| {
            state.borrow_mut().config.layout.mode = if dropdown.selected() == 0 {
                LayoutMode::Tiling
            } else {
                LayoutMode::Floating
            };
            state.borrow().mark_dirty();
        }
    });
    page.append(&labelled("Как расставлять окна", &modes));

    page.append(&scale_row(
        state,
        "Зазор между окнами",
        0.0,
        40.0,
        1.0,
        state.borrow().config.layout.gaps_inner,
        |config, value| config.layout.gaps_inner = value,
    ));
    page.append(&scale_row(
        state,
        "Отступ от краёв экрана",
        0.0,
        60.0,
        1.0,
        state.borrow().config.layout.gaps_outer,
        |config, value| config.layout.gaps_outer = value,
    ));
    page.append(&scale_row(
        state,
        "Толщина рамки",
        0.0,
        8.0,
        1.0,
        state.borrow().config.layout.border_width,
        |config, value| config.layout.border_width = value,
    ));
    page.append(&scale_row(
        state,
        "Доля главного окна",
        0.2,
        0.8,
        0.05,
        state.borrow().config.layout.master_ratio,
        |config, value| config.layout.master_ratio = value,
    ));

    page.append(&section_title("Рабочие столы"));
    let adjustment = Adjustment::new(
        state.borrow().config.layout.workspaces as f64,
        1.0,
        20.0,
        1.0,
        1.0,
        0.0,
    );
    let spin = SpinButton::new(Some(&adjustment), 1.0, 0);
    spin.connect_value_changed({
        let state = Rc::clone(state);
        move |spin| {
            state.borrow_mut().config.layout.workspaces = spin.value() as u8;
            state.borrow().mark_dirty();
        }
    });
    page.append(&labelled("Количество рабочих столов", &spin));

    page.append(&section_title("Панель"));
    page.append(&switch_row(
        state,
        "Показывать панель",
        state.borrow().config.panel.enabled,
        |config, on| config.panel.enabled = on,
    ));
    let positions = DropDown::from_strings(&["Сверху", "Снизу"]);
    positions.set_selected(
        if state.borrow().config.panel.position == PanelPosition::Top {
            0
        } else {
            1
        },
    );
    positions.connect_selected_notify({
        let state = Rc::clone(state);
        move |dropdown| {
            state.borrow_mut().config.panel.position = if dropdown.selected() == 0 {
                PanelPosition::Top
            } else {
                PanelPosition::Bottom
            };
            state.borrow().mark_dirty();
        }
    });
    page.append(&labelled("Положение панели", &positions));

    scroll(page)
}

fn page_input(state: &Shared) -> ScrolledWindow {
    let page = page_box();

    page.append(&section_title("Клавиатура"));
    page.append(&hint(
        "Раскладки перечисляются через запятую в терминах xkb: us,ru.\n\
         Способ переключения задаётся параметром вроде grp:alt_shift_toggle.",
    ));

    let layout = Entry::new();
    layout.set_text(&state.borrow().config.input.keyboard_layout);
    layout.connect_changed({
        let state = Rc::clone(state);
        move |entry| {
            state.borrow_mut().config.input.keyboard_layout = entry.text().to_string();
            state.borrow().mark_dirty();
        }
    });
    page.append(&labelled("Раскладки", &layout));

    let options = Entry::new();
    options.set_text(&state.borrow().config.input.keyboard_options);
    options.connect_changed({
        let state = Rc::clone(state);
        move |entry| {
            state.borrow_mut().config.input.keyboard_options = entry.text().to_string();
            state.borrow().mark_dirty();
        }
    });
    page.append(&labelled("Параметры xkb", &options));

    page.append(&section_title("Мышь и тачпад"));
    page.append(&switch_row(
        state,
        "Касание считается щелчком",
        state.borrow().config.input.tap_to_click,
        |config, on| config.input.tap_to_click = on,
    ));
    page.append(&switch_row(
        state,
        "Естественная прокрутка",
        state.borrow().config.input.natural_scroll,
        |config, on| config.input.natural_scroll = on,
    ));
    page.append(&switch_row(
        state,
        "Фокус следует за курсором",
        state.borrow().config.input.focus_follows_mouse,
        |config, on| config.input.focus_follows_mouse = on,
    ));
    page.append(&scale_row(
        state,
        "Ускорение указателя",
        -1.0,
        1.0,
        0.1,
        state.borrow().config.input.pointer_accel,
        |config, value| config.input.pointer_accel = value,
    ));

    scroll(page)
}

fn page_keys(state: &Shared) -> ScrolledWindow {
    let page = page_box();
    page.append(&section_title("Горячие клавиши"));
    page.append(&hint(
        "Список читается из config.toml. Правка прямо здесь появится позже —\n\
         пока сочетания меняются в файле настроек.",
    ));

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(24);
    grid.set_margin_top(8);

    for (row, binding) in state.borrow().config.keybinds.iter().enumerate() {
        let keys = Label::new(Some(&binding.keys.to_string()));
        keys.add_css_class("hype-accent");
        keys.set_halign(Align::Start);
        keys.set_width_chars(20);

        let action = Label::new(Some(&binding.action.description()));
        action.set_halign(Align::Start);

        grid.attach(&keys, 0, row as i32, 1, 1);
        grid.attach(&action, 1, row as i32, 1, 1);
    }

    page.append(&grid);
    scroll(page)
}

// --- вспомогательные виджеты ---

fn page_box() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_start(20);
    page.set_margin_end(20);
    page.set_margin_top(16);
    page.set_margin_bottom(16);
    page
}

fn scroll(child: GtkBox) -> ScrolledWindow {
    ScrolledWindow::builder()
        .child(&child)
        .hexpand(true)
        .vexpand(true)
        .build()
}

fn section_title(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(Align::Start);
    label.set_margin_top(14);
    label.add_css_class("heading");
    label.add_css_class("hype-accent");
    label
}

fn hint(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
    label.add_css_class("hype-dim");
    label.set_wrap(true);
    label
}

fn labelled(title: &str, widget: &impl IsA<gtk4::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_top(4);
    let label = Label::new(Some(title));
    label.set_halign(Align::Start);
    label.set_hexpand(true);
    row.append(&label);
    row.append(widget);
    row
}

fn switch_row(state: &Shared, title: &str, initial: bool, apply: fn(&mut Config, bool)) -> GtkBox {
    let switch = Switch::new();
    switch.set_active(initial);
    switch.set_valign(Align::Center);
    switch.connect_active_notify({
        let state = Rc::clone(state);
        move |switch| {
            let mut state = state.borrow_mut();
            apply(&mut state.config, switch.is_active());
            state.mark_dirty();
        }
    });
    labelled(title, &switch)
}

fn scale_row(
    state: &Shared,
    title: &str,
    min: f64,
    max: f64,
    step: f64,
    initial: f64,
    apply: fn(&mut Config, f64),
) -> GtkBox {
    let adjustment = Adjustment::new(initial.clamp(min, max), min, max, step, step * 2.0, 0.0);
    let scale = Scale::new(Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_hexpand(true);
    scale.set_size_request(280, -1);
    // Значение вроде 0.55000000001 выглядит поломкой, поэтому округляем показ.
    scale.set_digits(if step < 0.1 { 2 } else { 1 });

    scale.connect_value_changed({
        let state = Rc::clone(state);
        move |scale| {
            let mut state = state.borrow_mut();
            apply(&mut state.config, scale.value());
            state.mark_dirty();
        }
    });

    labelled(title, &scale)
}

/// Рисует полосу с цветами текущей палитры.
fn draw_palette_preview(area: &DrawingArea, state: &Shared) {
    let state = Rc::clone(state);
    area.set_draw_func(move |_, context, width, height| {
        let config = &state.borrow().config;
        let palette = Palette::from_accent(config.theme.accent, config.theme.variant);

        let swatches = [
            palette.bg,
            palette.surface,
            palette.surface_raised,
            palette.overlay,
            palette.accent_bg,
            palette.accent,
            palette.success_bg,
            palette.warning_bg,
            palette.error_bg,
            palette.fg,
        ];

        let step = width as f64 / swatches.len() as f64;
        for (index, color) in swatches.iter().enumerate() {
            context.set_source_rgb(color.r, color.g, color.b);
            context.rectangle(index as f64 * step, 0.0, step, height as f64);
            let _ = context.fill();
        }
    });
}

/// Сохраняет настройки и просит композитор их применить.
fn apply_settings(state: &Shared) {
    let config = state.borrow().config.clone();

    let warnings = config.validate();
    for warning in &warnings {
        tracing::warn!("настройки: {warning}");
    }

    let saved = match hype_config::save(&config) {
        Ok(path) => {
            tracing::info!("настройки записаны в {}", path.display());
            true
        }
        Err(err) => {
            state
                .borrow()
                .status
                .set_text(&format!("Не удалось сохранить: {err}"));
            false
        }
    };
    if !saved {
        return;
    }

    if let Err(err) = hype_config::export_theme_css(&config) {
        tracing::warn!("не удалось выгрузить CSS темы: {err}");
    }

    // Композитор может быть не запущен — приложение настроек работает и
    // отдельно, просто без мгновенного применения.
    let message = match hype_ipc::Client::connect_default() {
        Ok(mut client) => match client.request(hype_ipc::Request::ApplyConfig {
            config: Box::new(config),
        }) {
            Ok(response) if !response.is_error() => "Применено".to_string(),
            Ok(_) => "Сохранено, но композитор отказался применять".to_string(),
            Err(err) => format!("Сохранено; композитор не ответил: {err}"),
        },
        Err(_) => "Сохранено. Применится при следующем входе в HypeDE".to_string(),
    };

    let suffix = if warnings.is_empty() {
        String::new()
    } else {
        format!(" · замечаний: {}", warnings.len())
    };
    state
        .borrow()
        .status
        .set_text(&format!("{message}{suffix}"));
}

/// Подключает CSS темы и значки среды.
///
/// Если файла темы нет, приложение берёт системную: его можно запустить и вне
/// сеанса HypeDE.
fn load_theme_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    // Собственные значки приложений находятся и без установки пакетом.
    let icons = gtk4::IconTheme::for_display(&display);
    for dir in hype_config::paths::asset_dirs() {
        let path = dir.join("icons");
        if path.is_dir() {
            icons.add_search_path(&path);
        }
    }

    let Some(path) = hype_config::paths::generated_css_file() else {
        return;
    };
    if !path.exists() {
        return;
    }

    let provider = CssProvider::new();
    provider.load_from_file(&gtk4::gio::File::for_path(&path));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
