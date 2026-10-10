//! Анимация значения во времени.
//!
//! Композитор на каждом кадре вызывает [`Animation::advance`] с временем,
//! прошедшим с прошлого кадра, и получает текущее значение. Анимация не знает
//! ни про часы, ни про вертикальную синхронизацию — это делает её полностью
//! тестируемой.

use std::time::Duration;

use crate::easing::Easing;
use crate::lerp::Lerp;
use crate::spring::Spring;

/// Закон, по которому значение идёт от начала к цели.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Curve {
    /// Фиксированная длительность плюс кривая сглаживания.
    Timed { duration: Duration, easing: Easing },
    /// Пружина: длительность вычисляется из физики.
    Spring(Spring),
}

impl Curve {
    /// Кривая заданной длительности в миллисекундах.
    pub fn timed(ms: u64, easing: Easing) -> Self {
        Curve::Timed {
            duration: Duration::from_millis(ms),
            easing,
        }
    }
}

/// Анимация значения типа `T`.
#[derive(Debug, Clone, Copy)]
pub struct Animation<T: Lerp> {
    from: T,
    to: T,
    curve: Curve,
    delay: f64,
    /// Время с момента запуска, в секундах, включая задержку.
    elapsed: f64,
    /// Начальная скорость в единицах прогресса за секунду. Ненулевой она
    /// становится после перенацеливания.
    initial_velocity: f64,
}

impl<T: Lerp> Animation<T> {
    /// Анимация с фиксированной длительностью.
    pub fn timed(from: T, to: T, duration: Duration, easing: Easing) -> Self {
        Self::new(from, to, Curve::Timed { duration, easing })
    }

    /// Пружинная анимация.
    pub fn spring(from: T, to: T, spring: Spring) -> Self {
        Self::new(from, to, Curve::Spring(spring))
    }

    /// Анимация с произвольной кривой.
    pub fn new(from: T, to: T, curve: Curve) -> Self {
        Self {
            from,
            to,
            curve,
            delay: 0.0,
            elapsed: 0.0,
            initial_velocity: 0.0,
        }
    }

    /// Значение, которое уже стоит на месте: никаких кадров не потребует.
    pub fn settled(value: T) -> Self {
        let mut anim = Self::timed(value, value, Duration::ZERO, Easing::Linear);
        anim.elapsed = 0.0;
        anim
    }

    /// Откладывает старт — удобно для «лесенки» из нескольких элементов.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay.as_secs_f64();
        self
    }

    /// Время с момента старта без учёта задержки.
    fn local_time(&self) -> f64 {
        (self.elapsed - self.delay).max(0.0)
    }

    /// Текущий прогресс: 0 — начало, 1 — цель. Может выходить за пределы
    /// отрезка, если кривая перелетает.
    pub fn progress(&self) -> f64 {
        let t = self.local_time();
        match self.curve {
            Curve::Timed { duration, easing } => {
                let d = duration.as_secs_f64();
                if d <= 0.0 {
                    1.0
                } else {
                    easing.apply(t / d)
                }
            }
            Curve::Spring(spring) => spring.value_at(0.0, 1.0, self.initial_velocity, t),
        }
    }

    /// Скорость изменения прогресса, единиц в секунду.
    fn progress_velocity(&self) -> f64 {
        let t = self.local_time();
        match self.curve {
            Curve::Timed { duration, easing } => {
                let d = duration.as_secs_f64();
                if d <= 0.0 {
                    return 0.0;
                }
                const H: f64 = 1e-4;
                let a = easing.apply(((t - H) / d).max(0.0));
                let b = easing.apply((t + H) / d);
                (b - a) / (2.0 * H)
            }
            Curve::Spring(spring) => spring.velocity_at(0.0, 1.0, self.initial_velocity, t),
        }
    }

    /// Текущее значение.
    pub fn value(&self) -> T {
        if self.is_finished() {
            return self.to;
        }
        self.from.lerp(self.to, self.progress())
    }

    /// Значение, к которому анимация стремится.
    pub fn target(&self) -> T {
        self.to
    }

    /// Закончилась ли анимация.
    pub fn is_finished(&self) -> bool {
        if self.elapsed < self.delay {
            return false;
        }
        let t = self.local_time();
        match self.curve {
            Curve::Timed { duration, .. } => t >= duration.as_secs_f64(),
            Curve::Spring(spring) => spring.is_settled(0.0, 1.0, self.initial_velocity, t),
        }
    }

    /// Продвигает анимацию на `dt` и возвращает новое значение.
    pub fn advance(&mut self, dt: Duration) -> T {
        if !self.is_finished() {
            self.elapsed += dt.as_secs_f64();
        }
        self.value()
    }

    /// Мгновенно доводит анимацию до цели — например, когда пользователь
    /// отключил анимации или окно закрылось раньше времени.
    pub fn skip_to_end(&mut self) {
        self.from = self.to;
        self.elapsed = self.delay
            + match self.curve {
                Curve::Timed { duration, .. } => duration.as_secs_f64(),
                Curve::Spring(spring) => spring.duration(0.0, 1.0, self.initial_velocity),
            };
    }

    /// Меняет цель на ходу, сохраняя текущее значение и скорость.
    ///
    /// Это то, ради чего стоит держать пружины: если пользователь перетащил
    /// окно в другой угол, пока анимация ещё шла, картинка не дёрнется — новая
    /// анимация стартует ровно с той же скоростью, с какой шла старая.
    ///
    /// Скорость пересчитывается по отношению старой и новой дистанций. Для
    /// многомерных значений (прямоугольник окна) это приближение: направление
    /// движения может измениться, а масштаб скорости сохраняется.
    pub fn retarget(&mut self, new_target: T) {
        let current = self.value();
        let velocity = self.progress_velocity();

        let old_distance = self.from.distance(self.to);
        let new_distance = current.distance(new_target);

        self.initial_velocity = if new_distance > 1e-9 && old_distance > 1e-9 {
            velocity * old_distance / new_distance
        } else {
            0.0
        };

        self.from = current;
        self.to = new_target;
        self.elapsed = 0.0;
        self.delay = 0.0;
    }
}

/// Раздаёт нарастающие задержки элементам списка.
///
/// Так оживают меню и сетка приложений: элементы появляются «волной», а не все
/// разом. `step` — задержка между соседями, `max_total` ограничивает суммарную
/// задержку, чтобы список из двухсот файлов не проявлялся минуту.
#[derive(Debug, Clone, Copy)]
pub struct Stagger {
    pub step: Duration,
    pub max_total: Duration,
}

impl Default for Stagger {
    fn default() -> Self {
        Self {
            step: Duration::from_millis(20),
            max_total: Duration::from_millis(220),
        }
    }
}

impl Stagger {
    pub fn new(step_ms: u64, max_total_ms: u64) -> Self {
        Self {
            step: Duration::from_millis(step_ms),
            max_total: Duration::from_millis(max_total_ms),
        }
    }

    /// Задержка для элемента с индексом `index`.
    pub fn delay_for(&self, index: usize) -> Duration {
        let raw = self.step.saturating_mul(index as u32);
        raw.min(self.max_total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lerp::Rect;

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn timed_animation_walks_from_start_to_finish() {
        let mut anim = Animation::timed(0.0f64, 100.0, Duration::from_millis(100), Easing::Linear);
        assert_eq!(anim.value(), 0.0);

        anim.advance(Duration::from_millis(50));
        assert!((anim.value() - 50.0).abs() < 1e-6);
        assert!(!anim.is_finished());

        anim.advance(Duration::from_millis(50));
        assert_eq!(anim.value(), 100.0);
        assert!(anim.is_finished());
    }

    #[test]
    fn finished_animation_lands_exactly_on_the_target() {
        // Кривые с перелётом не должны оставлять окно на 100.4 пикселя.
        let mut anim = Animation::timed(
            0.0f64,
            100.0,
            Duration::from_millis(100),
            Easing::EaseOutBack,
        );
        for _ in 0..20 {
            anim.advance(FRAME);
        }
        assert_eq!(anim.value(), 100.0);
    }

    #[test]
    fn advancing_a_finished_animation_is_a_no_op() {
        let mut anim = Animation::timed(0.0f64, 1.0, Duration::from_millis(10), Easing::Linear);
        anim.advance(Duration::from_secs(1));
        let elapsed_after_finish = anim.elapsed;
        anim.advance(Duration::from_secs(10));
        assert_eq!(anim.elapsed, elapsed_after_finish);
    }

    #[test]
    fn delay_holds_the_value_at_the_start() {
        let mut anim = Animation::timed(0.0f64, 100.0, Duration::from_millis(100), Easing::Linear)
            .with_delay(Duration::from_millis(50));

        anim.advance(Duration::from_millis(40));
        assert_eq!(anim.value(), 0.0);
        assert!(!anim.is_finished());

        anim.advance(Duration::from_millis(60));
        assert!(anim.value() > 0.0 && anim.value() < 100.0);
    }

    #[test]
    fn zero_duration_finishes_immediately() {
        let anim = Animation::timed(0.0f64, 100.0, Duration::ZERO, Easing::Linear);
        assert!(anim.is_finished());
        assert_eq!(anim.value(), 100.0);
    }

    #[test]
    fn settled_animation_needs_no_frames() {
        let anim = Animation::settled(42.0f64);
        assert!(anim.is_finished());
        assert_eq!(anim.value(), 42.0);
    }

    #[test]
    fn spring_animation_reaches_the_target() {
        let mut anim = Animation::spring(0.0f64, 500.0, Spring::SMOOTH);
        let mut frames = 0;
        while !anim.is_finished() && frames < 600 {
            anim.advance(FRAME);
            frames += 1;
        }
        assert!(anim.is_finished(), "пружина не сошлась за {frames} кадров");
        assert_eq!(anim.value(), 500.0);
    }

    #[test]
    fn retarget_keeps_the_current_value_continuous() {
        let mut anim = Animation::spring(0.0f64, 500.0, Spring::SMOOTH);
        for _ in 0..6 {
            anim.advance(FRAME);
        }
        let before = anim.value();

        anim.retarget(-200.0);
        let after = anim.value();

        assert!(
            (before - after).abs() < 1e-9,
            "картинка дёрнулась: {before} -> {after}"
        );
        assert_eq!(anim.target(), -200.0);
    }

    #[test]
    fn retarget_preserves_the_direction_of_motion() {
        let mut anim = Animation::spring(0.0f64, 500.0, Spring::SMOOTH);
        for _ in 0..6 {
            anim.advance(FRAME);
        }
        let speed_before = anim.progress_velocity() * anim.from.distance(anim.to);

        anim.retarget(600.0);
        let speed_after = anim.progress_velocity() * anim.from.distance(anim.to);

        // Скорость в единицах значения должна совпасть с точностью до
        // численной производной.
        assert!(
            (speed_before - speed_after).abs() < speed_before.abs() * 0.05 + 1.0,
            "скорость скакнула: {speed_before} -> {speed_after}"
        );
    }

    #[test]
    fn retarget_to_the_current_value_stops_cleanly() {
        let mut anim = Animation::spring(0.0f64, 500.0, Spring::SMOOTH);
        anim.advance(FRAME);
        let current = anim.value();
        anim.retarget(current);

        let mut frames = 0;
        while !anim.is_finished() && frames < 600 {
            anim.advance(FRAME);
            frames += 1;
        }
        assert!(anim.is_finished());
        assert_eq!(anim.value(), current);
    }

    #[test]
    fn skip_to_end_jumps_to_the_target() {
        let mut anim = Animation::spring(0.0f64, 500.0, Spring::GENTLE);
        anim.advance(FRAME);
        anim.skip_to_end();
        assert!(anim.is_finished());
        assert_eq!(anim.value(), 500.0);
    }

    #[test]
    fn rects_animate_as_a_whole() {
        let from = Rect::new(0.0, 0.0, 100.0, 100.0);
        let to = Rect::new(200.0, 100.0, 400.0, 300.0);
        let mut anim = Animation::timed(from, to, Duration::from_millis(100), Easing::Linear);

        anim.advance(Duration::from_millis(50));
        assert_eq!(anim.value(), Rect::new(100.0, 50.0, 250.0, 200.0));

        anim.advance(Duration::from_millis(50));
        assert_eq!(anim.value(), to);
    }

    #[test]
    fn stagger_grows_then_saturates() {
        let stagger = Stagger::new(20, 100);
        assert_eq!(stagger.delay_for(0), Duration::ZERO);
        assert_eq!(stagger.delay_for(3), Duration::from_millis(60));
        assert_eq!(stagger.delay_for(50), Duration::from_millis(100));
    }
}
