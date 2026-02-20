//! macOS native platform backend using objc2.

mod app;
mod view;
pub mod webview;
mod window;

pub use app::MacosApp;
pub use window::MacosWindow;
