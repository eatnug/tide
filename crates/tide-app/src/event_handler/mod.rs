mod click;
pub(crate) mod ime;
mod keyboard;
mod mouse;
mod scroll;
mod search;

use winit::event::WindowEvent;

use crate::App;

impl App {
    pub(crate) fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                let session = crate::session::Session::from_app(self);
                crate::session::save_session(&session);
                crate::session::delete_running_marker();
                std::process::exit(0);
            }
            WindowEvent::Resized(new_size) => {
                self.window_size = new_size;
                self.reconfigure_surface();
                self.compute_layout();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(ime) => {
                self.handle_ime(ime);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(event);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_input(state, button);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta);
            }
            // RedrawRequested is handled directly in window_event() with early return
            // to avoid the unconditional `needs_redraw = true` at the end.
            _ => {}
        }
    }
}
