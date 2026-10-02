//! Обои рабочего стола.
//!
//! Изображение подгоняется под экран на процессоре один раз при загрузке, а не
//! на каждом кадре: масштабирование средствами видеокарты потребовало бы
//! своего шейдера, а обои меняются несопоставимо реже, чем рисуются кадры.

use std::path::Path;

use hype_config::WallpaperMode;
use hype_theme::Color;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;

/// Куда ляжет изображение на экране.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Во сколько раз изображение увеличено.
    pub scale: f64,
    /// Положение левого верхнего угла масштабированного изображения
    /// относительно экрана. Отрицательные значения означают обрезку.
    pub x: f64,
    pub y: f64,
}

impl Placement {
    /// Ширина изображения после масштабирования.
    pub fn width(&self, image_width: u32) -> f64 {
        image_width as f64 * self.scale
    }

    /// Высота изображения после масштабирования.
    pub fn height(&self, image_height: u32) -> f64 {
        image_height as f64 * self.scale
    }
}

/// Считает, как изображение ложится на экран.
///
/// Функция чистая, поэтому все режимы проверены тестами: ошибка здесь видна
/// пользователю при каждом взгляде на рабочий стол.
pub fn place(image: (u32, u32), screen: (u32, u32), mode: WallpaperMode) -> Placement {
    let (iw, ih) = (image.0.max(1) as f64, image.1.max(1) as f64);
    let (sw, sh) = (screen.0.max(1) as f64, screen.1.max(1) as f64);

    let scale = match mode {
        // Заполнить: берём больший коэффициент, лишнее уходит за края.
        WallpaperMode::Fill => (sw / iw).max(sh / ih),
        // Вписать: берём меньший, по краям остаются поля.
        WallpaperMode::Fit => (sw / iw).min(sh / ih),
        WallpaperMode::Center | WallpaperMode::Color => 1.0,
    };

    Placement {
        scale,
        x: (sw - iw * scale) / 2.0,
        y: (sh - ih * scale) / 2.0,
    }
}

/// Готовые обои: буфер размером ровно с экран.
#[derive(Debug, Clone)]
pub struct Wallpaper {
    pub buffer: MemoryRenderBuffer,
    pub width: i32,
    pub height: i32,
}

/// Готовит обои под размер экрана.
///
/// Если изображения нет или его не удалось прочитать, возвращается сплошная
/// заливка цветом фона темы — рабочий стол не должен оставаться чёрным
/// из-за испорченного файла.
pub fn prepare(
    path: Option<&Path>,
    mode: WallpaperMode,
    screen: (u32, u32),
    background: Color,
) -> Wallpaper {
    let (width, height) = (screen.0.max(1), screen.1.max(1));
    let mut canvas = solid_canvas(width, height, background);

    if mode != WallpaperMode::Color {
        if let Some(path) = path {
            match image::open(path) {
                Ok(image) => draw_image(&mut canvas, width, height, &image.to_rgba8(), mode),
                Err(err) => tracing::warn!("обои {} не прочитаны: {err}", path.display()),
            }
        }
    }

    Wallpaper {
        buffer: MemoryRenderBuffer::from_slice(
            &canvas,
            // Байты лежат в порядке R, G, B, A. В обозначениях DRM это
            // Abgr8888: имя формата читается от старшего байта к младшему, а
            // в памяти всё идёт наоборот. Argb8888 здесь поменял бы местами
            // красный и синий — небо стало бы оранжевым.
            Fourcc::Abgr8888,
            (width as i32, height as i32),
            1,
            Transform::Normal,
            None,
        ),
        width: width as i32,
        height: height as i32,
    }
}

/// Холст, залитый сплошным цветом.
fn solid_canvas(width: u32, height: u32, color: Color) -> Vec<u8> {
    let quantise = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let pixel = [quantise(color.r), quantise(color.g), quantise(color.b), 255];

    let mut canvas = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        canvas.extend_from_slice(&pixel);
    }
    canvas
}

/// Рисует изображение поверх холста согласно режиму.
fn draw_image(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    image: &image::RgbaImage,
    mode: WallpaperMode,
) {
    let placement = place((image.width(), image.height()), (width, height), mode);

    let scaled_width = placement.width(image.width()).round().max(1.0) as u32;
    let scaled_height = placement.height(image.height()).round().max(1.0) as u32;

    // Lanczos даёт заметно более чистый результат на фотографиях, а считается
    // один раз при смене обоев или разрешения.
    let scaled = if scaled_width == image.width() && scaled_height == image.height() {
        image.clone()
    } else {
        image::imageops::resize(
            image,
            scaled_width,
            scaled_height,
            image::imageops::FilterType::Lanczos3,
        )
    };

    let offset_x = placement.x.round() as i64;
    let offset_y = placement.y.round() as i64;

    for y in 0..scaled_height as i64 {
        let target_y = y + offset_y;
        if target_y < 0 || target_y >= height as i64 {
            continue;
        }
        for x in 0..scaled_width as i64 {
            let target_x = x + offset_x;
            if target_x < 0 || target_x >= width as i64 {
                continue;
            }

            let source = scaled.get_pixel(x as u32, y as u32).0;
            let index = ((target_y * width as i64 + target_x) * 4) as usize;
            canvas[index..index + 4].copy_from_slice(&source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_covers_the_whole_screen() {
        // Широкое изображение на квадратном экране: подгоняем по высоте,
        // бока обрезаются.
        let placement = place((1000, 500), (800, 800), WallpaperMode::Fill);
        assert!((placement.scale - 1.6).abs() < 1e-9);
        assert!(placement.width(1000) >= 800.0);
        assert!(placement.height(500) >= 800.0);
        assert!(placement.x < 0.0, "лишнее должно уходить за края");
        assert!((placement.y).abs() < 1e-9);
    }

    #[test]
    fn fit_shows_the_whole_image() {
        let placement = place((1000, 500), (800, 800), WallpaperMode::Fit);
        assert!((placement.scale - 0.8).abs() < 1e-9);
        assert!(placement.width(1000) <= 800.0 + 1e-9);
        assert!(placement.height(500) <= 800.0 + 1e-9);
        assert!(
            placement.x >= -1e-9 && placement.y > 0.0,
            "должны остаться поля"
        );
    }

    #[test]
    fn center_keeps_the_original_size() {
        let placement = place((400, 300), (800, 600), WallpaperMode::Center);
        assert_eq!(placement.scale, 1.0);
        assert_eq!(placement.x, 200.0);
        assert_eq!(placement.y, 150.0);
    }

    #[test]
    fn the_image_is_always_centred() {
        for mode in [
            WallpaperMode::Fill,
            WallpaperMode::Fit,
            WallpaperMode::Center,
        ] {
            let placement = place((640, 480), (1920, 1080), mode);
            let centre_x = placement.x + placement.width(640) / 2.0;
            let centre_y = placement.y + placement.height(480) / 2.0;
            assert!((centre_x - 960.0).abs() < 1e-6, "{mode:?}");
            assert!((centre_y - 540.0).abs() < 1e-6, "{mode:?}");
        }
    }

    #[test]
    fn degenerate_sizes_do_not_divide_by_zero() {
        let placement = place((0, 0), (0, 0), WallpaperMode::Fill);
        assert!(placement.scale.is_finite());
    }

    #[test]
    fn a_missing_file_still_produces_a_filled_screen() {
        let wallpaper = prepare(
            Some(Path::new("/нет/таких/обоев.png")),
            WallpaperMode::Fill,
            (64, 32),
            Color::from_hex("#112233").unwrap(),
        );
        assert_eq!(wallpaper.width, 64);
        assert_eq!(wallpaper.height, 32);
    }

    #[test]
    fn solid_canvas_has_the_requested_colour_everywhere() {
        let canvas = solid_canvas(3, 2, Color::from_hex("#ff8000").unwrap());
        assert_eq!(canvas.len(), 3 * 2 * 4);
        for pixel in canvas.chunks(4) {
            assert_eq!(pixel, [255, 128, 0, 255]);
        }
    }

    #[test]
    fn drawing_respects_the_screen_bounds() {
        // Изображение крупнее экрана не должно выйти за пределы холста.
        let mut canvas = solid_canvas(4, 4, Color::BLACK);
        let image = image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 255, 255, 255]));
        draw_image(&mut canvas, 4, 4, &image, WallpaperMode::Center);
        assert!(canvas.iter().all(|byte| *byte == 255));
    }
}
