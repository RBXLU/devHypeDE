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

                // Берётся латинский символ клавиши, а не набранный: привязка
                // Super+Q обязана работать и в кириллической раскладке.
                let sym = handle
                    .raw_latin_sym_or_raw_current_sym()
                    .unwrap_or_else(|| handle.modified_sym());

                let Some(shortcut) = shortcut_from(modifiers, &xkb::keysym_get_name(sym)) else {
                    return FilterResult::Forward;
                };

                match bindings.iter().find(|b| b.keys == shortcut) {
                    Some(binding) => FilterResult::Intercept(binding.action.clone()),
                    None => FilterResult::Forward,
                }
            },
        );

        if let Some(action) = action {
            self.dispatch(&action);
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

        let natural = if self.config.input.natural_scroll { -1.0 } else { 1.0 };
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
    fn focus_window_under_pointer(&mut self, position: smithay::utils::Point<f64, smithay::utils::Logical>) {
        let Some((window, _)) = self.space.element_under(position).map(|(w, l)| (w.clone(), l))
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
