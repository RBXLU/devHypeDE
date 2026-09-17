//! Интерполяция значений и простая геометрия.

/// Тип, значения которого можно смешивать.
///
/// `lerp` вызывается на каждом кадре, `distance` нужен только при
/// перенацеливании пружины — чтобы пересчитать скорость под новую дистанцию.
pub trait Lerp: Copy {
    /// Значение между `self` (при `t = 0`) и `other` (при `t = 1`).
    ///
    /// `t` может выходить за пределы [0, 1]: кривые с перелётом на это
    /// рассчитывают, поэтому реализация не должна его обрезать.
    fn lerp(self, other: Self, t: f64) -> Self;

    /// Расстояние между двумя значениями в тех же единицах, что и само значение.
    fn distance(self, other: Self) -> f64;
}

impl Lerp for f64 {
    fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }

    fn distance(self, other: Self) -> f64 {
        (other - self).abs()
    }
}

impl Lerp for f32 {
    fn lerp(self, other: Self, t: f64) -> Self {
        (self as f64).lerp(other as f64, t) as f32
    }

    fn distance(self, other: Self) -> f64 {
        (other - self).abs() as f64
    }
}

impl Lerp for i32 {
    fn lerp(self, other: Self, t: f64) -> Self {
        (self as f64).lerp(other as f64, t).round() as i32
    }

    fn distance(self, other: Self) -> f64 {
        (other as f64 - self as f64).abs()
    }
}

impl<A: Lerp, B: Lerp> Lerp for (A, B) {
    fn lerp(self, other: Self, t: f64) -> Self {
        (self.0.lerp(other.0, t), self.1.lerp(other.1, t))
    }

    fn distance(self, other: Self) -> f64 {
        let a = self.0.distance(other.0);
        let b = self.1.distance(other.1);
        (a * a + b * b).sqrt()
    }
}

impl<const N: usize, T: Lerp> Lerp for [T; N] {
    fn lerp(self, other: Self, t: f64) -> Self {
        let mut out = self;
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self[i].lerp(other[i], t);
        }
        out
    }

    fn distance(self, other: Self) -> f64 {
        let sum: f64 = (0..N).map(|i| self[i].distance(other[i]).powi(2)).sum();
        sum.sqrt()
    }
}

/// Точка или вектор смещения в логических пикселях.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Lerp for Point {
    fn lerp(self, other: Self, t: f64) -> Self {
        Point::new(self.x.lerp(other.x, t), self.y.lerp(other.y, t))
    }

    fn distance(self, other: Self) -> f64 {
        ((other.x - self.x).powi(2) + (other.y - self.y).powi(2)).sqrt()
    }
}

/// Размер в логических пикселях.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Size {
    pub const fn new(w: f64, h: f64) -> Self {
        Self { w, h }
    }
}

impl Lerp for Size {
    fn lerp(self, other: Self, t: f64) -> Self {
        Size::new(self.w.lerp(other.w, t), self.h.lerp(other.h, t))
    }

    fn distance(self, other: Self) -> f64 {
        ((other.w - self.w).powi(2) + (other.h - self.h).powi(2)).sqrt()
    }
}

/// Прямоугольник окна: положение левого верхнего угла и размер.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            origin: Point { x, y },
            size: Size { w, h },
        }
    }

    pub fn center(&self) -> Point {
        Point::new(
            self.origin.x + self.size.w / 2.0,
            self.origin.y + self.size.h / 2.0,
        )
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.origin.x
            && p.y >= self.origin.y
            && p.x < self.origin.x + self.size.w
            && p.y < self.origin.y + self.size.h
    }

    /// Прямоугольник, уменьшенный на `amount` с каждой стороны.
    pub fn inset(&self, amount: f64) -> Rect {
        Rect::new(
            self.origin.x + amount,
            self.origin.y + amount,
            (self.size.w - amount * 2.0).max(0.0),
            (self.size.h - amount * 2.0).max(0.0),
        )
    }

    /// Прямоугольник, отмасштабированный относительно собственного центра.
    ///
    /// Используется для анимации появления окна: окно «вырастает» из своего
    /// центра, а не из угла экрана.
    pub fn scaled_around_center(&self, factor: f64) -> Rect {
        let c = self.center();
        let w = self.size.w * factor;
        let h = self.size.h * factor;
        Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h)
    }
}

impl Lerp for Rect {
    fn lerp(self, other: Self, t: f64) -> Self {
        Rect {
            origin: self.origin.lerp(other.origin, t),
            size: self.size.lerp(other.size, t),
        }
    }

    fn distance(self, other: Self) -> f64 {
        (self.origin.distance(other.origin).powi(2) + self.size.distance(other.size).powi(2)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_lerp_hits_both_ends() {
        assert_eq!(0.0f64.lerp(10.0, 0.0), 0.0);
        assert_eq!(0.0f64.lerp(10.0, 1.0), 10.0);
        assert_eq!(0.0f64.lerp(10.0, 0.25), 2.5);
    }

    #[test]
    fn lerp_extrapolates_beyond_the_unit_range() {
        // Нужно кривым с перелётом: они отдают t > 1.
        assert_eq!(0.0f64.lerp(10.0, 1.2), 12.0);
    }

    #[test]
    fn integer_lerp_rounds_to_nearest() {
        assert_eq!(0i32.lerp(10, 0.46), 5);
        assert_eq!(0i32.lerp(10, 0.44), 4);
    }

    #[test]
    fn rect_lerp_moves_and_resizes_together() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(100.0, 50.0, 200.0, 300.0);
        let mid = a.lerp(b, 0.5);
        assert_eq!(mid, Rect::new(50.0, 25.0, 150.0, 200.0));
    }

    #[test]
    fn scaling_around_center_keeps_the_center_put() {
        let r = Rect::new(10.0, 20.0, 100.0, 80.0);
        let scaled = r.scaled_around_center(0.9);
        assert!((scaled.center().x - r.center().x).abs() < 1e-9);
        assert!((scaled.center().y - r.center().y).abs() < 1e-9);
        assert!(scaled.size.w < r.size.w);
    }

    #[test]
    fn rect_contains_uses_half_open_bounds() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.9, 9.9)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
    }

    #[test]
    fn distance_is_euclidean() {
        assert_eq!(Point::new(0.0, 0.0).distance(Point::new(3.0, 4.0)), 5.0);
        assert_eq!([0.0f64, 0.0].distance([3.0, 4.0]), 5.0);
    }

    #[test]
    fn inset_never_produces_negative_size() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0).inset(50.0);
        assert_eq!(r.size, Size::new(0.0, 0.0));
    }
}
