//! Сборка кадра.
//!
//! Композитор рисует два вида слоёв: обои снизу и окна поверх них. Оба вида
//! приходится объединить в один тип — отрисовщику нужен однородный список
//! элементов, поэтому они складываются в перечисление.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{render_elements, Kind};
use smithay::backend::renderer::{ImportAll, ImportMem, Renderer};
use smithay::desktop::space::{space_render_elements, SpaceRenderElements};
use smithay::desktop::{Space, Window};
use smithay::output::Output;

use crate::cursor::CursorElement;
use crate::wallpaper::Wallpaper;

render_elements! {
    /// Слой кадра HypeDE.
    ///
    /// Тип элемента окна вынесен в параметр `E`: иначе проверка ограничений
    /// внутри макроса не сходится — таким же образом устроен пример anvil из
    /// самой smithay.
    pub HypeRenderElement<R, E> where R: ImportAll + ImportMem;
    Space = SpaceRenderElements<R, E>,
    Wallpaper = MemoryRenderBufferRenderElement<R>,
    Cursor = CursorElement<R>,
}

/// Кадр, каким его собирает HypeDE.
pub type Frame<R> = HypeRenderElement<R, WaylandSurfaceRenderElement<R>>;

/// Собирает список слоёв для одного монитора.
///
/// Порядок в списке — от переднего слоя к заднему: сначала указатель, затем
/// окна, и последними обои.
pub fn output_elements<R>(
    renderer: &mut R,
    space: &Space<Window>,
    output: &Output,
    wallpaper: Option<&Wallpaper>,
    cursor: Vec<CursorElement<R>>,
) -> Vec<Frame<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let mut elements: Vec<Frame<R>> = cursor.into_iter().map(HypeRenderElement::Cursor).collect();

    let windows: Vec<Frame<R>> = match space_render_elements(renderer, [space], output, 1.0) {
        Ok(windows) => windows.into_iter().map(HypeRenderElement::Space).collect(),
        Err(err) => {
            tracing::warn!("не удалось собрать окна кадра: {err}");
            Vec::new()
        }
    };
    elements.extend(windows);

    if let Some(wallpaper) = wallpaper {
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            &wallpaper.buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        ) {
            Ok(element) => elements.push(HypeRenderElement::Wallpaper(element)),
            Err(err) => tracing::warn!("обои не показаны: {err}"),
        }
    }

    elements
}
