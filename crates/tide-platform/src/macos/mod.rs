//! macOS native platform backend using objc2.

mod app;
pub(crate) mod ime_proxy;
mod view;
mod window;

pub use app::MacosApp;
pub use window::MacosWindow;
