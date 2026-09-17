//! Исполнение действий среды.
//!
//! Сюда сходятся и горячие клавиши, и команды по протоколу управления: список
//! действий один, значит и поведение одинаковое.

use hype_anim::Rect;
use hype_config::{Action, Config, Direction, ScreenshotTarget};
use hype_ipc::Event;
use smithay::utils::SERIAL_COUNTER;
use tracing::{info, warn};

use crate::state::HypeState;

impl HypeState {
    /// Выполняет действие среды.
    pub fn dispatch(&mut self, action: &Action) {
        match action {
            Action::Spawn { .. } => self.spawn(action),
            Action::CloseWindow => self.close_focused(),
            Action::ToggleFullscreen => self.toggle_fullscreen(),
            Action::ToggleMaximized => self.toggle_maximized(),
            Action::ToggleFloating => self.toggle_floating(),
            Action::FocusDirection { direction } => self.focus_direction(*direction),
            Action::MoveWindow { direction } => self.move_window(*direction),
            Action::ResizeWindow { direction, delta } => self.resize_window(*direction, *delta),
            Action::Workspace { index } => self.switch_workspace(*index),
            Action::MoveToWorkspace { index } => self.move_to_workspace(*index),
            Action::NextWorkspace => {
                self.workspaces.activate_next();
                self.after_workspace_change();
            }
            Action::PrevWorkspace => {
                self.workspaces.activate_prev();
                self.after_workspace_change();
            }
            Action::ToggleLauncher => self.spawn_helper("hype-shell", &["--launcher"]),
            Action::ToggleOverview => self.spawn_helper("hype-shell", &["--overview"]),
            Action::Screenshot { target } => self.screenshot(*target),
            Action::ReloadConfig => self.reload_config(),
            Action::Quit => {
                info!("завершение сеанса по запросу пользователя");
                self.loop_signal.stop();
            }
        }
    }

    /// Запускает программу из действия `spawn`.
    fn spawn(&self, action: &Action) {
        let Some(argv) = action.command_line() else {
            warn!("не удалось разобрать команду запуска");
            return;
        };
        self.spawn_helper(&argv[0], &argv[1..]);
    }

    /// Запускает программу в окружении текущего сеанса Wayland.
    fn spawn_helper<S: AsRef<std::ffi::OsStr>>(&self, program: &str, args: &[S]) {
        let mut command = std::process::Command::new(program);
        command.args(args);
        // Запускаемая программа должна попасть именно в наш сеанс, а не в тот,
        // изнутри которого запущен вложенный композитор.
        command.env("WAYLAND_DISPLAY", &self.socket_name);

        match command.spawn() {
            Ok(child) => info!("запущено: {program} (pid {})", child.id()),
            Err(err) => warn!("не удалось запустить {program}: {err}"),
        }
    }

    fn close_focused(&mut self) {
        let Some(id) = self.workspaces.focused() else {
            return;
        };
        let Some(managed) = self.windows.get(&id) else {
            return;
        };
        // Окно просят закрыться само: так у приложения остаётся возможность
        // спросить про несохранённые изменения.
        if let Some(toplevel) = managed.window.toplevel() {
            toplevel.send_close();
        }
    }

    fn toggle_fullscreen(&mut self) {
        let Some(id) = self.workspaces.focused() else {
            return;
        };
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.fullscreen = !managed.fullscreen;
            let fullscreen = managed.fullscreen;
            if let Some(toplevel) = managed.window.toplevel() {
                toplevel.with_pending_state(|state| {
                    if fullscreen {
                        state.states.set(
                            smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Fullscreen,
                        );
                    } else {
                        state.states.unset(
                            smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Fullscreen,
                        );
                    }
                });
            }
        }
        self.relayout();
        self.notify_window_changed(id);
    }

    fn toggle_maximized(&mut self) {
        // В плитке окно и так занимает всю отведённую площадь, поэтому
        // «развернуть» осмысленно только для плавающего окна: оно занимает всё
        // рабочее поле и возвращается обратно повторным нажатием.
        let Some(id) = self.workspaces.focused() else {
            return;
        };
        let area = self.work_area();
        let Some(managed) = self.windows.get_mut(&id) else {
            return;
        };
        if !managed.floating {
            self.toggle_fullscreen();
            return;
        }

        let maximized = managed.floating_rect.size == area.size;
        managed.floating_rect = if maximized {
            area.scaled_around_center(0.6)
        } else {
            area
        };
        self.relayout();
        self.notify_window_changed(id);
    }

    fn toggle_floating(&mut self) {
        let Some(id) = self.workspaces.focused() else {
            return;
        };
        let area = self.work_area();
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.floating = !managed.floating;
            if managed.floating {
                // Окно вылетает из плитки там же, где стояло, — так его не
                // приходится искать глазами.
                let current = managed.anim.target();
                managed.floating_rect = if current.size.w > 1.0 {
                    current
                } else {
                    area.scaled_around_center(0.6)
                };
            }
        }
        self.relayout();
        self.notify_window_changed(id);
    }

    /// Геометрия видимых окон — нужна для переходов фокуса по направлению.
    fn visible_geometry(&self) -> Vec<(u64, Rect)> {
        self.workspaces
            .visible_windows()
            .iter()
            .filter_map(|id| Some((*id, self.windows.get(id)?.anim.target())))
            .collect()
    }

    fn focus_direction(&mut self, direction: Direction) {
        let geometry = self.visible_geometry();
        if self.workspaces.focus_direction(direction, &geometry) {
            self.update_keyboard_focus();
            self.sync_space();
            self.redraw_needed = true;
            let focused = self.workspaces.focused();
            self.broadcast(&Event::FocusChanged { id: focused });
        }
    }

    fn move_window(&mut self, direction: Direction) {
        let geometry = self.visible_geometry();
        if self.workspaces.move_window(direction, &geometry) {
            self.relayout();
        }
    }

    fn resize_window(&mut self, direction: Direction, delta: i32) {
        let Some(id) = self.workspaces.focused() else {
            return;
        };
        let floating = self.windows.get(&id).is_some_and(|m| m.floating);

        if floating {
            let area = self.work_area();
            if let Some(managed) = self.windows.get_mut(&id) {
                let (dx, dy) = direction.vector();
                let rect = managed.floating_rect;
                managed.floating_rect = Rect::new(
                    rect.origin.x,
                    rect.origin.y,
                    (rect.size.w + dx * delta as f64).clamp(120.0, area.size.w),
                    (rect.size.h + dy * delta as f64).clamp(80.0, area.size.h),
                );
            }
        } else {
            // В плитке изменение размера — это сдвиг границы между главным
            // окном и стопкой.
            let area = self.work_area();
            let step = delta as f64 / area.size.w.max(1.0);
            let (dx, _) = direction.vector();
            self.config.layout.master_ratio =
                (self.config.layout.master_ratio + dx * step).clamp(0.1, 0.9);
        }

        self.relayout();
        self.notify_window_changed(id);
    }

    fn switch_workspace(&mut self, index: u8) {
        if self.workspaces.active_index() == index {
            return;
        }
        if self.workspaces.activate(index) {
            self.after_workspace_change();
        }
    }

    fn move_to_workspace(&mut self, index: u8) {
        if self.workspaces.move_focused_to(index) {
            self.after_workspace_change();
        }
    }

    fn after_workspace_change(&mut self) {
        self.update_keyboard_focus();
        self.relayout();
        let index = self.workspaces.active_index();
        self.broadcast(&Event::WorkspaceChanged { index });
        let focused = self.workspaces.focused();
        self.broadcast(&Event::FocusChanged { id: focused });
    }

    fn screenshot(&self, target: ScreenshotTarget) {
        // Снимок экрана делается внешней утилитой через протокол
        // wlr-screencopy; собственная реализация появится вместе с ним.
        let args: &[&str] = match target {
            ScreenshotTarget::Screen => &["output"],
            ScreenshotTarget::Window => &["window"],
            ScreenshotTarget::Region => &["area"],
        };
        self.spawn_helper("hype-shot", args);
    }

    fn reload_config(&mut self) {
        match hype_config::load() {
            Ok(loaded) => {
                for warning in &loaded.warnings {
                    warn!("настройки: {warning}");
                }
                info!("настройки перечитаны");
                self.apply_config(loaded.config);
            }
            Err(err) => warn!("не удалось перечитать настройки: {err}"),
        }
    }

    /// Применяет новые настройки к работающему сеансу.
    pub fn apply_config(&mut self, config: Config) {
        let theme_changed = config.theme != self.config.theme;
        let workspace_count = config.layout.workspaces;

        self.config = config;

        // Рабочих столов могло стать меньше — окна с исчезнувших нужно вернуть
        // пользователю, а не потерять.
        if workspace_count != self.workspaces.count() {
            let orphans: Vec<u64> = self
                .workspaces
                .all()
                .iter()
                .filter(|w| w.index > workspace_count)
                .flat_map(|w| w.windows.clone())
                .collect();

            let active = self.workspaces.active_index().min(workspace_count.max(1));
            let mut workspaces = crate::workspace::Workspaces::new(workspace_count);
            for space in self.workspaces.all() {
                if space.index <= workspace_count {
                    for window in space.windows.iter().rev() {
                        workspaces.activate(space.index);
                        workspaces.add_window(*window);
                    }
                }
            }
            workspaces.activate(1);
            for window in orphans {
                workspaces.add_window(window);
            }
            workspaces.activate(active);
            self.workspaces = workspaces;
        }

        if theme_changed {
            if let Err(err) = hype_config::export_theme_css(&self.config) {
                warn!("не удалось выгрузить CSS темы: {err}");
            }
            self.broadcast(&Event::ThemeChanged);
        }

        self.relayout();
    }

    /// Переводит клавиатурный фокус на окно, выбранное рабочим столом.
    pub fn update_keyboard_focus(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();

        let surface = self
            .workspaces
            .focused()
            .and_then(|id| self.windows.get(&id))
            .and_then(|managed| managed.window.toplevel())
            .map(|toplevel| toplevel.wl_surface().clone());

        keyboard.set_focus(self, surface, serial);
    }

    /// Сообщает подписчикам, что окно изменилось.
    pub fn notify_window_changed(&mut self, id: u64) {
        if let Some(window) = self.window_info(id) {
            self.broadcast(&Event::WindowChanged { window });
        }
    }
}
