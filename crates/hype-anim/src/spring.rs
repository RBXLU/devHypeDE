//! Пружинная физика.
//!
//! Пружина отличается от кривой сглаживания тем, что у неё нет заданной
//! длительности: движение описывается уравнением затухающего осциллятора, а
//! длительность вычисляется из параметров. Главное преимущество — анимацию
//! можно перенацелить на новое значение прямо на ходу, сохранив текущую
//! скорость, и переход останется плавным. Именно так ведут себя окна, когда
//! пользователь тащит их мышью и отпускает.

/// Параметры затухающего гармонического осциллятора.
///
/// * `stiffness` — жёсткость пружины: чем больше, тем быстрее движение.
/// * `damping_ratio` — коэффициент затухания: `< 1` даёт колебания,
///   `== 1` — самое быстрое движение без перелёта, `> 1` — вязкое торможение.
/// * `mass` — масса: чем больше, тем инертнее.
/// * `epsilon` — порог, при котором движение считается завершённым.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Spring {
    pub stiffness: f64,
    pub damping_ratio: f64,
    pub mass: f64,
    pub epsilon: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Spring::SMOOTH
    }
}

impl Spring {
    /// Базовая пружина интерфейса: без перелёта, около 300 мс.
    pub const SMOOTH: Spring = Spring {
        stiffness: 400.0,
        damping_ratio: 1.0,
        mass: 1.0,
        epsilon: 0.001,
    };

    /// Быстрая и резкая — для реакции на клик.
    pub const SNAPPY: Spring = Spring {
        stiffness: 900.0,
        damping_ratio: 0.9,
        mass: 1.0,
        epsilon: 0.001,
    };

    /// С заметным перелётом — для появления окон и всплывающих панелей.
    pub const BOUNCY: Spring = Spring {
        stiffness: 500.0,
        damping_ratio: 0.55,
        mass: 1.0,
        epsilon: 0.001,
    };

    /// Мягкая и медленная — для фоновых перестроений раскладки.
    pub const GENTLE: Spring = Spring {
        stiffness: 180.0,
        damping_ratio: 1.0,
        mass: 1.2,
        epsilon: 0.001,
    };

    /// Собственная частота колебаний, рад/с.
    fn omega0(&self) -> f64 {
        (self.stiffness / self.mass.max(f64::EPSILON)).sqrt()
    }

    /// Смещение от цели в момент `t` при начальном смещении `x0` и скорости `v0`.
    fn displacement(&self, x0: f64, v0: f64, t: f64) -> f64 {
        if t <= 0.0 {
            return x0;
        }
        let w0 = self.omega0();
        let zeta = self.damping_ratio.max(0.0);

        if zeta < 1.0 {
            // Недодемпфированная: затухающие колебания вокруг цели.
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            let envelope = (-zeta * w0 * t).exp();
            envelope * (x0 * (wd * t).cos() + ((v0 + zeta * w0 * x0) / wd) * (wd * t).sin())
        } else if (zeta - 1.0).abs() < 1e-9 {
            // Критическая: самый быстрый подход к цели без единого перелёта.
            (x0 + (v0 + w0 * x0) * t) * (-w0 * t).exp()
        } else {
            // Передемпфированная: два вещественных корня, движение вязкое.
            let s = w0 * (zeta * zeta - 1.0).sqrt();
            let r1 = -zeta * w0 + s;
            let r2 = -zeta * w0 - s;
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            c1 * (r1 * t).exp() + c2 * (r2 * t).exp()
        }
    }

    /// Значение пружины в момент `t` (в секундах) при движении `from → to`.
    ///
    /// `velocity` — начальная скорость в единицах значения за секунду.
    pub fn value_at(&self, from: f64, to: f64, velocity: f64, t: f64) -> f64 {
        to + self.displacement(from - to, velocity, t)
    }

    /// Скорость в момент `t`. Считается численно — аналитические производные для
    /// трёх режимов затухания дают тот же результат, но заметно легче ошибиться.
    pub fn velocity_at(&self, from: f64, to: f64, velocity: f64, t: f64) -> f64 {
        const H: f64 = 1e-5;
        let t0 = (t - H).max(0.0);
        let t1 = t + H;
        let v0 = self.value_at(from, to, velocity, t0);
        let v1 = self.value_at(from, to, velocity, t1);
        (v1 - v0) / (t1 - t0)
    }

    /// Пришла ли пружина в покой: и смещение, и скорость меньше порога.
    pub fn is_settled(&self, from: f64, to: f64, velocity: f64, t: f64) -> bool {
        let scale = (to - from).abs().max(1.0);
        let eps = self.epsilon * scale;
        (self.value_at(from, to, velocity, t) - to).abs() < eps
            && self.velocity_at(from, to, velocity, t).abs() < eps * 10.0
    }

    /// Оценка длительности движения в секундах.
    ///
    /// Шагает по времени с сеткой 1 мс до момента покоя. Результат ограничен
    /// 20 секундами, чтобы заведомо неустойчивые параметры не вешали вызывающий
    /// код в бесконечном цикле.
    pub fn duration(&self, from: f64, to: f64, velocity: f64) -> f64 {
        const STEP: f64 = 0.001;
        const MAX: f64 = 20.0;

        if (from - to).abs() < f64::EPSILON && velocity.abs() < f64::EPSILON {
            return 0.0;
        }

        let mut t = 0.0;
        while t < MAX {
            t += STEP;
            if self.is_settled(from, to, velocity, t) {
                return t;
            }
        }
        MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_starts_at_the_source_value() {
        let spring = Spring::SMOOTH;
        assert!((spring.value_at(0.0, 100.0, 0.0, 0.0) - 0.0).abs() < 1e-9);
        assert!((spring.value_at(50.0, 100.0, 0.0, 0.0) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn spring_converges_to_the_target() {
        for spring in [Spring::SMOOTH, Spring::SNAPPY, Spring::BOUNCY, Spring::GENTLE] {
            let v = spring.value_at(0.0, 100.0, 0.0, 5.0);
            assert!((v - 100.0).abs() < 0.01, "{spring:?} не сошлась: {v}");
        }
    }

    #[test]
    fn critically_damped_spring_never_overshoots() {
        let spring = Spring::SMOOTH;
        for i in 0..2000 {
            let v = spring.value_at(0.0, 100.0, 0.0, i as f64 / 1000.0);
            assert!(v <= 100.0 + 1e-6, "перелёт до {v} на шаге {i}");
        }
    }

    #[test]
    fn bouncy_spring_does_overshoot() {
        let spring = Spring::BOUNCY;
        let peak = (0..2000)
            .map(|i| spring.value_at(0.0, 100.0, 0.0, i as f64 / 1000.0))
            .fold(f64::MIN, f64::max);
        assert!(peak > 100.5, "BOUNCY обязана перелетать, пик = {peak}");
    }

    #[test]
    fn overdamped_spring_is_monotonic_and_slow() {
        let spring = Spring {
            stiffness: 300.0,
            damping_ratio: 2.5,
            mass: 1.0,
            epsilon: 0.001,
        };
        let mut prev = 0.0;
        for i in 0..3000 {
            let v = spring.value_at(0.0, 100.0, 0.0, i as f64 / 1000.0);
            assert!(v >= prev - 1e-9, "передемпфированная пружина качнулась назад");
            prev = v;
        }
        assert!((prev - 100.0).abs() < 0.5);
    }

    #[test]
    fn initial_velocity_pushes_the_value_forward() {
        let spring = Spring::SMOOTH;
        let without = spring.value_at(0.0, 100.0, 0.0, 0.02);
        let with = spring.value_at(0.0, 100.0, 500.0, 0.02);
        assert!(with > without, "начальная скорость должна ускорять старт");
    }

    #[test]
    fn velocity_is_zero_at_rest() {
        let spring = Spring::SMOOTH;
        assert!(spring.velocity_at(0.0, 100.0, 0.0, 5.0).abs() < 0.01);
    }

    #[test]
    fn stiffer_springs_finish_sooner() {
        let soft = Spring::GENTLE.duration(0.0, 100.0, 0.0);
        let stiff = Spring::SNAPPY.duration(0.0, 100.0, 0.0);
        assert!(stiff < soft, "жёсткая {stiff} должна быть быстрее мягкой {soft}");
        assert!(stiff > 0.0 && soft < 20.0);
    }

    #[test]
    fn zero_distance_takes_no_time() {
        assert_eq!(Spring::SMOOTH.duration(42.0, 42.0, 0.0), 0.0);
    }
}
