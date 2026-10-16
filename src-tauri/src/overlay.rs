use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Upper bound on per-monitor windows, so a bad monitor enumeration can never
/// turn one break into a window storm.
const MAX_MONITORS: usize = 16;

const LABEL_PREFIX: &str = "overlay-";

/// Build one fullscreen overlay window per monitor. All window work happens on
/// the main thread, and failures are contained: one bad monitor, one failed
/// webview, or even a panic inside the windowing stack can't take the app
/// down or leave half the screens without an overlay.
pub fn show(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            destroy_overlays(&handle);
            create_windows(&handle)
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(err)) => log::error!("overlay windows failed: {err}"),
            Err(_) => log::error!("overlay creation panicked"),
        }
    });
}

/// Tear down every overlay window. Runs on the main thread; a panic in the
/// windowing stack is contained so a stuck window can't wedge the whole app.
pub fn close(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            destroy_overlays(&handle);
        }));
    });
}

/// Is any overlay window still up? Used by the scheduler watchdog.
pub fn any_open(app: &AppHandle) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with(LABEL_PREFIX))
}

/// Cheap fingerprint of the current monitor layout. The scheduler compares it
/// while a break is up: any change means displays were plugged, unplugged,
/// moved, or rescaled, and the overlay windows are rebuilt to match.
pub fn monitor_signature(app: &AppHandle) -> Option<String> {
    let window = app.get_webview_window("main")?;
    let monitors = window.available_monitors().ok()?;
    let mut parts = Vec::with_capacity(monitors.len().min(MAX_MONITORS));
    for monitor in monitors.iter().take(MAX_MONITORS) {
        let position = monitor.position();
        let size = monitor.size();
        parts.push(format!(
            "{}x{}@{},{}@{}",
            size.width,
            size.height,
            position.x,
            position.y,
            monitor.scale_factor(),
        ));
    }
    Some(parts.join("|"))
}

fn destroy_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
}

fn create_windows(app: &AppHandle) -> tauri::Result<()> {
    let monitors = app
        .get_webview_window("main")
        .and_then(|window| window.available_monitors().ok())
        .unwrap_or_default();

    if monitors.is_empty() {
        // No monitor information at all: better a default-size overlay on the
        // primary screen than none.
        build(app, "overlay-0", None, true)?;
        return Ok(());
    }

    let mut created = 0usize;
    for (index, monitor) in monitors.iter().take(MAX_MONITORS).enumerate() {
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();
        let geometry = Some((
            position.x as f64 / scale,
            position.y as f64 / scale,
            size.width as f64 / scale,
            size.height as f64 / scale,
        ));
        let label = format!("overlay-{index}");
        match build(app, &label, geometry, index == 0) {
            Ok(()) => created += 1,
            Err(err) => log::error!("overlay on monitor {index} failed: {err}"),
        }
    }

    if created == 0 {
        // Every per-monitor window failed; fall back to a single overlay.
        build(app, "overlay-0", None, true)?;
    }

    Ok(())
}

/// `geometry` is (x, y, width, height) in logical units.
fn build(
    app: &AppHandle,
    label: &str,
    geometry: Option<(f64, f64, f64, f64)>,
    focus: bool,
) -> tauri::Result<()> {
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("overlay.html".into()))
        .title("Eye Break")
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible_on_all_workspaces(true)
        .focused(focus);

    if let Some((x, y, width, height)) = geometry {
        builder = builder.position(x, y).inner_size(width, height);
    }

    let window = builder.build()?;
    let _ = window.set_always_on_top(true);
    Ok(())
}
