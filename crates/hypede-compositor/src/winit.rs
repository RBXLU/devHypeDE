//! Вложенный бэкенд: композитор работает в окне другого сеанса.
//!
//! Это режим разработки — в нём HypeDE запускается прямо поверх GNOME, KDE или
//! другого HypeDE, без выхода из сеанса и без риска остаться перед чёрным
//! экраном. Режим с прямым доступом к видеокарте (DRM/KMS) устроен иначе, но
//! состояние и логика у них общие.

use std::time::Duration;

use hype_theme::Color;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use smithay::utils::{Rectangle, Transform};

use crate::state::{HypeState, LoopData};

/// Частота обновления окна по умолчанию, в милигерцах.
const REFRESH_MHZ: i32 = 60_000;

/// Поднимает вложенный бэкенд и вешает его на цикл событий.
pub fn init(
    event_loop: &mut EventLoop<'static, LoopData>,
    data: &mut LoopData,
) -> anyhow::Result<()> {
    let (mut backend, winit_source) = winit::init::<GlesRenderer>()
        .map_err(|err| anyhow::anyhow!("не удалось открыть окно композитора: {err}"))?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: REFRESH_MHZ,
    };

    let output = Output::new(
        "HypeDE-1".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "HypeDE".into(),
            model: "Вложенный вывод".into(),
        },
    );
    output.create_global::<HypeState>(&data.display_handle);
    // Winit отдаёт кадр перевёрнутым — трансформация возвращает картинку.
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    data.state.space.map_output(&output, (0, 0));
    data.state.relayout();

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    std::env::set_var("WAYLAND_DISPLAY", &data.state.socket_name);

    event_loop
        .handle()
        .insert_source(winit_source, move |event, _, data| {
            let state = &mut data.state;

            match event {
                WinitEvent::Resized { size, .. } => {
                    output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: REFRESH_MHZ,
                        }),
                        None,
                        None,
                        None,
                    );
                    // Окно композитора изменилось — раскладка и обои обязаны
                    // перестроиться под новый размер.
                    state.invalidate_wallpaper();
                    state.relayout();
                }
                WinitEvent::Input(event) => state.process_input_event(event),
                WinitEvent::Redraw => {
                    state.advance_animations();

                    let size = backend.window_size();
                    state.rebuild_wallpaper((size.w.max(0) as u32, size.h.max(0) as u32));

                    let damage = Rectangle::from_size(size);
                    let clear = clear_color(&state.config.theme.palette().bg);

                    {
                        let (renderer, mut framebuffer) = match backend.bind() {
                            Ok(pair) => pair,
                            Err(err) => {
                                tracing::error!("не удалось получить кадровый буфер: {err}");
                                return;
                            }
                        };

                        let elements = crate::render::output_elements(
                            renderer,
                            &state.space,
                            &output,
                            state.wallpaper.as_ref(),
                        );

                        if let Err(err) = damage_tracker.render_output(
                            renderer,
                            &mut framebuffer,
                            0,
                            &elements,
                            clear,
                        ) {
                            tracing::error!("ошибка отрисовки: {err}");
                        }
                    }

                    if let Err(err) = backend.submit(Some(&[damage])) {
                        tracing::error!("не удалось показать кадр: {err}");
                    }

                    // Клиентам сообщается, что кадр показан, — без этого они
                    // не станут рисовать следующий.
                    let elapsed = state.start_time.elapsed();
                    state.space.elements().for_each(|window| {
                        window.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
                            Some(output.clone())
                        })
                    });

                    state.space.refresh();
                    state.popups.cleanup();
                    let _ = data.display_handle.flush_clients();

                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => data.state.loop_signal.stop(),
                _ => {}
            }
        })
        .map_err(|err| anyhow::anyhow!("не удалось добавить бэкенд в цикл событий: {err}"))?;

    Ok(())
}

/// Переводит цвет темы в формат, который ждёт отрисовка.
pub fn clear_color(color: &Color) -> [f32; 4] {
    [color.r as f32, color.g as f32, color.b as f32, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_comes_from_the_theme() {
        let color = Color::from_hex("#1a1a2e").unwrap();
        let cleared = clear_color(&color);
        assert!((cleared[0] - 0.1019).abs() < 0.001);
        assert_eq!(cleared[3], 1.0, "фон обязан быть непрозрачным");
    }
}
