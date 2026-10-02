//! Указатель мыши.
//!
//! Указатель рисует сам композитор — и в сеансе на видеокарте, и во
//! вложенном режиме. Во вложенном хозяйский указатель прячется, иначе на
//! экране было бы две стрелки.
//!
//! Берётся тема XCursor, указанная в настройках. Если её нет, используется
//! встроенная стрелка: остаться вовсе без указателя хуже, чем показать не тот.

use std::sync::Mutex;
use std::time::Duration;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{render_elements, Kind};
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer};
use smithay::input::pointer::{CursorImageAttributes, CursorImageStatus};
use smithay::utils::{Logical, Physical, Point, Scale, Transform};
use smithay::wayland::compositor::with_states;
use xcursor::parser::{parse_xcursor, Image};
use xcursor::CursorTheme;

render_elements! {
    /// Слой указателя.
    ///
    /// Указатель бывает двух видов: картинка из темы и поверхность, которую
    /// нарисовал сам клиент (например, курсор-крестик в графическом
    /// редакторе). Оба вида приходится сложить в один тип.
    pub CursorElement<R> where R: ImportAll + ImportMem;
    Surface = WaylandSurfaceRenderElement<R>,
    Memory = MemoryRenderBufferRenderElement<R>,
}

/// Встроенная стрелка на случай, когда тема курсоров не нашлась.
///
/// Рисунок задан картой символов: `#` — чёрный контур, `.` — белая заливка,
/// пробел — прозрачно. Так форму видно прямо в исходнике, и её можно
/// поправить, не открывая графический редактор.
const FALLBACK_ARROW: &[&str] = &[
    "#                       ",
    "##                      ",
    "#.#                     ",
    "#..#                    ",
    "#...#                   ",
    "#....#                  ",
    "#.....#                 ",
    "#......#                ",
    "#.......#               ",
    "#........#              ",
    "#.........#             ",
    "#..........#            ",
    "#...........#           ",
    "#............#          ",
    "#.....########          ",
    "#..#..#                 ",
    "#.# #..#                ",
    "##  #..#                ",
    "#    #..#               ",
    "     #..#               ",
    "      #.#               ",
    "      ###               ",
    "                        ",
    "                        ",
];

/// Загруженный указатель.
pub struct Cursor {
    frames: Vec<Image>,
    /// Кадры, уже разложенные для отрисовщика.
    ///
    /// Раскладка кадра стоит заметно дороже, чем его показ, а кадр за кадром
    /// повторяется один и тот же рисунок, поэтому готовый буфер сохраняется
    /// рядом с исходным изображением.
    buffers: Vec<Option<MemoryRenderBuffer>>,
    size: u32,
}

impl std::fmt::Debug for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cursor")
            .field("frames", &self.frames.len())
            .field("size", &self.size)
            .finish()
    }
}

impl Cursor {
    /// Загружает указатель из темы, названной в настройках.
    pub fn load(theme_name: &str, size: u32) -> Self {
        let size = size.max(1);

        let frames = load_theme_frames(theme_name).unwrap_or_else(|| {
            tracing::warn!(
                "тема курсоров «{theme_name}» не найдена, используется встроенная стрелка"
            );
            vec![fallback_image(size)]
        });

        Self {
            buffers: (0..frames.len()).map(|_| None).collect(),
            frames,
            size,
        }
    }

    /// Кадр указателя для заданного момента времени.
    ///
    /// У анимированных курсоров (например, «занято») кадры сменяются по
    /// задержкам, записанным в самой теме.
    pub fn frame(&self, time: Duration) -> &Image {
        &self.frames[self.frame_index(time)]
    }

    /// Номер кадра, который нужно показать.
    fn frame_index(&self, time: Duration) -> usize {
        let candidates = nearest_frames(self.size, &self.frames);

        let total: u32 = candidates
            .iter()
            .map(|&index| self.frames[index].delay)
            .sum();
        if total == 0 {
            return candidates[0];
        }

        let mut elapsed = time.as_millis() as u32 % total;
        for &index in &candidates {
            if elapsed < self.frames[index].delay {
                return index;
            }
            elapsed -= self.frames[index].delay;
        }

        candidates[0]
    }

    /// Готовит кадр к отрисовке.
    ///
    /// Вместе с буфером возвращается остриё: точка внутри рисунка, которая и
    /// считается положением указателя.
    pub fn buffer(&mut self, time: Duration) -> (&MemoryRenderBuffer, Point<i32, Logical>) {
        let index = self.frame_index(time);
        let image = &self.frames[index];
        let hotspot = Point::from((image.xhot as i32, image.yhot as i32));

        let buffer = self.buffers[index].get_or_insert_with(|| {
            MemoryRenderBuffer::from_slice(
                &image.pixels_rgba,
                // Байты идут как R, G, B, A — в обозначениях DRM это Abgr8888.
                Fourcc::Abgr8888,
                (image.width as i32, image.height as i32),
                1,
                Transform::Normal,
                None,
            )
        });

        (buffer, hotspot)
    }

    /// Собирает слои указателя для одного кадра.
    ///
    /// `location` — положение указателя в логических точках экрана. Если
    /// клиент прислал свой рисунок, показывается он; если клиент попросил
    /// спрятать указатель, не показывается ничего.
    pub fn render<R>(
        &mut self,
        renderer: &mut R,
        status: &CursorImageStatus,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        time: Duration,
    ) -> Vec<CursorElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        match status {
            CursorImageStatus::Hidden => Vec::new(),
            CursorImageStatus::Surface(surface) => {
                let hotspot = surface_hotspot(surface);
                let position = position_on_screen(location, hotspot, scale);
                render_elements_from_surface_tree(
                    renderer,
                    surface,
                    position,
                    scale,
                    1.0,
                    Kind::Cursor,
                )
            }
            // Имя указателя («стрелка», «текст», «ожидание») мы пока не
            // различаем: из темы берётся обычная стрелка.
            CursorImageStatus::Named(_) => {
                let (buffer, hotspot) = self.buffer(time);
                let buffer = buffer.clone();
                let position = position_on_screen(location, hotspot, scale);

                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    position.to_f64(),
                    &buffer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    Ok(element) => vec![CursorElement::Memory(element)],
                    Err(err) => {
                        tracing::warn!("указатель не показан: {err}");
                        Vec::new()
                    }
                }
            }
        }
    }
}

/// Переводит положение указателя в точки на экране с поправкой на остриё.
fn position_on_screen(
    location: Point<f64, Logical>,
    hotspot: Point<i32, Logical>,
    scale: Scale<f64>,
) -> Point<i32, Physical> {
    (location - hotspot.to_f64())
        .to_physical(scale)
        .to_i32_round()
}

/// Остриё указателя, который нарисовал сам клиент.
fn surface_hotspot(
    surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
) -> Point<i32, Logical> {
    with_states(surface, |states| {
        states
            .data_map
            .get::<Mutex<CursorImageAttributes>>()
            .map(|attributes| attributes.lock().unwrap().hotspot)
            .unwrap_or_default()
    })
}

/// Кадры одного размера, ближайшего к запрошенному.
fn nearest_frames(size: u32, frames: &[Image]) -> Vec<usize> {
    let nearest = frames
        .iter()
        .min_by_key(|image| (size as i32 - image.size as i32).abs())
        .expect("список кадров не бывает пустым");
    let (width, height) = (nearest.width, nearest.height);

    frames
        .iter()
        .enumerate()
        .filter(|(_, image)| image.width == width && image.height == height)
        .map(|(index, _)| index)
        .collect()
}

/// Читает кадры указателя из установленной темы.
fn load_theme_frames(theme_name: &str) -> Option<Vec<Image>> {
    let theme = CursorTheme::load(theme_name);

    // У разных тем указатель по умолчанию называется по-разному.
    let path = ["default", "left_ptr", "arrow"]
        .iter()
        .find_map(|name| theme.load_icon(name))?;

    let data = std::fs::read(path).ok()?;
    let frames = parse_xcursor(&data)?;

    (!frames.is_empty()).then_some(frames)
}

/// Рисует встроенную стрелку нужного размера.
fn fallback_image(size: u32) -> Image {
    let source_height = FALLBACK_ARROW.len() as u32;
    let source_width = FALLBACK_ARROW
        .iter()
        .map(|row| row.chars().count() as u32)
        .max()
        .unwrap_or(1);

    // Целое увеличение вместо сглаживания: у стрелки нет полутонов, и резкие
    // края выглядят лучше размытых.
    let scale = (size / source_height).max(1);
    let width = source_width * scale;
    let height = source_height * scale;

    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let symbol = FALLBACK_ARROW
                .get((y / scale) as usize)
                .and_then(|row| row.chars().nth((x / scale) as usize))
                .unwrap_or(' ');

            pixels.extend_from_slice(match symbol {
                '#' => &[0, 0, 0, 255],
                '.' => &[255, 255, 255, 255],
                _ => &[0, 0, 0, 0],
            });
        }
    }

    Image {
        size,
        width,
        height,
        // Остриё стрелки — левый верхний угол рисунка.
        xhot: 0,
        yhot: 0,
        delay: 0,
        pixels_rgba: pixels,
        pixels_argb: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_arrow_is_a_valid_image() {
        let image = fallback_image(24);
        assert_eq!(
            image.pixels_rgba.len() as u32,
            image.width * image.height * 4
        );
        assert!(image.width > 0 && image.height > 0);
    }

    #[test]
    fn the_fallback_arrow_has_a_visible_tip() {
        let image = fallback_image(24);
        // Левый верхний угол — остриё, оно обязано быть непрозрачным.
        assert_eq!(image.pixels_rgba[3], 255, "остриё стрелки прозрачно");
    }

    #[test]
    fn the_fallback_arrow_has_transparent_corners() {
        let image = fallback_image(24);
        // Правый верхний угол в рисунок не входит.
        let index = ((image.width - 1) * 4 + 3) as usize;
        assert_eq!(image.pixels_rgba[index], 0, "угол должен быть прозрачным");
    }

    #[test]
    fn the_fallback_arrow_scales_up() {
        let small = fallback_image(24);
        let large = fallback_image(48);
        assert!(large.width > small.width, "крупный указатель не увеличился");
    }

    #[test]
    fn an_absurd_size_does_not_break_the_arrow() {
        for size in [0, 1, 512] {
            let image = fallback_image(size);
            assert!(image.width > 0 && image.height > 0, "размер {size}");
        }
    }

    #[test]
    fn a_missing_theme_falls_back_instead_of_failing() {
        let cursor = Cursor::load("такой-темы-нет", 24);
        assert!(!cursor.frames.is_empty());
    }

    #[test]
    fn a_still_cursor_always_returns_the_same_frame() {
        let cursor = Cursor::load("такой-темы-нет", 24);
        let first = cursor.frame(Duration::ZERO).width;
        let later = cursor.frame(Duration::from_secs(5)).width;
        assert_eq!(first, later);
    }

    #[test]
    fn animated_frames_follow_their_delays() {
        let frames = vec![
            Image {
                size: 24,
                width: 24,
                height: 24,
                xhot: 0,
                yhot: 0,
                delay: 100,
                pixels_rgba: vec![0; 24 * 24 * 4],
                pixels_argb: Vec::new(),
            },
            Image {
                size: 24,
                width: 24,
                height: 24,
                xhot: 1,
                yhot: 1,
                delay: 100,
                pixels_rgba: vec![0; 24 * 24 * 4],
                pixels_argb: Vec::new(),
            },
        ];
        let cursor = Cursor {
            buffers: (0..frames.len()).map(|_| None).collect(),
            frames,
            size: 24,
        };

        assert_eq!(cursor.frame(Duration::from_millis(50)).xhot, 0);
        assert_eq!(cursor.frame(Duration::from_millis(150)).xhot, 1);
        // Через полный круг начинается сначала.
        assert_eq!(cursor.frame(Duration::from_millis(250)).xhot, 0);
    }
}
