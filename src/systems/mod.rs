//! Logic-thread systems and observers registered in `main`: [`setup`] inserts the shared
//! `AppState` handles once at engine start, [`file_dialogs`] bridges the async native file
//! dialogs into the logic thread, and [`window_resize`] keeps the render target in sync with
//! the window size.

pub mod file_dialogs;
pub mod setup;
pub mod window_resize;
