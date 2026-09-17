//! Движок анимаций HypeDE.
//!
//! Крейт ничего не знает ни про Wayland, ни про GTK — это чистая математика,
//! общая для композитора, панели и приложений. Благодаря этому одна и та же
//! кривая описывает и появление окна, и раскрытие меню в файловом менеджере:
//! среда ощущается цельной, а не набором программ с разными характерами.
//!
//! # Пример
//!
//! ```
//! use std::time::Duration;
//! use hype_anim::{Animation, Easing, Rect};
//!
//! // Окно въезжает на своё место за 250 мс.
//! let mut anim = Animation::timed(
//!     Rect::new(0.0, 0.0, 800.0, 600.0),
//!     Rect::new(100.0, 40.0, 800.0, 600.0),
//!     Duration::from_millis(250),
//!     Easing::EaseOutCubic,
//! );
//!
//! while !anim.is_finished() {
//!     let rect = anim.advance(Duration::from_millis(16));
//!     // ... отрисовать окно в `rect`
//!     let _ = rect;
//! }
//! ```

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

mod animation;
mod easing;
mod lerp;
mod spring;

pub use animation::{Animation, Curve, Stagger};
pub use easing::{CubicBezier, Easing};
pub use lerp::{Lerp, Point, Rect, Size};
pub use spring::Spring;

/// Частота кадров по умолчанию, если композитор ещё не знает реальную.
pub const DEFAULT_REFRESH_RATE: f64 = 60.0;

/// Длительность одного кадра при заданной частоте обновления экрана.
pub fn frame_duration(refresh_rate: f64) -> std::time::Duration {
    let hz = if refresh_rate > 1.0 {
        refresh_rate
    } else {
        DEFAULT_REFRESH_RATE
    };
    std::time::Duration::from_secs_f64(1.0 / hz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_duration_matches_common_refresh_rates() {
        assert_eq!(frame_duration(60.0).as_micros(), 16_666);
        assert_eq!(frame_duration(144.0).as_micros(), 6_944);
    }

    #[test]
    fn nonsense_refresh_rate_falls_back_to_sixty_hertz() {
        assert_eq!(frame_duration(0.0), frame_duration(60.0));
        assert_eq!(frame_duration(-10.0), frame_duration(60.0));
    }
}
