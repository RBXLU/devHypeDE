//! Анимации окон.
//!
//! Каждое окно хранит анимацию своего прямоугольника и прозрачности. Когда
//! раскладка меняется, окно не прыгает в новое место — оно туда едет, причём
//! едет из текущего положения с текущей скоростью, даже если предыдущая
//! анимация ещё не закончилась.

use std::time::Duration;

use hype_anim::{Animation, Curve, Rect};
use hype_config::AnimationConfig;

/// Что сейчас происходит с окном.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Окно появляется.
    Opening,
    /// Обычное состояние.
    Open,
    /// Окно закрывается; после окончания анимации его можно убирать.
    Closing,
}

/// Анимационное состояние одного окна.
#[derive(Debug, Clone)]
pub struct WindowAnimation {
    phase: Phase,
    rect: Animation<Rect>,
    alpha: Animation<f64>,
}

/// Во сколько раз окно меньше своего размера в момент появления.
const OPEN_SCALE: f64 = 0.88;

impl WindowAnimation {
    /// Новое окно: выезжает из уменьшенной копии самого себя и проявляется.
    pub fn opening(target: Rect, config: &AnimationConfig, motion_scale: f64) -> Self {
        let curve = config.window_open.curve_scaled(motion_scale);
        let from = target.scaled_around_center(OPEN_SCALE);

        Self {
            phase: Phase::Opening,
            rect: Animation::new(from, target, curve),
            alpha: Animation::new(0.0, 1.0, fade_curve(curve)),
        }
    }

    /// Окно, которое уже на месте и никуда не едет.
    pub fn settled(rect: Rect) -> Self {
        Self {
            phase: Phase::Open,
            rect: Animation::settled(rect),
            alpha: Animation::settled(1.0),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn rect(&self) -> Rect {
        self.rect.value()
    }

    pub fn alpha(&self) -> f64 {
        self.alpha.value().clamp(0.0, 1.0)
    }

    /// Куда окно едет.
    pub fn target(&self) -> Rect {
        self.rect.target()
    }

    /// Всё ли доехало.
    pub fn is_idle(&self) -> bool {
        self.rect.is_finished() && self.alpha.is_finished()
    }

    /// Можно ли убирать окно окончательно.
    pub fn is_gone(&self) -> bool {
        self.phase == Phase::Closing && self.is_idle()
    }

    /// Двигает окно к новому месту.
    ///
    /// Если окно уже едет, оно не начинает путь заново: анимация
    /// перенацеливается, сохраняя текущую скорость. Без этого перетаскивание
    /// окна в плитке выглядит как череда рывков.
    pub fn move_to(&mut self, target: Rect, config: &AnimationConfig, motion_scale: f64) {
        if self.phase == Phase::Closing {
            return;
        }
        if self.rect.target() == target {
            return;
        }

        if self.rect.is_finished() {
            let curve = config.window_move.curve_scaled(motion_scale);
            self.rect = Animation::new(self.rect.value(), target, curve);
        } else {
            self.rect.retarget(target);
        }
    }

    /// Запускает закрытие окна: оно уезжает обратно в свой центр и гаснет.
    pub fn close(&mut self, config: &AnimationConfig, motion_scale: f64) {
        if self.phase == Phase::Closing {
            return;
        }
        self.phase = Phase::Closing;

        let curve = config.window_close.curve_scaled(motion_scale);
        let current = self.rect.value();
        self.rect = Animation::new(current, current.scaled_around_center(OPEN_SCALE), curve);
        self.alpha = Animation::new(self.alpha.value(), 0.0, curve);
    }

    /// Продвигает анимации на один кадр.
    ///
    /// Возвращает `true`, если что-то изменилось и нужна перерисовка. Ответ
    /// `false` позволяет композитору не будить видеокарту впустую.
    pub fn advance(&mut self, dt: Duration) -> bool {
        if self.is_idle() {
            if self.phase == Phase::Opening {
                self.phase = Phase::Open;
                return true;
            }
            return false;
        }

        self.rect.advance(dt);
        self.alpha.advance(dt);

        if self.phase == Phase::Opening && self.is_idle() {
            self.phase = Phase::Open;
        }
        true
    }
}

/// Прозрачность меняется по той же кривой, что и геометрия, но никогда не
/// перелетает: альфа больше единицы бессмысленна, а пружина с перелётом
/// давала бы именно её.
fn fade_curve(curve: Curve) -> Curve {
    match curve {
        Curve::Spring(spring) if spring.damping_ratio < 1.0 => Curve::Spring(hype_anim::Spring {
            damping_ratio: 1.0,
            ..spring
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);
    const TARGET: Rect = Rect::new(100.0, 100.0, 800.0, 600.0);

    fn config() -> AnimationConfig {
        AnimationConfig::default()
    }

    /// Крутит анимацию, пока она не успокоится, и возвращает число кадров.
    fn settle(anim: &mut WindowAnimation) -> usize {
        let mut frames = 0;
        while !anim.is_idle() && frames < 1000 {
            anim.advance(FRAME);
            frames += 1;
        }
        assert!(frames < 1000, "анимация не закончилась");
        frames
    }

    #[test]
    fn a_new_window_grows_into_place_and_fades_in() {
        let anim = WindowAnimation::opening(TARGET, &config(), 1.0);
        assert_eq!(anim.phase(), Phase::Opening);
        assert!(anim.rect().size.w < TARGET.size.w, "окно должно расти");
        assert!(anim.alpha() < 0.01, "окно должно проявляться");
        assert_eq!(anim.rect().center(), TARGET.center(), "рост идёт из центра");
    }

    #[test]
    fn an_opening_window_ends_up_exactly_on_target() {
        let mut anim = WindowAnimation::opening(TARGET, &config(), 1.0);
        settle(&mut anim);
        assert_eq!(anim.rect(), TARGET);
        assert_eq!(anim.alpha(), 1.0);
        assert_eq!(anim.phase(), Phase::Open);
    }

    #[test]
    fn alpha_never_leaves_the_zero_to_one_range() {
        // Пружина по умолчанию перелетает — прозрачность перелетать не должна.
        let mut anim = WindowAnimation::opening(TARGET, &config(), 1.0);
        for _ in 0..200 {
            anim.advance(FRAME);
            let alpha = anim.alpha();
            assert!(
                (0.0..=1.0).contains(&alpha),
                "альфа вышла за пределы: {alpha}"
            );
        }
    }

    #[test]
    fn a_settled_window_needs_no_frames() {
        let mut anim = WindowAnimation::settled(TARGET);
        assert!(anim.is_idle());
        assert_eq!(anim.rect(), TARGET);
        assert!(
            !anim.advance(FRAME),
            "неподвижное окно не требует перерисовки"
        );
    }

    #[test]
    fn moving_a_window_animates_it_to_the_new_place() {
        let mut anim = WindowAnimation::settled(TARGET);
        let destination = Rect::new(900.0, 100.0, 800.0, 600.0);

        anim.move_to(destination, &config(), 1.0);
        assert!(!anim.is_idle());
        assert_ne!(anim.rect(), destination, "прыжок вместо анимации");

        settle(&mut anim);
        assert_eq!(anim.rect(), destination);
    }

    #[test]
    fn moving_to_the_current_target_changes_nothing() {
        let mut anim = WindowAnimation::settled(TARGET);
        anim.move_to(TARGET, &config(), 1.0);
        assert!(anim.is_idle());
    }

    #[test]
    fn retargeting_mid_flight_does_not_jump() {
        let mut anim = WindowAnimation::settled(TARGET);
        anim.move_to(Rect::new(900.0, 100.0, 800.0, 600.0), &config(), 1.0);
        for _ in 0..5 {
            anim.advance(FRAME);
        }

        let before = anim.rect();
        anim.move_to(Rect::new(100.0, 700.0, 800.0, 600.0), &config(), 1.0);
        let after = anim.rect();

        assert_eq!(before, after, "картинка дёрнулась при смене цели");
    }

    #[test]
    fn a_closing_window_fades_out_and_reports_itself_gone() {
        let mut anim = WindowAnimation::settled(TARGET);
        anim.close(&config(), 1.0);
        assert_eq!(anim.phase(), Phase::Closing);
        assert!(!anim.is_gone());

        settle(&mut anim);
        assert!(anim.is_gone());
        assert_eq!(anim.alpha(), 0.0);
    }

    #[test]
    fn a_closing_window_ignores_further_moves() {
        let mut anim = WindowAnimation::settled(TARGET);
        anim.close(&config(), 1.0);
        let target_after_close = anim.target();

        anim.move_to(Rect::new(0.0, 0.0, 100.0, 100.0), &config(), 1.0);
        assert_eq!(anim.target(), target_after_close);
    }

    #[test]
    fn closing_twice_does_not_restart_the_animation() {
        let mut anim = WindowAnimation::settled(TARGET);
        anim.close(&config(), 1.0);
        anim.advance(FRAME);
        let alpha = anim.alpha();

        anim.close(&config(), 1.0);
        assert_eq!(anim.alpha(), alpha);
    }

    #[test]
    fn disabling_motion_makes_windows_appear_instantly() {
        let mut anim = WindowAnimation::opening(TARGET, &config(), 0.0);
        assert!(
            anim.is_idle(),
            "с выключенными анимациями окно уже на месте"
        );
        assert_eq!(anim.rect(), TARGET);
        assert_eq!(anim.alpha(), 1.0);

        anim.close(&config(), 0.0);
        assert!(anim.is_gone());
    }

    #[test]
    fn a_slower_motion_scale_takes_more_frames() {
        let mut fast = WindowAnimation::settled(TARGET);
        let mut slow = WindowAnimation::settled(TARGET);
        let destination = Rect::new(900.0, 700.0, 800.0, 600.0);

        fast.move_to(destination, &config(), 1.0);
        slow.move_to(destination, &config(), 2.5);

        assert!(settle(&mut slow) > settle(&mut fast));
    }

    #[test]
    fn advance_reports_whether_a_redraw_is_needed() {
        let mut anim = WindowAnimation::settled(TARGET);
        assert!(!anim.advance(FRAME));

        anim.move_to(Rect::new(0.0, 0.0, 200.0, 200.0), &config(), 1.0);
        assert!(anim.advance(FRAME));

        settle(&mut anim);
        assert!(
            !anim.advance(FRAME),
            "доехавшее окно продолжает просить кадры"
        );
    }
}
