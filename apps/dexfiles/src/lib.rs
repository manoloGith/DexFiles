//! DexFiles: a file manager for Samsung DeX and Android's desktop windowing (and any desktop),
//! in Rust with egui, on craft-android's Android layer like the other Dex apps (DexPdfCraft).
//!
//! The Android app (`android/`) is craft-android's `CraftActivity` (GameActivity), which loads
//! this library and calls `android_main`; egui draws the window with wgpu (Vulkan or GLES), or
//! with OpenGL ES through glow when wgpu can't start. `src/main.rs` runs the same window on a
//! desktop, to iterate without a device.

pub mod apps;
pub mod archive;
pub mod dirsize;
pub mod fileinfo;
pub mod i18n;
pub mod media;
pub mod ops;
pub mod platform;
pub mod preview;
pub mod rename;
pub mod search;
pub mod settings;
pub mod tags;
pub mod trash;
pub mod ui;
pub mod vfs;

/// What to start with.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// Folders or files to open (a file opens its folder).
    pub open: Vec<std::path::PathBuf>,
    /// Where eframe keeps the settings (`app.ron`).
    pub persistence_path: Option<std::path::PathBuf>,
}

/// The window's options, shared by Android and the desktop binary.
/// The app's icon (the launcher's, as a 256 px PNG): the window's on a desktop, and About's.
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn window_icon() -> Option<std::sync::Arc<egui::IconData>> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    Some(std::sync::Arc::new(egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    }))
}

pub fn native_options(renderer: eframe::Renderer, launch: &Launch) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default();
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::NativeOptions {
        renderer,
        viewport: viewport
            .with_title("DexFiles")
            .with_app_id("dexfiles")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([360.0, 400.0])
            .with_drag_and_drop(true),
        persistence_path: launch.persistence_path.clone(),
        ..Default::default()
    }
}

/// Run the window with wgpu, then with glow if wgpu can't start (old or unusual GPU drivers).
pub fn run(mut options: impl FnMut(eframe::Renderer) -> eframe::NativeOptions, launch: Launch) {
    let mut last_error = None;
    for renderer in [eframe::Renderer::Wgpu, eframe::Renderer::Glow] {
        let launch = launch.clone();
        let result = eframe::run_native(
            "DexFiles",
            options(renderer),
            Box::new(move |cc| Ok(create(cc, launch))),
        );
        match result {
            Ok(()) => return,
            Err(e) => {
                log::error!("dexfiles: the {renderer} renderer could not start: {e}");
                #[cfg(target_os = "android")]
                craft_android::android::diag(&format!(
                    "the {renderer} renderer could not start: {e}"
                ));
                last_error = Some(e);
            }
        }
    }
    if let Some(e) = last_error {
        log::error!("dexfiles: no renderer could start: {e}");
    }
}

fn create(cc: &eframe::CreationContext<'_>, launch: Launch) -> Box<dyn eframe::App> {
    #[cfg(target_os = "android")]
    craft_android::android::events::set_context(cc.egui_ctx.clone());
    let app = ui::App::new(cc, launch.open);
    // craft-android's shell: the activity's mouse, keyboard and insets, Back as the close button,
    // the window title for DeX's taskbar. Nothing to autosave: every operation is already on disk.
    Box::new(
        craft_android::Shell::new(app)
            .title(ui::App::title)
            .on_focus_lost(ui::App::persist),
    )
}

#[cfg(target_os = "android")]
pub(crate) mod android;
