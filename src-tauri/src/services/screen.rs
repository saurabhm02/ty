//! Taking screenshots. On macOS this uses the built-in `screencapture` tool and
//! saves PNG files in the app's cache folder.

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use tauri::{AppHandle, Manager, Runtime};

    use crate::models::screen::ScreenShot;
    use crate::services::{panel, window};
    use crate::utils::images;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    /// How long macOS needs to really remove a hidden window from the screen.
    const HIDE_SETTLE_MS: u64 = 250;

    enum Mode {
        /// The whole main display
        Display,
        Region,
    }

    /// Without the Screen Recording permission macOS silently gives a screenshot
    /// with no app windows in it. So check first, and trigger the system prompt.
    fn ensure_screen_permission() -> Result<(), String> {
        if unsafe { CGPreflightScreenCaptureAccess() } {
            return Ok(());
        }
        unsafe { CGRequestScreenCaptureAccess() };
        Err("Screen Recording permission is needed. Allow it in System Settings → Privacy & Security → Screen & System Audio Recording, then restart Chotu.".to_string())
    }

    /// The whole main display.
    pub async fn capture_display<R: Runtime>(app: &AppHandle<R>) -> Result<ScreenShot, String> {
        capture(app, Mode::Display)
            .await?
            .ok_or_else(|| "the screenshot file was not created".to_string())
    }

    /// A box the user drags. `None` means they pressed Esc.
    pub async fn capture_region<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Result<Option<ScreenShot>, String> {
        capture(app, Mode::Region).await
    }

    async fn capture<R: Runtime>(
        app: &AppHandle<R>,
        mode: Mode,
    ) -> Result<Option<ScreenShot>, String> {
        ensure_screen_permission()?;

        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = screen_dir(app)?.join(format!("{id}.png"));

        take_screenshot(app, &path, &mode).await?;

        // Pressing Esc while dragging ends `screencapture` without making a file.
        if !path.exists() {
            return match mode {
                Mode::Region => Ok(None),
                Mode::Display => Err("the screenshot file was not created".to_string()),
            };
        }

        let (width, height) = png_size(&path)?;
        log::info!("screen captured: {} ({width}x{height})", path.display());

        Ok(Some(ScreenShot {
            id,
            image_path: path.to_string_lossy().into_owned(),
            width,
            height,
        }))
    }

    /// The whole screen is photographed without Chotu and without hiding it. If that fails, or
    /// the user drags a box, Chotu hides for a moment instead.
    async fn take_screenshot<R: Runtime>(
        app: &AppHandle<R>,
        path: &Path,
        mode: &Mode,
    ) -> Result<(), String> {
        if matches!(mode, Mode::Display) {
            match photograph_below_chotu(app, path).await {
                Ok(()) => return Ok(()),
                Err(e) => log::warn!("could not photograph below chotu ({e}), hiding it instead!"),
            }
        }

        photograph_with_hiding_chotu(app, path, mode).await
    }

    /// Hides Chotu so it isn't in the picture, and always brings it back after.
    async fn photograph_with_hiding_chotu<R: Runtime>(
        app: &AppHandle<R>,
        path: &Path,
        mode: &Mode,
    ) -> Result<(), String> {
        panel::hide_main_window(app);
        tokio::time::sleep(Duration::from_millis(HIDE_SETTLE_MS)).await;

        let finished = run_screencapture(path, mode).await;
        panel::show_main_window(app);
        finished
    }

    /// Asks macOS for every window below Chotu's, so Chotu stays where it is.
    async fn photograph_below_chotu<R: Runtime>(
        app: &AppHandle<R>,
        path: &Path,
    ) -> Result<(), String> {
        let window_id = panel::main_window_number(app)
            .await
            .ok_or("could not find chotu's main window!")?;

        let (width, height, pixels) =
            panel::on_main_thread(app, move || window::capture_below_chotu(window_id)).await??;
        images::save_rgba_png(width, height, pixels, path)
    }

    fn screen_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("screens");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    /// `-x` = no camera sound, `-t png` = PNG file, `-i` = let the user drag a box.
    async fn run_screencapture(path: &Path, mode: &Mode) -> Result<(), String> {
        let mut command = tokio::process::Command::new("screencapture");
        if matches!(mode, Mode::Region) {
            command.arg("-i");
        }
        let status = command
            .args(["-x", "-t", "png"])
            .arg(path)
            .status()
            .await
            .map_err(|e| format!("could not run screencapture: {e}"))?;

        if !status.success() {
            return Err(format!("screencapture failed ({status})"));
        }
        Ok(())
    }

    /// Width and height straight from the PNG header (two big-endian numbers at
    /// bytes 16..24), so we don't need an image library just for this.
    fn png_size(path: &Path) -> Result<(u32, u32), String> {
        use std::io::Read;

        let mut header = [0u8; 24];
        std::fs::File::open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|e| format!("could not read the screenshot: {e}"))?;

        if &header[1..4] != b"PNG" {
            return Err("the screenshot is not a PNG file".to_string());
        }
        let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
        let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
        Ok((width, height))
    }
}

#[cfg(target_os = "macos")]
pub use mac::{capture_display, capture_region};

#[cfg(not(target_os = "macos"))]
pub async fn capture_display<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
) -> Result<crate::models::screen::ScreenShot, String> {
    Err("Screen capture only works on macOS for now.".to_string())
}

#[cfg(not(target_os = "macos"))]
pub async fn capture_region<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
) -> Result<Option<crate::models::screen::ScreenShot>, String> {
    Err("Screen capture only works on macOS for now.".to_string())
}
