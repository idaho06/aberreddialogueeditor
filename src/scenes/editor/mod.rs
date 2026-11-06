mod actions;
mod context_menu;
mod gui;
mod lifecycle;
mod menu;
mod modals;
mod nodes;
mod shortcuts;
mod tabs;

pub use gui::gui;
pub use lifecycle::enter;

use aberredengine::prelude::Vec2;
use aberredengine::prelude::imgui::Ui;

/// The window size in screen pixels as a `Vec2` (== the render size), read by the producers of
/// the viewport-carrying actions (`FrameAll`, `Relayout`, `AddNode`).
pub(super) fn display_viewport(ui: &Ui) -> Vec2 {
    let size = ui.io().display_size;
    Vec2::new(size[0], size[1])
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Test scaffolding shared by the editor and file-dialog test modules.

    use std::path::PathBuf;

    /// A path inside `tests/fixtures/`.
    pub fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// A unique temp directory per test, removed on drop. `scope` keeps same-named directories
    /// of different test modules from colliding inside one test process.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(scope: &str, name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "aberred-dialogue-{scope}-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create the temp dir");
            Self(dir)
        }

        pub fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
