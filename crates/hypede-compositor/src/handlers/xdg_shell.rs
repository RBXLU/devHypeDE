//! Обработка xdg-shell: окна верхнего уровня и всплывающие меню.

use hype_ipc::Event;
use smithay::delegate_xdg_shell;
use smithay::desktop::{PopupKind, Window};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::{wl_output, wl_seat, wl_surface::WlSurface};
use smithay::utils::Serial;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use tracing::debug;

use crate::state::{HypeState, ManagedWindow};
use crate::window_anim::WindowAnimation;

impl XdgShellHandler for HypeState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let id = self.allocate_window_id();
        let window = Window::new_wayland_window(surface);

        // Пока клиент не прислал первый буфер, настоящего размера у окна нет.
        // Стартовая геометрия — небольшой прямоугольник в центре рабочей
        // области: с неё начнётся анимация появления.
        let area = self.work_area();
        let initial = area.scaled_around_center(0.6);

        self.windows.insert(
            id,
            ManagedWindow {
                id,
                window,
                anim: WindowAnimation::opening(
                    initial,
                    &self.config.animations,
                    self.config.theme.motion.scale,
                ),
                floating: false,
                fullscreen: false,
                floating_rect: initial,
            },
        );

        self.workspaces.add_window(id);
        self.relayout();
        self.update_keyboard_focus();

        if let Some(info) = self.window_info(id) {
            self.broadcast(&Event::WindowOpened { window: info });
        }
        let focused = self.workspaces.focused();
        self.broadcast(&Event::FocusChanged { id: focused });
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(id) = self
            .windows
            .values()
            .find(|m| m.window.toplevel() == Some(&surface))
            .map(|m| m.id)
        else {
            return;
        };

        // Окно не исчезает мгновенно: сначала доигрывает анимация закрытия, и
        // только потом оно убирается со сцены (см. `advance_animations`).
        let animations = self.config.animations.clone();
        let motion = self.config.theme.motion.scale;
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.anim.close(&animations, motion);
        }

        self.workspaces.remove_window(id);
        self.relayout();
        self.update_keyboard_focus();

        let focused = self.workspaces.focused();
        self.broadcast(&Event::FocusChanged { id: focused });
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            debug!("не удалось взять всплывающее окно под управление: {err}");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Захват ввода всплывающим меню появится вместе с поддержкой
        // перетаскивания окон мышью.
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Перетаскивание окна мышью — следующий шаг; в плиточной раскладке оно
        // и так не требуется.
    }

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        self.set_fullscreen(&surface, true);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.set_fullscreen(&surface, false);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        // В плитке окно уже занимает всю отведённую площадь — подтверждаем
        // запрос, чтобы клиент не ждал ответа вечно.
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Maximized);
        });
        surface.send_pending_configure();
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
        });
        surface.send_pending_configure();
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.notify_toplevel_changed(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.notify_toplevel_changed(&surface);
    }
}

impl HypeState {
    fn set_fullscreen(&mut self, surface: &ToplevelSurface, fullscreen: bool) {
        let Some(id) = self
            .windows
            .values()
            .find(|m| m.window.toplevel() == Some(surface))
            .map(|m| m.id)
        else {
            return;
        };

        if let Some(managed) = self.windows.get_mut(&id) {
            managed.fullscreen = fullscreen;
        }
        surface.with_pending_state(|state| {
            if fullscreen {
                state.states.set(xdg_toplevel::State::Fullscreen);
            } else {
                state.states.unset(xdg_toplevel::State::Fullscreen);
            }
        });
        surface.send_pending_configure();

        self.relayout();
        self.notify_window_changed(id);
    }

    fn notify_toplevel_changed(&mut self, surface: &ToplevelSurface) {
        if let Some(id) = self
            .windows
            .values()
            .find(|m| m.window.toplevel() == Some(surface))
            .map(|m| m.id)
        {
            self.notify_window_changed(id);
        }
    }
}

delegate_xdg_shell!(HypeState);

/// Досылает клиенту первичную конфигурацию после первого коммита.
///
/// По протоколу xdg-shell клиент не может показать окно, пока не получит
/// configure; забыть про это — значит получить сеанс, в котором приложения
/// запускаются, но не появляются.
pub fn handle_commit(state: &mut HypeState, surface: &WlSurface) {
    state.popups.commit(surface);

    let Some(managed) = state
        .windows
        .values()
        .find(|m| m.window.toplevel().is_some_and(|t| t.wl_surface() == surface))
    else {
        return;
    };

    let Some(toplevel) = managed.window.toplevel() else {
        return;
    };

    let initial_configure_sent = with_states(surface, |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().ok().map(|d| d.initial_configure_sent))
            .unwrap_or(false)
    });

    if !initial_configure_sent {
        toplevel.send_configure();
    }
}
