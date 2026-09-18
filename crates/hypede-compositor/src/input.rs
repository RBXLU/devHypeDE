//! Обработка ввода: клавиатура, указатель, прокрутка.

use hype_config::{Key, Mods, Shortcut};
use hype_ipc::Event;
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event as BackendEvent, InputBackend,
    InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::input::keyboard::{xkb, FilterResult, ModifiersState};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::utils::SERIAL_COUNTER;

use crate::state::HypeState;

/// Первый код клавиши переключения виртуальной консоли (`XF86Switch_VT_1`).
///
/// Ctrl+Alt+F1…F12 приходят от xkb именно такими кодами, идущими подряд.
const VT_SWITCH_FIRST: u32 = 0x1008_FE01;
/// Сколько таких клавиш определено.
const VT_SWITCH_COUNT: u32 = 12;

/// Номер консоли, на которую просят переключиться.
///
/// Обрабатывать это обязан композитор: он держит видеокарту и клавиатуру, и
/// без его участия уйти с сеанса нечем — ни одно приложение такую клавишу не
/// перехватит.
fn vt_from_keysym(raw: u32) -> Option<i32> {
    (VT_SWITCH_FIRST..VT_SWITCH_FIRST + VT_SWITCH_COUNT)
        .contains(&raw)
        .then(|| (raw - VT_SWITCH_FIRST + 1) as i32)
}

/// Код клавиши Backspace в xkb.
const KEYSYM_BACKSPACE: u32 = 0xFF08;

/// Аварийный выход из сеанса — Ctrl+Alt+Backspace.
///
/// Проверяется до настроек и не может быть из них убран. Своя привязка для
/// выхода в конфиге есть, но пользователь волен заменить весь набор привязок
/// целиком, и тогда без этой лазейки из сеанса не выйти вообще.
fn is_emergency_exit(modifiers: &ModifiersState, raw: u32) -> bool {
    modifiers.ctrl && modifiers.alt && raw == KEYSYM_BACKSPACE
}

/// Что делать с нажатой клавишей.
enum KeyAction {
    /// Выполнить действие среды из настроек.
    Bound(hype_config::Action),
    /// Переключиться на другую виртуальную консоль.
    SwitchVt(i32),
    /// Завершить сеанс, что бы ни было в настройках.
    EmergencyExit,
}

impl HypeState {
    /// Разбирает событие ввода от бэкенда.
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => self.on_keyboard::<I>(event),
            InputEvent::PointerMotionAbsolute { event, .. } => self.on_pointer_absolute::<I>(event),
            InputEvent::PointerButton { event, .. } => self.on_pointer_button::<I>(event),
            InputEvent::PointerAxis { event, .. } => self.on_pointer_axis::<I>(event),
            _ => {}
        }
    }

    fn on_keyboard<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let time = BackendEvent::time_msec(&event);
        let key_state = event.state();
        let keycode = event.key_code();

        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };

        // Список привязок копируется до вызова: внутри замыкания состояние
        // композитора уже занято как изменяемое.
        let bindings = self.config.keybinds.clone();

        let action = keyboard.input(
            self,
            keycode,
            key_state,
            serial,
            time,
            |_state, modifiers, handle| {
                if key_state != KeyState::Pressed {
                    return FilterResult::Forward;
                }

                // Переключение консоли проверяется до всего остального и по
                // текущему символу: это единственный способ уйти из сеанса,
                // и он обязан работать, что бы ни было в настройках.
                for sym in handle.raw_syms() {
                    if let Some(vt) = vt_from_keysym(sym.raw()) {
                        return FilterResult::Intercept(KeyAction::SwitchVt(vt));
                    }
                    if is_emergency_exit(modifiers, sym.raw()) {
                        return FilterResult::Intercept(KeyAction::EmergencyExit);
                    }
                }

                // Берётся латинский символ клавиши, а не набранный: привязка
                // Super+Q обязана работать и в кириллической раскладке.
                let sym = handle
                    .raw_latin_sym_or_raw_current_sym()
                    .unwrap_or_else(|| handle.modified_sym());

                let Some(shortcut) = shortcut_from(modifiers, &xkb::keysym_get_name(sym)) else {
                    return FilterResult::Forward;
                };

                match bindings.iter().find(|b| b.keys == shortcut) {
                    Some(binding) => {
                        FilterResult::Intercept(KeyAction::Bound(binding.action.clone()))
                    }
                    None => FilterResult::Forward,
                }
            },
        );

        match action {
            Some(KeyAction::Bound(action)) => self.dispatch(&action),
            Some(KeyAction::SwitchVt(vt)) => self.switch_vt(vt),
            Some(KeyAction::EmergencyExit) => {
                tracing::info!("аварийный выход по Ctrl+Alt+Backspace");
                self.dispatch(&hype_config::Action::Quit);
            }
            None => {}
        }
    }

    /// Переключается на другую виртуальную консоль.
    ///
    /// Во вложенном режиме консолей нет, и запрос просто игнорируется.
    pub fn switch_vt(&mut self, vt: i32) {
        use smithay::backend::session::Session;

        let Some(drm) = self.drm.as_mut() else {
            tracing::debug!("переключение консоли доступно только в сеансе на видеокарте");
            return;
        };

        match drm.session.change_vt(vt) {
            Ok(()) => tracing::info!("переключение на консоль {vt}"),
            Err(err) => tracing::warn!("не удалось переключиться на консоль {vt}: {err}"),
        }
    }

    fn on_pointer_absolute<I: InputBackend>(&mut self, event: I::PointerMotionAbsoluteEvent) {
        let Some(output) = self.space.outputs().next().cloned() else {
            return;
        };
        let Some(geometry) = self.space.output_geometry(&output) else {
            return;
        };

        let position = event.position_transformed(geometry.size) + geometry.loc.to_f64();
        let serial = SERIAL_COUNTER.next_serial();
        let under = self.surface_under(position);

        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: position,
                serial,
                time: event.time_msec(),
            },
        );
        pointer.frame(self);

        if self.config.input.focus_follows_mouse {
            self.focus_window_under_pointer(position);
        }
    }

    fn on_pointer_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let state = event.state();

        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            let location = pointer.current_location();
            self.focus_window_under_pointer(location);
        }

        pointer.button(
            self,
            &ButtonEvent {
                button: event.button_code(),
                state,
                serial,
                time: event.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_pointer_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
        let source = event.source();
        // Часть устройств сообщает только дискретные шаги — переводим их в
        // привычные единицы, иначе прокрутка в приложениях не работает.
        let amount = |axis: Axis| {
            event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
        };

        let natural = if self.config.input.natural_scroll {
            -1.0
        } else {
            1.0
        };
        let mut frame = AxisFrame::new(event.time_msec()).source(source);

        for axis in [Axis::Horizontal, Axis::Vertical] {
            let value = amount(axis) * natural;
            if value != 0.0 {
                frame = frame.value(axis, value);
                if let Some(discrete) = event.amount_v120(axis) {
                    frame = frame.v120(axis, (discrete * natural) as i32);
                }
            } else if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                // Палец оторвали от тачпада — прокрутка должна остановиться,
                // иначе приложение продолжит считать её активной.
                frame = frame.stop(axis);
            }
        }

        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// Переводит фокус на окно под указателем.
    fn focus_window_under_pointer(
        &mut self,
        position: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) {
        let Some((window, _)) = self
            .space
            .element_under(position)
            .map(|(w, l)| (w.clone(), l))
        else {
            return;
        };

        let Some(id) = self
            .windows
            .values()
            .find(|m| m.window == window)
            .map(|m| m.id)
        else {
            return;
        };

        if self.workspaces.focused() == Some(id) {
            return;
        }

        if self.workspaces.focus_window(id) {
            self.space.raise_element(&window, true);
            self.update_keyboard_focus();
            self.sync_space();
            self.redraw_needed = true;
            self.broadcast(&Event::FocusChanged { id: Some(id) });
        }
    }
}

/// Собирает сочетание клавиш из состояния модификаторов и имени клавиши.
///
/// Возвращает `None`, если клавиша не имеет осмысленного имени (например,
/// нажат сам модификатор) — такие события уходят приложению без изменений.
fn shortcut_from(modifiers: &ModifiersState, key_name: &str) -> Option<Shortcut> {
    let key = Key::from_name(key_name).ok()?;
    Some(Shortcut {
        mods: Mods {
            logo: modifiers.logo,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifiers(logo: bool, ctrl: bool, alt: bool, shift: bool) -> ModifiersState {
        ModifiersState {
            logo,
            ctrl,
            alt,
            shift,
            ..Default::default()
        }
    }

    #[test]
    fn console_switch_keys_are_recognised() {
        // Ctrl+Alt+F1 … Ctrl+Alt+F12 идут подряд начиная с XF86Switch_VT_1.
        assert_eq!(vt_from_keysym(0x1008_FE01), Some(1));
        assert_eq!(vt_from_keysym(0x1008_FE02), Some(2));
        assert_eq!(vt_from_keysym(0x1008_FE0C), Some(12));
    }

    #[test]
    fn other_keys_are_not_mistaken_for_console_switches() {
        assert_eq!(vt_from_keysym(0x1008_FE00), None);
        assert_eq!(vt_from_keysym(0x1008_FE0D), None);
        // Обычная буква.
        assert_eq!(vt_from_keysym(0x0071), None);
        // Мультимедийная клавиша из того же диапазона XF86.
        assert_eq!(vt_from_keysym(0x1008_FF11), None);
    }

    #[test]
    fn the_emergency_exit_needs_both_modifiers() {
        let both = modifiers(false, true, true, false);
        assert!(is_emergency_exit(&both, KEYSYM_BACKSPACE));

        // Один модификатор — обычное удаление символа, его трогать нельзя.
        assert!(!is_emergency_exit(
            &modifiers(false, true, false, false),
            KEYSYM_BACKSPACE
        ));
        assert!(!is_emergency_exit(
            &modifiers(false, false, true, false),
            KEYSYM_BACKSPACE
        ));
        // Та же пара модификаторов с другой клавишей.
        assert!(!is_emergency_exit(&both, 0x0071));
    }

    #[test]
    fn a_plain_letter_becomes_a_shortcut() {
        let shortcut = shortcut_from(&modifiers(true, false, false, false), "q").unwrap();
        assert_eq!(shortcut, "Super+Q".parse().unwrap());
    }

    #[test]
    fn every_modifier_is_carried_over() {
        let shortcut = shortcut_from(&modifiers(true, true, true, true), "k").unwrap();
        assert_eq!(shortcut, "Super+Ctrl+Alt+Shift+K".parse().unwrap());
    }

    #[test]
    fn named_keys_from_xkb_are_recognised() {
        assert_eq!(
            shortcut_from(&modifiers(true, false, false, false), "Return").unwrap(),
            "Super+Return".parse().unwrap()
        );
        assert_eq!(
            shortcut_from(&modifiers(false, false, false, false), "Print").unwrap(),
            "Print".parse().unwrap()
        );
    }

    #[test]
    fn a_modifier_press_alone_is_not_a_shortcut() {
        // xkb отдаёт такие имена при нажатии самого модификатора.
        assert!(shortcut_from(&modifiers(true, false, false, false), "Super_L").is_none());
        assert!(shortcut_from(&modifiers(false, false, false, false), "NoSymbol").is_none());
    }
}
