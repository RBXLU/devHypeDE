//! Бэкенд DRM/KMS — работа прямо на видеокарте.
//!
//! В этом режиме HypeDE и есть сеанс: композитор сам открывает видеокарту,
//! сам читает устройства ввода и сам переключает виртуальные консоли. Именно
//! так среда запускается из экрана входа.
//!
//! Устройство модуля:
//!
//! * `libseat` выдаёт права на видеокарту и устройства ввода без root;
//! * `udev` сообщает, какие видеокарты есть и когда их подключают;
//! * `libinput` приносит события клавиатуры, мыши и тачпада;
//! * `DrmOutputManager` из smithay держит кадровые буферы и выводит кадр.
//!
//! Реализация намеренно рассчитана на одну видеокарту. Раздельный рендеринг
//! на нескольких GPU (ноутбуки с дискретной картой) — отдельная задача, и
//! тащить её сложность в первую рабочую версию незачем.

use std::collections::HashMap;
use std::path::Path;

use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType};
use smithay::backend::egl::{EGLDevice, EGLDisplay};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::multigpu::gbm::GbmGlesBackend;
use smithay::backend::renderer::multigpu::{GpuManager, MultiRenderer};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{primary_gpu, UdevBackend, UdevEvent};
use smithay::desktop::utils::OutputPresentationFeedback;
use smithay::output::{Mode as WlMode, Output, PhysicalProperties};
use smithay::reexports::calloop::{EventLoop, RegistrationToken};
use smithay::reexports::drm::control::{connector, crtc, ModeTypeFlags};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::backend::GlobalId;
use smithay::utils::DeviceFd;
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use tracing::{error, info, warn};

use crate::state::{HypeState, LoopData};

/// Форматы кадрового буфера в порядке предпочтения.
///
/// Десятибитные идут первыми: на мониторе с широким охватом разница в
/// градиентах заметна глазом. Если карта их не поддерживает, smithay возьмёт
/// восьмибитный вариант из этого же списка.
const COLOR_FORMATS: &[Fourcc] = &[
    Fourcc::Abgr2101010,
    Fourcc::Argb2101010,
    Fourcc::Abgr8888,
    Fourcc::Argb8888,
];

type GbmBackend = GbmGlesBackend<GlesRenderer, DrmDeviceFd>;

/// Отрисовщик поверх видеокарты.
pub type DrmRenderer<'a> = MultiRenderer<'a, 'a, GbmBackend, GbmBackend>;

/// Данные, которые smithay хранит вместе с кадром. Здесь — сведения о показе
/// кадра для протокола `presentation-time`.
type FrameUserData = Option<OutputPresentationFeedback>;

type HypeDrmOutput = DrmOutput<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameUserData,
    DrmDeviceFd,
>;

type HypeDrmOutputManager = DrmOutputManager<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    FrameUserData,
    DrmDeviceFd,
>;

/// Один подключённый монитор.
struct SurfaceData {
    output: Output,
    global: Option<GlobalId>,
    drm_output: HypeDrmOutput,
    /// Пока ждём вертикальной синхронизации, что-то изменилось и понадобится
    /// ещё один кадр.
    needs_redraw: bool,
    /// Кадр отправлен видеокарте и ждёт показа.
    frame_pending: bool,
}

/// Одна видеокарта.
struct DeviceData {
    token: RegistrationToken,
    manager: HypeDrmOutputManager,
    scanner: DrmScanner,
    render_node: Option<DrmNode>,
    surfaces: HashMap<crtc::Handle, SurfaceData>,
}

/// Состояние бэкенда.
pub struct DrmState {
    pub session: LibSeatSession,
    pub seat_name: String,
    primary_gpu: DrmNode,
    gpus: GpuManager<GbmBackend>,
    devices: HashMap<DrmNode, DeviceData>,
    /// Ввод приостановлен, пока пользователь на другой виртуальной консоли.
    active: bool,
}

impl std::fmt::Debug for DrmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrmState")
            .field("seat_name", &self.seat_name)
            .field("primary_gpu", &self.primary_gpu)
            .field("devices", &self.devices.len())
            .field("active", &self.active)
            .finish()
    }
}

/// Поднимает бэкенд DRM/KMS и вешает его на цикл событий.
pub fn init(
    event_loop: &mut EventLoop<'static, LoopData>,
    data: &mut LoopData,
) -> anyhow::Result<()> {
    let (session, session_notifier) = LibSeatSession::new().map_err(|err| {
        anyhow::anyhow!(
            "не удалось получить доступ к сеансу через libseat: {err}. \
             Запуск на видеокарте возможен только из консоли или из экрана входа; \
             внутри другого сеанса используйте вложенный режим"
        )
    })?;

    let seat_name = session.seat();
    info!("сеанс libseat: {seat_name}");

    let primary_gpu = primary_gpu(&seat_name)
        .ok()
        .flatten()
        .and_then(|path| DrmNode::from_path(path).ok())
        .ok_or_else(|| anyhow::anyhow!("не найдена основная видеокарта"))?;
    info!("основная видеокарта: {primary_gpu}");

    let gpus = GpuManager::new(GbmGlesBackend::default())
        .map_err(|err| anyhow::anyhow!("не удалось подготовить отрисовку: {err}"))?;

    data.state.drm = Some(DrmState {
        session: session.clone(),
        seat_name: seat_name.clone(),
        primary_gpu,
        gpus,
        devices: HashMap::new(),
        active: true,
    });

    init_input(event_loop, session.clone(), &seat_name)?;
    init_session_events(event_loop, session_notifier)?;
    init_udev(event_loop, data, &seat_name)?;

    Ok(())
}

/// Подключает клавиатуру, мышь и тачпад через libinput.
fn init_input(
    event_loop: &mut EventLoop<'static, LoopData>,
    session: LibSeatSession,
    seat_name: &str,
) -> anyhow::Result<()> {
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.into());
    libinput
        .udev_assign_seat(seat_name)
        .map_err(|_| anyhow::anyhow!("не удалось привязать устройства ввода к сеансу"))?;

    event_loop
        .handle()
        .insert_source(LibinputInputBackend::new(libinput), |event, _, data| {
            data.state.process_input_event(event);
        })
        .map_err(|err| anyhow::anyhow!("не удалось подключить ввод: {err}"))?;

    Ok(())
}

/// Следит за переключением виртуальных консолей.
///
/// Когда пользователь уходит на другую консоль, видеокарту нужно отпустить, а
/// при возвращении — забрать обратно и перерисовать всё заново.
fn init_session_events(
    event_loop: &mut EventLoop<'static, LoopData>,
    notifier: smithay::backend::session::libseat::LibSeatSessionNotifier,
) -> anyhow::Result<()> {
    event_loop
        .handle()
        .insert_source(notifier, move |event, _, data| match event {
            SessionEvent::PauseSession => {
                info!("сеанс приостановлен: уходим с консоли");
                if let Some(drm) = data.state.drm.as_mut() {
                    drm.active = false;
                    for device in drm.devices.values_mut() {
                        device.manager.pause();
                    }
                }
            }
            SessionEvent::ActivateSession => {
                info!("сеанс возобновлён");
                data.state.with_drm(|state, drm| {
                    drm.active = true;
                    let nodes: Vec<DrmNode> = drm.devices.keys().copied().collect();

                    for node in nodes {
                        if let Some(device) = drm.devices.get_mut(&node) {
                            // Часть кадровых буферов могла устареть, пока
                            // видеокартой владел другой сеанс.
                            if let Err(err) = device.manager.activate(true) {
                                warn!("не удалось вернуть видеокарту {node}: {err}");
                                continue;
                            }
                            for surface in device.surfaces.values_mut() {
                                surface.needs_redraw = true;
                                surface.frame_pending = false;
                            }
                        }
                        render_device(state, drm, node);
                    }
                });
            }
        })
        .map_err(|err| anyhow::anyhow!("не удалось подключить события сеанса: {err}"))?;

    Ok(())
}

/// Находит видеокарты и следит за их появлением и исчезновением.
fn init_udev(
    event_loop: &mut EventLoop<'static, LoopData>,
    data: &mut LoopData,
    seat_name: &str,
) -> anyhow::Result<()> {
    let udev = UdevBackend::new(seat_name)
        .map_err(|err| anyhow::anyhow!("не удалось опросить устройства: {err}"))?;

    // Уже подключённые карты.
    for (device_id, path) in udev.device_list() {
        let Ok(node) = DrmNode::from_dev_id(device_id) else {
            continue;
        };
        if let Err(err) = device_added(&mut data.state, event_loop, node, path) {
            warn!("видеокарта {node} не подключена: {err}");
        }
    }

    let handle = event_loop.handle();
    event_loop
        .handle()
        .insert_source(udev, move |event, _, data| match event {
            UdevEvent::Added { device_id, path } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    // Карту подключили на ходу — например, док-станцию.
                    if let Err(err) = device_added_runtime(&mut data.state, &handle, node, &path) {
                        warn!("видеокарта {node} не подключена: {err}");
                    }
                }
            }
            UdevEvent::Changed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    data.state
                        .with_drm(|state, drm| scan_connectors(state, drm, node));
                }
            }
            UdevEvent::Removed { device_id } => {
                if let Ok(node) = DrmNode::from_dev_id(device_id) {
                    device_removed(&mut data.state, node);
                }
            }
        })
        .map_err(|err| anyhow::anyhow!("не удалось подключить слежение за устройствами: {err}"))?;

    Ok(())
}

/// Подключает видеокарту при запуске.
fn device_added(
    state: &mut HypeState,
    event_loop: &mut EventLoop<'static, LoopData>,
    node: DrmNode,
    path: &Path,
) -> anyhow::Result<()> {
    let handle = event_loop.handle();
    device_added_runtime(state, &handle, node, path)
}

/// Подключает видеокарту.
fn device_added_runtime(
    state: &mut HypeState,
    handle: &smithay::reexports::calloop::LoopHandle<'static, LoopData>,
    node: DrmNode,
    path: &Path,
) -> anyhow::Result<()> {
    let Some(drm) = state.drm.as_mut() else {
        anyhow::bail!("бэкенд DRM не запущен");
    };

    // Устройство открывает libseat: у композитора нет прав root, и это
    // правильно — доступ выдаётся только на время сеанса.
    let fd = drm
        .session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|err| anyhow::anyhow!("libseat не открыл {}: {err}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));

    let (device, notifier) = DrmDevice::new(fd.clone(), true)
        .map_err(|err| anyhow::anyhow!("не удалось открыть видеокарту: {err}"))?;
    let gbm = GbmDevice::new(fd)
        .map_err(|err| anyhow::anyhow!("не удалось подготовить буферы: {err}"))?;

    // Видеокарта сообщает о завершении показа кадра — по этому событию
    // рисуется следующий.
    let token = handle
        .insert_source(notifier, move |event, _, data| match event {
            DrmEvent::VBlank(crtc) => {
                data.state
                    .with_drm(|state, drm| frame_finished(state, drm, node, crtc));
            }
            DrmEvent::Error(err) => error!("ошибка видеокарты: {err}"),
        })
        .map_err(|err| anyhow::anyhow!("не удалось подключить видеокарту к циклу: {err}"))?;

    // Узел отрисовки может отличаться от узла вывода: на ноутбуках это разные
    // устройства.
    let render_node = unsafe { EGLDisplay::new(gbm.clone()) }
        .ok()
        .and_then(|display| EGLDevice::device_for_display(&display).ok())
        .filter(|egl| !egl.is_software())
        .and_then(|egl| egl.try_get_render_node().ok().flatten())
        .or_else(|| node.node_with_type(NodeType::Render).and_then(Result::ok));

    let render_node = render_node.unwrap_or(node);
    drm.gpus
        .as_mut()
        .add_node(render_node, gbm.clone())
        .map_err(|err| anyhow::anyhow!("видеокарта не пригодна для отрисовки: {err}"))?;

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), Some(render_node));

    let render_formats = drm
        .gpus
        .single_renderer(&render_node)
        .map_err(|err| anyhow::anyhow!("не удалось создать отрисовщик: {err}"))?
        .as_mut()
        .egl_context()
        .dmabuf_render_formats()
        .clone();

    let manager = DrmOutputManager::new(
        device,
        allocator,
        exporter,
        Some(gbm),
        COLOR_FORMATS.iter().copied(),
        render_formats,
    );

    drm.devices.insert(
        node,
        DeviceData {
            token,
            manager,
            scanner: DrmScanner::new(),
            render_node: Some(render_node),
            surfaces: HashMap::new(),
        },
    );

    info!("подключена видеокарта {node} (отрисовка на {render_node})");
    state.with_drm(|state, drm| scan_connectors(state, drm, node));
    Ok(())
}

/// Опрашивает разъёмы видеокарты и заводит мониторы.
fn scan_connectors(state: &mut HypeState, drm: &mut DrmState, node: DrmNode) {
    let Some(device) = drm.devices.get_mut(&node) else {
        return;
    };

    let scan = match device.scanner.scan_connectors(device.manager.device()) {
        Ok(scan) => scan,
        Err(err) => {
            warn!("не удалось опросить разъёмы {node}: {err}");
            return;
        }
    };

    for event in scan {
        match event {
            DrmScanEvent::Connected {
                connector,
                crtc: Some(crtc),
            } => connector_connected(state, drm, node, connector, crtc),
            DrmScanEvent::Disconnected {
                connector,
                crtc: Some(crtc),
            } => connector_disconnected(state, drm, node, connector, crtc),
            _ => {}
        }
    }
}

/// Заводит монитор на подключённом разъёме.
fn connector_connected(
    state: &mut HypeState,
    drm: &mut DrmState,
    node: DrmNode,
    connector: connector::Info,
    crtc: crtc::Handle,
) {
    let primary_gpu = drm.primary_gpu;
    let Some(device) = drm.devices.get_mut(&node) else {
        return;
    };

    let name = format!(
        "{}-{}",
        connector.interface().as_str(),
        connector.interface_id()
    );
    info!("подключён монитор {name}");

    // Предпочтительный режим — тот, который монитор объявил родным.
    let Some(drm_mode) = connector
        .modes()
        .iter()
        .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| connector.modes().first())
        .copied()
    else {
        warn!("монитор {name} не сообщил ни одного режима");
        return;
    };
    let wl_mode = WlMode::from(drm_mode);

    let (width_mm, height_mm) = connector.size().unwrap_or((0, 0));
    let output = Output::new(
        name.clone(),
        PhysicalProperties {
            size: (width_mm as i32, height_mm as i32).into(),
            subpixel: connector.subpixel().into(),
            make: "HypeDE".into(),
            model: connector.interface().as_str().to_string(),
        },
    );
    let global = output.create_global::<HypeState>(&state.display_handle);

    // Мониторы выстраиваются слева направо в порядке подключения.
    let x = state
        .space
        .outputs()
        .filter_map(|existing| state.space.output_geometry(existing))
        .map(|geometry| geometry.size.w)
        .sum::<i32>();
    let position = (x, 0);

    output.set_preferred(wl_mode);
    output.change_current_state(Some(wl_mode), None, None, Some(position.into()));
    state.space.map_output(&output, position);

    let render_node = device.render_node.unwrap_or(primary_gpu);
    let mut renderer = match drm.gpus.single_renderer(&render_node) {
        Ok(renderer) => renderer,
        Err(err) => {
            warn!("нет отрисовщика для {name}: {err}");
            return;
        }
    };

    let planes = device.manager.device().planes(&crtc).ok();

    let drm_output = match device
        .manager
        .initialize_output::<_, crate::render::Frame<DrmRenderer<'_>>>(
            crtc,
            drm_mode,
            &[connector.handle()],
            &output,
            planes,
            &mut renderer,
            &DrmOutputRenderElements::default(),
        ) {
        Ok(drm_output) => drm_output,
        Err(err) => {
            warn!("не удалось настроить вывод на {name}: {err}");
            return;
        }
    };

    device.surfaces.insert(
        crtc,
        SurfaceData {
            output,
            global: Some(global),
            drm_output,
            needs_redraw: true,
            frame_pending: false,
        },
    );

    state.relayout();
    render_device(state, drm, node);
}

/// Убирает монитор, который отключили.
fn connector_disconnected(
    state: &mut HypeState,
    drm: &mut DrmState,
    node: DrmNode,
    connector: connector::Info,
    crtc: crtc::Handle,
) {
    let Some(device) = drm.devices.get_mut(&node) else {
        return;
    };
    let Some(surface) = device.surfaces.remove(&crtc) else {
        return;
    };

    info!(
        "отключён монитор {}-{}",
        connector.interface().as_str(),
        connector.interface_id()
    );

    state.space.unmap_output(&surface.output);
    if let Some(global) = surface.global {
        state.display_handle.remove_global::<HypeState>(global);
    }

    // Окна с исчезнувшего монитора должны переехать на оставшиеся.
    state.relayout();
}

/// Убирает видеокарту, которую отключили.
fn device_removed(state: &mut HypeState, node: DrmNode) {
    state.with_drm(|state, drm| {
        let Some(mut device) = drm.devices.remove(&node) else {
            return;
        };

        for (_, surface) in device.surfaces.drain() {
            state.space.unmap_output(&surface.output);
            if let Some(global) = surface.global {
                state.display_handle.remove_global::<HypeState>(global);
            }
        }

        if let Some(render_node) = device.render_node {
            drm.gpus.as_mut().remove_node(&render_node);
        }

        state.loop_handle.remove(device.token);
        info!("отключена видеокарта {node}");
        state.relayout();
    });
}

/// Видеокарта показала кадр — можно готовить следующий.
fn frame_finished(state: &mut HypeState, drm: &mut DrmState, node: DrmNode, crtc: crtc::Handle) {
    if let Some(device) = drm.devices.get_mut(&node) {
        if let Some(surface) = device.surfaces.get_mut(&crtc) {
            surface.frame_pending = false;
            if let Err(err) = surface.drm_output.frame_submitted() {
                warn!("сбой при завершении кадра: {err}");
            }
        }
    }

    render_output(state, drm, node, crtc);
}

/// Рисует все мониторы одной видеокарты.
fn render_device(state: &mut HypeState, drm: &mut DrmState, node: DrmNode) {
    let crtcs: Vec<crtc::Handle> = drm
        .devices
        .get(&node)
        .map(|device| device.surfaces.keys().copied().collect())
        .unwrap_or_default();

    for crtc in crtcs {
        render_output(state, drm, node, crtc);
    }
}

/// Рисует один монитор.
fn render_output(state: &mut HypeState, drm: &mut DrmState, node: DrmNode, crtc: crtc::Handle) {
    if !drm.active {
        return;
    }

    let primary_gpu = drm.primary_gpu;
    let Some(device) = drm.devices.get_mut(&node) else {
        return;
    };
    let render_node = device.render_node.unwrap_or(primary_gpu);
    let Some(surface) = device.surfaces.get_mut(&crtc) else {
        return;
    };

    // Кадр уже у видеокарты — дождёмся показа, иначе получим разрыв картинки.
    if surface.frame_pending || !surface.needs_redraw {
        return;
    }
    surface.needs_redraw = false;

    let mut renderer = match drm.gpus.single_renderer(&render_node) {
        Ok(renderer) => renderer,
        Err(err) => {
            warn!("нет отрисовщика: {err}");
            return;
        }
    };

    let mode_size = surface
        .output
        .current_mode()
        .map(|mode| (mode.size.w.max(0) as u32, mode.size.h.max(0) as u32))
        .unwrap_or((0, 0));
    state.rebuild_wallpaper(mode_size);

    let elements = crate::render::output_elements(
        &mut renderer,
        &state.space,
        &surface.output,
        state.wallpaper.as_ref(),
    );

    let clear = crate::winit::clear_color(&state.config.theme.palette().bg);

    match surface
        .drm_output
        .render_frame(&mut renderer, &elements, clear, FrameFlags::DEFAULT)
    {
        Ok(result) => {
            if result.is_empty {
                // Ничего не изменилось — кадр отправлять незачем.
                return;
            }
            if let Err(err) = surface.drm_output.queue_frame(None) {
                warn!("не удалось показать кадр: {err}");
                surface.needs_redraw = true;
                return;
            }
            surface.frame_pending = true;
        }
        Err(err) => {
            warn!("ошибка отрисовки: {err}");
            surface.needs_redraw = true;
            return;
        }
    }

    // Клиентам сообщается, что кадр показан, — без этого они не станут рисовать
    // следующий.
    let time = state.start_time.elapsed();
    let output = surface.output.clone();
    state.space.elements().for_each(|window| {
        window.send_frame(&output, time, Some(std::time::Duration::ZERO), |_, _| {
            Some(output.clone())
        })
    });
    state.space.refresh();
    state.popups.cleanup();
    let _ = state.display_handle.flush_clients();
}

impl HypeState {
    /// Даёт доступ к бэкенду DRM вместе с остальным состоянием.
    ///
    /// Состояние бэкенда лежит внутри `HypeState`, а отрисовке нужны оба сразу.
    /// Вместо того чтобы разбивать структуру на части ради проверки
    /// заимствований, бэкенд на время работы вынимается и возвращается обратно.
    /// Приём простой и честный: пока идёт отрисовка, `state.drm` пуст, и
    /// вложенный вызов туда не полезет.
    pub fn with_drm<R>(&mut self, f: impl FnOnce(&mut Self, &mut DrmState) -> R) -> Option<R> {
        let mut drm = self.drm.take()?;
        let result = f(self, &mut drm);
        self.drm = Some(drm);
        Some(result)
    }

    /// Такт среды в режиме DRM.
    ///
    /// Вызывается циклом событий. Двигает анимации и, если картинка изменилась,
    /// просит новый кадр. Во вложенном режиме этим занимается winit, поэтому
    /// метод молча выходит.
    pub fn tick_drm(&mut self) {
        if self.drm.is_none() {
            return;
        }

        let animating = self.advance_animations();
        if !animating && !std::mem::take(&mut self.redraw_needed) {
            return;
        }

        self.queue_drm_redraw();
    }

    /// Просит перерисовать все мониторы.
    ///
    /// В режиме DRM кадры идут по завершении предыдущего, поэтому после
    /// изменения раскладки нужно толкнуть цикл, если он остановился.
    pub fn queue_drm_redraw(&mut self) {
        self.with_drm(|state, drm| {
            let nodes: Vec<smithay::backend::drm::DrmNode> = drm.devices.keys().copied().collect();
            for node in nodes {
                if let Some(device) = drm.devices.get_mut(&node) {
                    for surface in device.surfaces.values_mut() {
                        surface.needs_redraw = true;
                    }
                }
                render_device(state, drm, node);
            }
        });
    }
}
