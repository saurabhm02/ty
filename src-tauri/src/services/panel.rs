use tauri::{AppHandle, Manager, Runtime};

#[cfg(target_os = "macos")]
mod mac {
    use tauri::{Manager, Runtime};
    use tauri_nspanel::{
        tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt,
    };

    tauri_panel! {
        panel!(FloatingPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }

    // The panel class is generated for the real runtime (`Wry`) only.
    pub fn convert_main_window(app: &tauri::App) -> Result<(), String> {
        let window = app
            .get_webview_window("main")
            .ok_or("the main window does not exist")?;
        let panel = window
            .to_panel::<FloatingPanel>()
            .map_err(|e| e.to_string())?;

        panel.set_level(PanelLevel::Floating.value());
        // The window shadow draws a grey square around the small icon in mini mode.
        panel.set_has_shadow(false);
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .full_screen_auxiliary()
                .can_join_all_spaces()
                .into(),
        );
        // Showing the panel must not activate the app, or macOS leaves the full-screen Space.
        panel
            .add_style_mask(StyleMask::empty().nonactivating_panel().into())
            .map_err(|e| format!("{e:?}"))?;
        Ok(())
    }

    pub fn show<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
        match app.get_webview_panel("main") {
            Ok(panel) => {
                panel.show_and_make_key();
                true
            }
            Err(_) => false,
        }
    }

    pub fn hide<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
        match app.get_webview_panel("main") {
            Ok(panel) => {
                panel.hide();
                true
            }
            Err(_) => false,
        }
    }

    pub fn window_number<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<u32> {
        let window = app.get_webview_window("main")?;
        let pointer = window.ns_window().ok()?;
        let ns_window = pointer.cast::<AnyObject>();
        let number: isize = unsafe { msg_send![ns_window, windowNumber] };
        u32::try_from(number).ok().filter(|number| *number > 0)
    }
}

/// Call once from `setup`. A failure is logged and the app keeps its plain window.
pub fn init(app: &mut tauri::App) {
    #[cfg(target_os = "macos")]
    {
        // No Dock icon: an accessory app can appear over full-screen apps.
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        if let Err(e) = mac::convert_main_window(app) {
            log::error!("could not turn the window into a panel: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Shows the main window and gives it the keyboard.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || show_now(&handle));
}

/// Hides the main window.
pub fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || hide_now(&handle));
}

// AppKit crashes the app when a window is touched off the main thread, and the callers
// (the screenshot command, for one) run on worker threads. Both functions below must only
// run through `run_on_main_thread`.
fn show_now<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    if mac::show(app) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_now<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    if mac::hide(app) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[cfg(target_os = "macos")]
pub async fn main_window_number<R: Runtime>(app: &AppHandle<R>) -> Option<u32> {
    let handle = app.clone();
    on_main_thread(app, move || mac::window_number(&handle))
        .await
        .ok()
        .flatten()
}

pub async fn on_main_thread<R, T, F>(app: &AppHandle<R>, work: F) -> Result<T, String>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (done, wait) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = done.send(work());
    })
    .map_err(|e| e.to_string())?;
    wait.await.map_err(|e| e.to_string())
}
