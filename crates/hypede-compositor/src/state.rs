//! Состояние сеанса композитора.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hype_anim::Rect;
use hype_config::{Config, LayoutConfig};
use hype_ipc::{Event, OutputInfo, State as IpcState, WindowInfo, WorkspaceInfo};
use smithay::desktop::{PopupManager, Space, Window, WindowSurfaceType};
use smithay::input::{Seat, SeatState};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{Logical, Point};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;

use crate::ipc::IpcServer;
use crate::layout::{self, LayoutWindow};
use crate::window_anim::WindowAnimation;
use crate::workspace::Workspaces;

/// Размер окна поиска приложений, пока клиент не сообщил свой.
const LAUNCHER_WIDTH: f64 = 620.0;
/// Высота окна поиска приложений по умолчанию.
const LAUNCHER_HEIGHT: f64 = 480.0;

/// Данные, которые цикл событий передаёт обработчикам.
pub struct LoopData {
    pub state: HypeState,
    pub display_handle: DisplayHandle,
}

/// Окно под управлением HypeDE.
pub struct ManagedWindow {
    pub id: u64,
    pub window: Window,
    pub anim: WindowAnimation,
    pub floating: bool,
    pub fullscreen: bool,
    /// Геометрия плавающего окна — то, куда оно вернётся из плитки.
    pub floating_rect: Rect,
}

impl ManagedWindow {
    /// Заголовок окна, как его сообщил клиент.
    pub fn title(&self) -> String {
        self.window
            .toplevel()
            .map(|t| {
                smithay::wayland::compositor::with_states(t.wl_surface(), |states| {
                    states
                        .data_map
                        .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                        .and_then(|d| d.lock().ok()?.title.clone())
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default()
    }

    /// Идентификатор приложения (`app_id`).
    pub fn app_id(&self) -> String {
        self.window
            .toplevel()
            .map(|t| {
                smithay::wayland::compositor::with_states(t.wl_surface(), |states| {
                    states
                        .data_map
                        .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                        .and_then(|d| d.lock().ok()?.app_id.clone())
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default()
    }
}

/// Состояние композитора.
pub struct HypeState {
    pub start_time: Instant,
    pub last_frame: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,

    pub space: Space<Window>,
    pub popups: PopupManager,

    // Состояние протоколов smithay.
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<Self>,

    // Состояние самой среды.
    pub config: Config,
    pub workspaces: Workspaces,
    pub windows: HashMap<u64, ManagedWindow>,
    next_window_id: u64,
    pub ipc: Option<IpcServer>,
    /// Нужна ли перерисовка — выставляется анимациями и изменениями раскладки.
    pub redraw_needed: bool,
}

impl HypeState {
    pub fn new(
        event_loop: &mut EventLoop<'static, LoopData>,
        display: Display<Self>,
        config: Config,
    ) -> Self {
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);

        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "hypede");
        let repeat_delay = config.input.repeat_delay_ms as i32;
        let repeat_rate = config.input.repeat_rate as i32;
        seat.add_keyboard(
            smithay::input::keyboard::XkbConfig {
                layout: &config.input.keyboard_layout,
                options: Some(config.input.keyboard_options.clone()),
                ..Default::default()
            },
            repeat_delay,
            repeat_rate,
        )
        .expect("не удалось настроить клавиатуру");
        seat.add_pointer();

        let socket_name = Self::init_wayland_listener(display, event_loop);
        let workspaces = Workspaces::new(config.layout.workspaces);

        Self {
            start_time: Instant::now(),
            last_frame: Instant::now(),
            socket_name,
            display_handle: dh,
            loop_signal: event_loop.get_signal(),
            space: Space::default(),
            popups: PopupManager::default(),
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            seat,
            config,
            workspaces,
            windows: HashMap::new(),
            next_window_id: 1,
            ipc: None,
            redraw_needed: true,
        }
    }

    fn init_wayland_listener(
        display: Display<Self>,
        event_loop: &mut EventLoop<'static, LoopData>,
    ) -> OsString {
        let listening_socket =
            ListeningSocketSource::new_auto().expect("не удалось открыть сокет Wayland");
        let socket_name = listening_socket.socket_name().to_os_string();
        let handle = event_loop.handle();

        handle
            .insert_source(listening_socket, move |stream, _, data| {
                data.display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                    .expect("не удалось принять клиента");
            })
            .expect("не удалось добавить сокет в цикл событий");

        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, data| {
                    // Безопасно: сам объект display мы не роняем.
                    unsafe {
                        display.get_mut().dispatch_clients(&mut data.state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("не удалось добавить дисплей в цикл событий");

        socket_name
    }

    /// Выдаёт следующий идентификатор окна.
    pub fn allocate_window_id(&mut self) -> u64 {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }

    /// Геометрия монитора целиком.
    pub fn screen_area(&self) -> Rect {
        self.space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))
            .map(|g| {
                Rect::new(
                    g.loc.x as f64,
                    g.loc.y as f64,
                    g.size.w as f64,
                    g.size.h as f64,
                )
            })
            // До появления монитора нужно на что-то опираться: раскладка
            // считается ещё до того, как бэкенд сообщит размер.
            .unwrap_or(Rect::new(0.0, 0.0, 1280.0, 720.0))
    }

    /// Область экрана, доступная окнам.
    ///
    /// Из геометрии монитора вычитается место под панель: окна не должны под
    /// неё заезжать.
    pub fn work_area(&self) -> Rect {
        let geometry = self.screen_area();

        if !self.config.panel.enabled {
            return geometry;
        }

        let panel = self.config.panel.height as f64;
        match self.config.panel.position {
            hype_config::PanelPosition::Top => Rect::new(
                geometry.origin.x,
                geometry.origin.y + panel,
                geometry.size.w,
                (geometry.size.h - panel).max(1.0),
            ),
            hype_config::PanelPosition::Bottom => Rect::new(
                geometry.origin.x,
                geometry.origin.y,
                geometry.size.w,
                (geometry.size.h - panel).max(1.0),
            ),
        }
    }

    /// Полоса, отведённая панели.
    ///
    /// Возвращает `None`, если панель отключена в настройках.
    pub fn panel_area(&self) -> Option<Rect> {
        if !self.config.panel.enabled {
            return None;
        }

        let screen = self.screen_area();
        let height = self.config.panel.height as f64;
        Some(match self.config.panel.position {
            hype_config::PanelPosition::Top => {
                Rect::new(screen.origin.x, screen.origin.y, screen.size.w, height)
            }
            hype_config::PanelPosition::Bottom => Rect::new(
                screen.origin.x,
                screen.origin.y + screen.size.h - height,
                screen.size.w,
                height,
            ),
        })
    }

    /// Пересчитывает раскладку и отправляет окнам новые размеры.
    pub fn relayout(&mut self) {
        let area = self.work_area();
        let screen = self.screen_area();
        let panel_area = self.panel_area();
        let visible: Vec<u64> = self.workspaces.visible_windows().to_vec();

        // Окна среды в общую раскладку не попадают: у панели своя полоса, а
        // поиск приложений висит по центру поверх всего.
        let mut special: Vec<(u64, Rect)> = Vec::new();
        let mut inputs: Vec<LayoutWindow> = Vec::new();

        for id in &visible {
            let Some(managed) = self.windows.get(id) else {
                continue;
            };

            match crate::roles::role_for(&managed.app_id()) {
                crate::roles::Role::Panel => {
                    if let Some(panel) = panel_area {
                        special.push((*id, panel));
                    }
                }
                crate::roles::Role::Launcher => {
                    // Размер берётся тот, который запросил сам клиент. Брать
                    // его из текущей анимации нельзя: она уже содержит
                    // результат прошлой раскладки, и окно разрасталось бы с
                    // каждым пересчётом.
                    let requested = managed.window.geometry().size;
                    let width = if requested.w > 1 {
                        requested.w as f64
                    } else {
                        LAUNCHER_WIDTH
                    };
                    let height = if requested.h > 1 {
                        requested.h as f64
                    } else {
                        LAUNCHER_HEIGHT
                    };

                    special.push((
                        *id,
                        Rect::new(
                            screen.origin.x + (screen.size.w - width) / 2.0,
                            // Чуть выше центра: так окно поиска не перекрывает
                            // то, что пользователь ищет глазами ниже.
                            screen.origin.y + (screen.size.h - height) / 3.0,
                            width,
                            height,
                        ),
                    ));
                }
                crate::roles::Role::Normal => inputs.push(LayoutWindow {
                    id: *id,
                    floating: managed.floating,
                    fullscreen: managed.fullscreen,
                    floating_rect: managed.floating_rect,
                }),
            }
        }

        let mut tiles = layout::arrange(area, &inputs, &self.config.layout);
        tiles.extend(special.into_iter().map(|(id, rect)| crate::layout::Tile {
            id,
            rect,
            fullscreen: false,
        }));
        let animations = self.config.animations.clone();
        let motion = self.config.theme.motion.scale;

        for tile in tiles {
            let Some(managed) = self.windows.get_mut(&tile.id) else {
                continue;
            };

            managed.anim.move_to(tile.rect, &animations, motion);

            // Размер клиенту сообщается сразу конечный: пересогласовывать его
            // на каждом кадре — значит заставить приложение перерисовываться
            // шестьдесят раз в секунду впустую.
            if let Some(toplevel) = managed.window.toplevel() {
                let size = (tile.rect.size.w as i32, tile.rect.size.h as i32);
                let changed = toplevel.with_pending_state(|state| {
                    let new_size = Some(size.into());
                    let changed = state.size != new_size;
                    state.size = new_size;
                    changed
                });
                if changed {
                    toplevel.send_pending_configure();
                }
            }
        }

        self.sync_space();
        self.redraw_needed = true;
    }

    /// Расставляет окна в пространстве smithay по текущим значениям анимаций.
    pub fn sync_space(&mut self) {
        let visible: Vec<u64> = self.workspaces.visible_windows().to_vec();
        let focused = self.workspaces.focused();

        // Окна, ушедшие на другой рабочий стол, убираются со сцены.
        let mapped: Vec<Window> = self.space.elements().cloned().collect();
        for window in mapped {
            let still_visible = self
                .windows
                .values()
                .any(|m| m.window == window && (visible.contains(&m.id) || m.anim.is_gone()));
            if !still_visible {
                self.space.unmap_elem(&window);
            }
        }

        for id in &visible {
            let Some(managed) = self.windows.get(id) else {
                continue;
            };
            let rect = managed.anim.rect();
            let location = (rect.origin.x.round() as i32, rect.origin.y.round() as i32);
            let activate = focused == Some(*id);
            self.space
                .map_element(managed.window.clone(), location, activate);
            managed.window.set_activated(activate);
        }
    }

    /// Продвигает все анимации. Возвращает `true`, если нужна перерисовка.
    pub fn advance_animations(&mut self) -> bool {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .min(Duration::from_millis(100));
        self.last_frame = now;

        let mut moving = false;
        let mut finished = Vec::new();

        for managed in self.windows.values_mut() {
            if managed.anim.advance(dt) {
                moving = true;
            }
            if managed.anim.is_gone() {
                finished.push(managed.id);
            }
        }

        for id in finished {
            if let Some(managed) = self.windows.remove(&id) {
                self.space.unmap_elem(&managed.window);
            }
            self.workspaces.remove_window(id);
            self.broadcast(&Event::WindowClosed { id });
            moving = true;
        }

        if moving {
            self.sync_space();
        }

        moving || std::mem::take(&mut self.redraw_needed)
    }

    /// Поверхность под точкой — по ней определяется, куда уходит клик.
    pub fn surface_under(
        &self,
        position: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space
            .element_under(position)
            .and_then(|(window, location)| {
                window
                    .surface_under(position - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(surface, point)| (surface, (point + location).to_f64()))
            })
    }

    /// Находит окно по идентификатору поверхности.
    pub fn window_id_for_surface(&self, surface: &WlSurface) -> Option<u64> {
        self.windows
            .values()
            .find(|m| {
                m.window
                    .toplevel()
                    .is_some_and(|t| t.wl_surface() == surface)
            })
            .map(|m| m.id)
    }

    /// Сведения об окне для протокола управления.
    pub fn window_info(&self, id: u64) -> Option<WindowInfo> {
        let managed = self.windows.get(&id)?;
        let rect = managed.anim.target();
        Some(WindowInfo {
            id,
            title: managed.title(),
            app_id: managed.app_id(),
            workspace: self.workspaces.workspace_of(id).unwrap_or(0),
            x: rect.origin.x.round() as i32,
            y: rect.origin.y.round() as i32,
            width: rect.size.w.round() as u32,
            height: rect.size.h.round() as u32,
            focused: self.workspaces.focused() == Some(id),
            floating: managed.floating,
            fullscreen: managed.fullscreen,
        })
    }

    /// Полное состояние среды для протокола управления.
    pub fn ipc_state(&self) -> IpcState {
        IpcState {
            version: crate::VERSION.to_string(),
            outputs: self.output_infos(),
            workspaces: self.workspace_infos(),
            windows: self
                .windows
                .keys()
                .filter_map(|id| self.window_info(*id))
                .collect(),
            focused_window: self.workspaces.focused(),
        }
    }

    pub fn output_infos(&self) -> Vec<OutputInfo> {
        self.space
            .outputs()
            .map(|output| {
                let geometry = self.space.output_geometry(output);
                let mode = output.current_mode();
                OutputInfo {
                    name: output.name(),
                    description: output.description(),
                    width: mode.map(|m| m.size.w as u32).unwrap_or(0),
                    height: mode.map(|m| m.size.h as u32).unwrap_or(0),
                    refresh_mhz: mode.map(|m| m.refresh as u32).unwrap_or(0),
                    scale: output.current_scale().fractional_scale(),
                    x: geometry.map(|g| g.loc.x).unwrap_or(0),
                    y: geometry.map(|g| g.loc.y).unwrap_or(0),
                }
            })
            .collect()
    }

    pub fn workspace_infos(&self) -> Vec<WorkspaceInfo> {
        let output = self
            .space
            .outputs()
            .next()
            .map(|o| o.name())
            .unwrap_or_default();

        self.workspaces
            .all()
            .iter()
            .map(|workspace| WorkspaceInfo {
                index: workspace.index,
                name: workspace.index.to_string(),
                windows: workspace.windows.len(),
                active: workspace.index == self.workspaces.active_index(),
                output: output.clone(),
            })
            .collect()
    }

    /// Рассылает событие подписчикам протокола управления.
    pub fn broadcast(&mut self, event: &Event) {
        if let Some(ipc) = &self.ipc {
            ipc.broadcast(event);
        }
    }

    /// Настройки раскладки — короткий доступ для обработчиков.
    pub fn layout_config(&self) -> &LayoutConfig {
        &self.config.layout
    }
}

/// Данные, которые композитор хранит про каждого клиента.
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
