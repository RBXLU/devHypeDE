//! Раскладка окон.
//!
//! Модуль сознательно не знает ни про Wayland, ни про графику: на входе —
//! область экрана, список окон и настройки, на выходе — прямоугольники. Такую
//! функцию можно проверить тестами до последнего пикселя, что для раскладки
//! важнее всего: ошибка здесь видна пользователю в каждую секунду работы.

use hype_anim::Rect;
use hype_config::{LayoutConfig, LayoutMode};

/// Окно глазами раскладки.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutWindow {
    pub id: u64,
    /// Окно вынуто из плитки и живёт своей геометрией.
    pub floating: bool,
    /// Окно на весь экран: ни зазоров, ни рамок.
    pub fullscreen: bool,
    /// Геометрия плавающего окна. Для окон в плитке значение игнорируется.
    pub floating_rect: Rect,
}

impl LayoutWindow {
    /// Обычное окно в плитке.
    pub fn tiled(id: u64) -> Self {
        Self {
            id,
            floating: false,
            fullscreen: false,
            floating_rect: Rect::new(0.0, 0.0, 800.0, 600.0),
        }
    }
}

/// Вычисленное место окна.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub id: u64,
    pub rect: Rect,
    /// Окно перекрывает всё остальное.
    pub fullscreen: bool,
}

/// Раскладывает окна по области экрана.
///
/// Порядок в `windows` задаёт порядок в плитке: первое окно становится
/// главным. Возвращаемый список сохраняет порядок входа, чтобы вызывающий код
/// мог сопоставлять окна по индексу.
pub fn arrange(area: Rect, windows: &[LayoutWindow], config: &LayoutConfig) -> Vec<Tile> {
    let floating_mode = config.mode == LayoutMode::Floating;

    // Зазоры ужимаются, если экран мал: лучше показать окна без отступов, чем
    // отдать им нулевую площадь. Это не выдуманный случай — окно вложенного
    // композитора можно растянуть мышью до нескольких десятков пикселей.
    let outer = clamp_gap(config.gaps_outer, area.size.w.min(area.size.h), 2.0);
    let work_area = area.inset(outer);

    let tiled_count = windows
        .iter()
        .filter(|w| !w.floating && !floating_mode)
        .count();
    let tiled_rects = arrange_master_stack(work_area, tiled_count, config);

    let mut tiles = Vec::with_capacity(windows.len());
    let mut next_tiled = 0;

    for window in windows {
        let rect = if window.floating || floating_mode {
            clamp_into(window.floating_rect, area)
        } else {
            let rect = tiled_rects[next_tiled];
            next_tiled += 1;
            rect
        };

        // Полноэкранное окно забирает область целиком, без отступов: иначе
        // видео с чёрными полями по краям выглядит поломкой. Геометрия в
        // плитке при этом сохраняется за остальными окнами — выход из полного
        // экрана не вызывает лавины изменений размера.
        tiles.push(if window.fullscreen {
            Tile {
                id: window.id,
                rect: area,
                fullscreen: true,
            }
        } else {
            Tile {
                id: window.id,
                rect,
                fullscreen: false,
            }
        });
    }

    tiles
}

/// Классическая раскладка «главное окно и стопка»: первое окно занимает левую
/// колонку, остальные делят правую по вертикали. При одном окне колонок нет —
/// оно занимает всё рабочее поле.
fn arrange_master_stack(area: Rect, count: usize, config: &LayoutConfig) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![area];
    }

    let stack_count = count - 1;
    // Горизонтальный зазор — один, вертикальных — на один меньше числа окон
    // в стопке.
    let gap_h = clamp_gap(config.gaps_inner, area.size.w, 1.0);
    let gap_v = clamp_gap(config.gaps_inner, area.size.h, stack_count as f64 - 1.0);
    let ratio = config.master_ratio.clamp(0.1, 0.9);

    let master_width = ((area.size.w - gap_h) * ratio).max(MIN_TILE);
    let stack_width = (area.size.w - gap_h - master_width).max(MIN_TILE);
    let stack_x = area.origin.x + master_width + gap_h;

    let mut rects = Vec::with_capacity(count);
    rects.push(Rect::new(
        area.origin.x,
        area.origin.y,
        master_width,
        area.size.h.max(MIN_TILE),
    ));

    let gap = gap_v;
    // Высоту делим так, чтобы сумма кусков и зазоров точно совпала с высотой
    // области: накапливать округления нельзя, иначе снизу останется щель.
    let total_gaps = gap * (stack_count as f64 - 1.0);
    let available = (area.size.h - total_gaps).max(MIN_TILE);

    for i in 0..stack_count {
        let start = area.origin.y + (available * i as f64 / stack_count as f64) + gap * i as f64;
        let end = area.origin.y
            + (available * (i + 1) as f64 / stack_count as f64)
            + gap * i as f64;
        rects.push(Rect::new(stack_x, start, stack_width, (end - start).max(MIN_TILE)));
    }

    rects
}

/// Наименьшая сторона окна. Нулевой размер ломает и клиентов, и отрисовку,
/// поэтому окно всегда занимает хотя бы пиксель.
const MIN_TILE: f64 = 1.0;

/// Ограничивает зазор так, чтобы `slots` зазоров не съели больше половины
/// доступной длины.
fn clamp_gap(gap: f64, available: f64, slots: f64) -> f64 {
    if gap <= 0.0 || available <= 0.0 || slots <= 0.0 {
        return gap.max(0.0);
    }
    gap.min(available / (2.0 * slots)).max(0.0)
}

/// Вгоняет прямоугольник в границы области, сохраняя размер, если он влезает.
///
/// Плавающее окно не должно уезжать за край экрана целиком: оттуда его не
/// достать мышью.
fn clamp_into(rect: Rect, area: Rect) -> Rect {
    let w = rect.size.w.min(area.size.w);
    let h = rect.size.h.min(area.size.h);
    let x = rect
        .origin
        .x
        .clamp(area.origin.x, (area.origin.x + area.size.w - w).max(area.origin.x));
    let y = rect
        .origin
        .y
        .clamp(area.origin.y, (area.origin.y + area.size.h - h).max(area.origin.y));
    Rect::new(x, y, w, h)
}

/// Выбирает окно, на которое перейдёт фокус при движении в заданную сторону.
///
/// Берётся ближайшее окно, центр которого лежит в нужной стороне. Смещение
/// поперёк направления штрафуется вдвое: при движении вправо окно чуть выше,
/// но прямо рядом, ощущается более «правым», чем далёкое и ровно напротив.
pub fn focus_target(
    from: Rect,
    candidates: &[(u64, Rect)],
    direction: hype_config::Direction,
) -> Option<u64> {
    let origin = from.center();
    let (dx, dy) = direction.vector();

    candidates
        .iter()
        .filter_map(|(id, rect)| {
            let center = rect.center();
            let offset = (center.x - origin.x, center.y - origin.y);
            let along = offset.0 * dx + offset.1 * dy;
            // Строго в нужную сторону: окна вплотную и позади не считаются.
            if along <= 1.0 {
                return None;
            }
            let across = (offset.0 * dy - offset.1 * dx).abs();
            Some((*id, along + across * 2.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hype_config::Direction;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1920.0, 1080.0);

    fn config() -> LayoutConfig {
        LayoutConfig {
            gaps_inner: 10.0,
            gaps_outer: 20.0,
            master_ratio: 0.5,
            ..LayoutConfig::default()
        }
    }

    fn tiled(count: usize) -> Vec<LayoutWindow> {
        (0..count as u64).map(LayoutWindow::tiled).collect()
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.origin.x < b.origin.x + b.size.w
            && b.origin.x < a.origin.x + a.size.w
            && a.origin.y < b.origin.y + b.size.h
            && b.origin.y < a.origin.y + a.size.h
    }

    #[test]
    fn no_windows_means_no_tiles() {
        assert!(arrange(SCREEN, &[], &config()).is_empty());
    }

    #[test]
    fn a_single_window_fills_the_area_minus_the_outer_gap() {
        let tiles = arrange(SCREEN, &tiled(1), &config());
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].rect, Rect::new(20.0, 20.0, 1880.0, 1040.0));
    }

    #[test]
    fn two_windows_split_by_the_master_ratio() {
        let tiles = arrange(SCREEN, &tiled(2), &config());
        // Рабочее поле 1880 шириной, минус зазор 10 — по 935 на колонку.
        assert_eq!(tiles[0].rect, Rect::new(20.0, 20.0, 935.0, 1040.0));
        assert_eq!(tiles[1].rect, Rect::new(965.0, 20.0, 935.0, 1040.0));
    }

    #[test]
    fn the_master_ratio_is_respected() {
        let mut cfg = config();
        cfg.master_ratio = 0.7;
        let tiles = arrange(SCREEN, &tiled(2), &cfg);
        let master = tiles[0].rect.size.w;
        let stack = tiles[1].rect.size.w;
        assert!((master / (master + stack) - 0.7).abs() < 0.01);
    }

    #[test]
    fn an_absurd_master_ratio_is_clamped_instead_of_producing_zero_width() {
        let mut cfg = config();
        cfg.master_ratio = 5.0;
        let tiles = arrange(SCREEN, &tiled(2), &cfg);
        assert!(tiles[0].rect.size.w > 0.0);
        assert!(tiles[1].rect.size.w > 0.0);
    }

    #[test]
    fn tiles_never_overlap() {
        for count in 1..=8 {
            let tiles = arrange(SCREEN, &tiled(count), &config());
            for i in 0..tiles.len() {
                for j in (i + 1)..tiles.len() {
                    assert!(
                        !overlaps(tiles[i].rect, tiles[j].rect),
                        "окна {i} и {j} наложились при {count} окнах"
                    );
                }
            }
        }
    }

    #[test]
    fn tiles_stay_inside_the_screen() {
        for count in 1..=8 {
            for tile in arrange(SCREEN, &tiled(count), &config()) {
                assert!(tile.rect.origin.x >= SCREEN.origin.x - 1e-9);
                assert!(tile.rect.origin.y >= SCREEN.origin.y - 1e-9);
                assert!(tile.rect.origin.x + tile.rect.size.w <= SCREEN.size.w + 1e-9);
                assert!(tile.rect.origin.y + tile.rect.size.h <= SCREEN.size.h + 1e-9);
            }
        }
    }

    #[test]
    fn the_stack_uses_the_full_height_without_leftover_gaps() {
        let tiles = arrange(SCREEN, &tiled(4), &config());
        let stack: Vec<Rect> = tiles[1..].iter().map(|t| t.rect).collect();

        let top = stack.first().unwrap().origin.y;
        let bottom = stack.last().unwrap();
        let bottom_edge = bottom.origin.y + bottom.size.h;

        assert!((top - 20.0).abs() < 1e-9, "стопка не прижата к верху");
        assert!(
            (bottom_edge - 1060.0).abs() < 1e-9,
            "снизу осталась щель: {bottom_edge}"
        );
    }

    #[test]
    fn the_stack_is_divided_evenly() {
        let tiles = arrange(SCREEN, &tiled(4), &config());
        let heights: Vec<f64> = tiles[1..].iter().map(|t| t.rect.size.h).collect();
        for h in &heights {
            assert!((h - heights[0]).abs() < 1.0, "куски разной высоты: {heights:?}");
        }
    }

    #[test]
    fn gaps_between_stacked_windows_match_the_setting() {
        let cfg = config();
        let tiles = arrange(SCREEN, &tiled(3), &cfg);
        let first = tiles[1].rect;
        let second = tiles[2].rect;
        let gap = second.origin.y - (first.origin.y + first.size.h);
        assert!((gap - cfg.gaps_inner).abs() < 1e-9, "зазор {gap}");
    }

    #[test]
    fn zero_gaps_produce_a_seamless_grid() {
        let cfg = LayoutConfig {
            gaps_inner: 0.0,
            gaps_outer: 0.0,
            master_ratio: 0.5,
            ..LayoutConfig::default()
        };
        let tiles = arrange(SCREEN, &tiled(2), &cfg);
        assert_eq!(tiles[0].rect, Rect::new(0.0, 0.0, 960.0, 1080.0));
        assert_eq!(tiles[1].rect, Rect::new(960.0, 0.0, 960.0, 1080.0));
    }

    #[test]
    fn a_fullscreen_window_takes_the_whole_screen() {
        let mut windows = tiled(3);
        windows[1].fullscreen = true;
        let tiles = arrange(SCREEN, &windows, &config());

        let full: Vec<&Tile> = tiles.iter().filter(|t| t.fullscreen).collect();
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].id, 1);
        assert_eq!(full[0].rect, SCREEN, "полный экран не должен иметь отступов");
    }

    #[test]
    fn floating_windows_keep_their_own_geometry() {
        let mut windows = tiled(2);
        windows[0].floating = true;
        windows[0].floating_rect = Rect::new(300.0, 200.0, 640.0, 480.0);

        let tiles = arrange(SCREEN, &windows, &config());
        assert_eq!(tiles[0].rect, Rect::new(300.0, 200.0, 640.0, 480.0));
        // Оставшееся окно получает всю плитку целиком.
        assert_eq!(tiles[1].rect, Rect::new(20.0, 20.0, 1880.0, 1040.0));
    }

    #[test]
    fn a_floating_window_cannot_escape_the_screen() {
        let mut windows = tiled(1);
        windows[0].floating = true;
        windows[0].floating_rect = Rect::new(5000.0, -800.0, 640.0, 480.0);

        let tiles = arrange(SCREEN, &windows, &config());
        assert_eq!(tiles[0].rect, Rect::new(1280.0, 0.0, 640.0, 480.0));
    }

    #[test]
    fn an_oversized_floating_window_shrinks_to_the_screen() {
        let mut windows = tiled(1);
        windows[0].floating = true;
        windows[0].floating_rect = Rect::new(0.0, 0.0, 4000.0, 3000.0);

        let tiles = arrange(SCREEN, &windows, &config());
        assert_eq!(tiles[0].rect.size, SCREEN.size);
    }

    #[test]
    fn floating_mode_leaves_every_window_where_it_is() {
        let mut cfg = config();
        cfg.mode = LayoutMode::Floating;

        let mut windows = tiled(2);
        windows[0].floating_rect = Rect::new(100.0, 100.0, 400.0, 300.0);
        windows[1].floating_rect = Rect::new(200.0, 150.0, 400.0, 300.0);

        let tiles = arrange(SCREEN, &windows, &cfg);
        assert_eq!(tiles[0].rect, windows[0].floating_rect);
        assert_eq!(tiles[1].rect, windows[1].floating_rect);
    }

    #[test]
    fn a_tiny_screen_still_produces_usable_rectangles() {
        // Окно композитора можно ужать до смешного размера — падать нельзя.
        let tiny = Rect::new(0.0, 0.0, 40.0, 30.0);
        for tile in arrange(tiny, &tiled(4), &config()) {
            assert!(tile.rect.size.w > 0.0 && tile.rect.size.h > 0.0, "{tile:?}");
        }
    }

    #[test]
    fn gaps_shrink_instead_of_swallowing_a_small_screen() {
        // Экран меньше, чем сумма зазоров: отступы должны ужаться.
        let small = Rect::new(0.0, 0.0, 60.0, 50.0);
        let tiles = arrange(small, &tiled(3), &config());
        for tile in &tiles {
            assert!(
                tile.rect.size.w >= 1.0 && tile.rect.size.h >= 1.0,
                "окно вышло нулевым: {tile:?}"
            );
        }
        assert!(
            tiles.iter().all(|t| t.rect.origin.x >= 0.0 && t.rect.origin.y >= 0.0),
            "окно уехало за край"
        );
    }

    #[test]
    fn gaps_are_untouched_when_there_is_room() {
        // Ужимание не должно срабатывать на нормальном экране.
        let cfg = config();
        let tiles = arrange(SCREEN, &tiled(2), &cfg);
        assert_eq!(tiles[0].rect.origin.x, cfg.gaps_outer);
    }

    // --- переход фокуса ---

    #[test]
    fn focus_moves_to_the_window_in_that_direction() {
        let current = Rect::new(0.0, 0.0, 100.0, 100.0);
        let candidates = [
            (1, Rect::new(200.0, 0.0, 100.0, 100.0)),
            (2, Rect::new(0.0, 200.0, 100.0, 100.0)),
            (3, Rect::new(-200.0, 0.0, 100.0, 100.0)),
        ];

        assert_eq!(focus_target(current, &candidates, Direction::Right), Some(1));
        assert_eq!(focus_target(current, &candidates, Direction::Down), Some(2));
        assert_eq!(focus_target(current, &candidates, Direction::Left), Some(3));
        assert_eq!(focus_target(current, &candidates, Direction::Up), None);
    }

    #[test]
    fn focus_prefers_the_nearest_window() {
        let current = Rect::new(0.0, 0.0, 100.0, 100.0);
        let candidates = [
            (1, Rect::new(600.0, 0.0, 100.0, 100.0)),
            (2, Rect::new(200.0, 0.0, 100.0, 100.0)),
        ];
        assert_eq!(focus_target(current, &candidates, Direction::Right), Some(2));
    }

    #[test]
    fn focus_tolerates_a_window_that_is_slightly_off_axis() {
        let current = Rect::new(0.0, 0.0, 100.0, 100.0);
        let candidates = [
            // Ровно напротив, но далеко.
            (1, Rect::new(900.0, 0.0, 100.0, 100.0)),
            // Рядом, но чуть выше.
            (2, Rect::new(200.0, -60.0, 100.0, 100.0)),
        ];
        assert_eq!(focus_target(current, &candidates, Direction::Right), Some(2));
    }

    #[test]
    fn focus_ignores_the_window_itself_and_exact_overlaps() {
        let current = Rect::new(0.0, 0.0, 100.0, 100.0);
        let candidates = [(1, current)];
        assert_eq!(focus_target(current, &candidates, Direction::Right), None);
    }

    #[test]
    fn focus_returns_nothing_when_there_is_nowhere_to_go() {
        assert_eq!(
            focus_target(Rect::new(0.0, 0.0, 100.0, 100.0), &[], Direction::Left),
            None
        );
    }
}
