//! Обработчики протоколов Wayland.

mod compositor;
mod xdg_shell;

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::{delegate_data_device, delegate_output, delegate_seat};

use crate::state::HypeState;

impl SeatHandler for HypeState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        image: smithay::input::pointer::CursorImageStatus,
    ) {
        // Клиент вправе подменить указатель своим рисунком — например,
        // крестиком над холстом. Композитор запоминает просьбу и учитывает её
        // при сборке следующего кадра.
        self.cursor_status = image;
        self.redraw_needed = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // Буфер обмена принадлежит тому, кто в фокусе: иначе вставка возьмёт
        // текст не из того окна.
        let dh = &self.display_handle;
        let client = focused.and_then(|surface| dh.get_client(surface.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(HypeState);

impl SelectionHandler for HypeState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for HypeState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for HypeState {}
impl ServerDndGrabHandler for HypeState {}

delegate_data_device!(HypeState);

impl OutputHandler for HypeState {}
delegate_output!(HypeState);
