//! Кривые сглаживания (easing).
//!
//! Каждая кривая — это функция `f(t)` на отрезке `t ∈ [0, 1]`, где 0 — начало
//! анимации, 1 — конец. Возвращаемое значение — «прогресс» анимации; он может
//! выходить за пределы [0, 1] (например, у `EaseOutBack` есть перелёт).

/// Набор готовых кривых.
///
/// Названия совпадают с общепринятыми (CSS / easings.net), чтобы дизайнерские
/// референсы можно было переносить без пересчёта.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Easing {
    Linear,

    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,

    EaseInCubic,
    /// Кривая по умолчанию: быстрый старт, мягкое торможение.
    #[default]
    EaseOutCubic,
    EaseInOutCubic,

    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,

    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,

    /// Небольшой «недолёт» в начале и перелёт в конце.
    EaseOutBack,
    EaseInOutBack,

    /// Пружинистое затухающее колебание в конце.
    EaseOutElastic,

    /// Отскок, как у мячика.
    EaseOutBounce,

    /// Произвольная кубическая кривая Безье, как `cubic-bezier()` в CSS.
    CubicBezier(CubicBezier),
}

impl Easing {
    /// Применяет кривую к нормированному времени `t ∈ [0, 1]`.
    ///
    /// Значения вне отрезка обрезаются, поэтому вызывающий код может не следить
    /// за точным попаданием в границы.
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,

            Easing::EaseInQuad => t * t,
            Easing::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }

            Easing::EaseInCubic => t * t * t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }

            Easing::EaseInQuart => t * t * t * t,
            Easing::EaseOutQuart => 1.0 - (1.0 - t).powi(4),
            Easing::EaseInOutQuart => {
                if t < 0.5 {
                    8.0 * t * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
                }
            }

            Easing::EaseInExpo => {
                if t == 0.0 {
                    0.0
                } else {
                    (2.0f64).powf(10.0 * t - 10.0)
                }
            }
            Easing::EaseOutExpo => {
                if t == 1.0 {
                    1.0
                } else {
                    1.0 - (2.0f64).powf(-10.0 * t)
                }
            }
            Easing::EaseInOutExpo => {
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else if t < 0.5 {
                    (2.0f64).powf(20.0 * t - 10.0) / 2.0
                } else {
                    (2.0 - (2.0f64).powf(-20.0 * t + 10.0)) / 2.0
                }
            }

            Easing::EaseOutBack => {
                const C1: f64 = 1.70158;
                const C3: f64 = C1 + 1.0;
                1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
            }
            Easing::EaseInOutBack => {
                const C1: f64 = 1.70158;
                const C2: f64 = C1 * 1.525;
                if t < 0.5 {
                    ((2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
                } else {
                    ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
                }
            }

            Easing::EaseOutElastic => {
                const C4: f64 = 2.0 * std::f64::consts::PI / 3.0;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    (2.0f64).powf(-10.0 * t) * ((t * 10.0 - 0.75) * C4).sin() + 1.0
                }
            }

            Easing::EaseOutBounce => ease_out_bounce(t),

            Easing::CubicBezier(bezier) => bezier.apply(t),
        }
    }
}

fn ease_out_bounce(t: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;

    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984375
    }
}

/// Кубическая кривая Безье с закреплёнными концами `(0,0)` и `(1,1)`.
///
/// Задаётся двумя контрольными точками, ровно как `cubic-bezier(x1, y1, x2, y2)`
/// в CSS. Значение считается тем же способом, что и в браузерах: по координате
/// `x` методом Ньютона находится параметр кривой, затем берётся `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl CubicBezier {
    /// Кривая из фирменного набора HypeDE: быстрый старт, мягкое торможение.
    pub const HYPE_STANDARD: CubicBezier = CubicBezier {
        x1: 0.2,
        y1: 0.0,
        x2: 0.0,
        y2: 1.0,
    };

    /// Появление элемента: резкий вылет без перелёта.
    pub const HYPE_ENTER: CubicBezier = CubicBezier {
        x1: 0.05,
        y1: 0.7,
        x2: 0.1,
        y2: 1.0,
    };

    /// Исчезновение: медленный старт, быстрый уход.
    pub const HYPE_EXIT: CubicBezier = CubicBezier {
        x1: 0.3,
        y1: 0.0,
        x2: 0.8,
        y2: 0.15,
    };

    /// Создаёт кривую. Координаты `x` обрезаются до [0, 1] — как требует CSS,
    /// иначе кривая перестаёт быть функцией от времени.
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self {
            x1: x1.clamp(0.0, 1.0),
            y1,
            x2: x2.clamp(0.0, 1.0),
            y2,
        }
    }

    fn sample_x(&self, t: f64) -> f64 {
        sample_cubic(self.x1, self.x2, t)
    }

    fn sample_y(&self, t: f64) -> f64 {
        sample_cubic(self.y1, self.y2, t)
    }

    fn sample_dx(&self, t: f64) -> f64 {
        sample_cubic_derivative(self.x1, self.x2, t)
    }

    /// Находит параметр кривой, при котором её `x` равен заданному времени.
    fn solve_t_for_x(&self, x: f64) -> f64 {
        // Ньютон сходится за несколько итераций почти всегда; если производная
        // вырождается (плоский участок), добираем результат делением пополам.
        let mut t = x;
        for _ in 0..8 {
            let dx = self.sample_dx(t);
            if dx.abs() < 1e-6 {
                break;
            }
            let err = self.sample_x(t) - x;
            if err.abs() < 1e-7 {
                return t;
            }
            t -= err / dx;
        }

        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        let mut t = x.clamp(0.0, 1.0);
        for _ in 0..24 {
            let err = self.sample_x(t) - x;
            if err.abs() < 1e-7 {
                break;
            }
            if err > 0.0 {
                hi = t;
            } else {
                lo = t;
            }
            t = (lo + hi) / 2.0;
        }
        t
    }

    /// Значение кривой в момент времени `x ∈ [0, 1]`.
    pub fn apply(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        if x == 0.0 || x == 1.0 {
            return x;
        }
        self.sample_y(self.solve_t_for_x(x))
    }
}

/// Значение кубической кривой Безье с концами 0 и 1 в точке `t`.
fn sample_cubic(p1: f64, p2: f64, t: f64) -> f64 {
    let inv = 1.0 - t;
    3.0 * inv * inv * t * p1 + 3.0 * inv * t * t * p2 + t * t * t
}

/// Производная той же кривой по `t`.
fn sample_cubic_derivative(p1: f64, p2: f64, t: f64) -> f64 {
    let inv = 1.0 - t;
    3.0 * inv * inv * p1 + 6.0 * inv * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn every_curve_starts_at_zero_and_ends_at_one() {
        let curves = [
            Easing::Linear,
            Easing::EaseInQuad,
            Easing::EaseOutQuad,
            Easing::EaseInOutQuad,
            Easing::EaseInCubic,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
            Easing::EaseInQuart,
            Easing::EaseOutQuart,
            Easing::EaseInOutQuart,
            Easing::EaseInExpo,
            Easing::EaseOutExpo,
            Easing::EaseInOutExpo,
            Easing::EaseOutBack,
            Easing::EaseInOutBack,
            Easing::EaseOutElastic,
            Easing::EaseOutBounce,
            Easing::CubicBezier(CubicBezier::HYPE_STANDARD),
        ];
        for curve in curves {
            assert!(curve.apply(0.0).abs() < 1e-6, "{curve:?} не стартует с 0");
            assert!(
                (curve.apply(1.0) - 1.0).abs() < 1e-6,
                "{curve:?} не финиширует в 1"
            );
        }
    }

    #[test]
    fn time_outside_the_unit_range_is_clamped() {
        assert_close(Easing::EaseOutCubic.apply(-5.0), 0.0);
        assert_close(Easing::EaseOutCubic.apply(5.0), 1.0);
    }

    #[test]
    fn linear_is_identity() {
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            assert_close(Easing::Linear.apply(t), t);
        }
    }

    #[test]
    fn ease_out_overshoots_ease_in_in_the_middle() {
        // «Out»-кривые тратят большую часть пути в начале, «in» — в конце.
        assert!(Easing::EaseOutCubic.apply(0.5) > 0.5);
        assert!(Easing::EaseInCubic.apply(0.5) < 0.5);
    }

    #[test]
    fn back_curve_overshoots_past_one() {
        let peak = (0..100)
            .map(|i| Easing::EaseOutBack.apply(i as f64 / 100.0))
            .fold(f64::MIN, f64::max);
        assert!(
            peak > 1.0,
            "EaseOutBack должна перелетать цель, пик = {peak}"
        );
    }

    #[test]
    fn bezier_matches_linear_when_control_points_are_on_the_diagonal() {
        let linear = CubicBezier::new(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert!((linear.apply(t) - t).abs() < 1e-5);
        }
    }

    #[test]
    fn bezier_is_monotonic_for_standard_curve() {
        let curve = CubicBezier::HYPE_STANDARD;
        let mut prev = 0.0;
        for i in 0..=200 {
            let v = curve.apply(i as f64 / 200.0);
            assert!(v >= prev - 1e-9, "кривая пошла назад на t = {i}");
            prev = v;
        }
    }

    #[test]
    fn bezier_clamps_x_control_points() {
        let curve = CubicBezier::new(-3.0, 0.0, 9.0, 1.0);
        assert_close(curve.apply(0.0), 0.0);
        assert_close(curve.apply(1.0), 1.0);
    }
}
