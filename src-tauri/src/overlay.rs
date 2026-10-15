use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const MAX_MONITORS: usize = 16;

pub fn show(app: &AppHandle) {
    close(app);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = create_windows(&handle) {
            eprintln!("BlinkWell: failed to create overlay windows: {err}");
        }
    });
}

fn create_windows(app: &AppHandle) -> tauri::Result<()> {
    let monitors = app
        .get_webview_window("main")
        .and_then(|window| window.available_monitors().ok())
        .unwrap_or_default();

    if monitors.is_empty() {
        let fallback =
            WebviewWindowBuilder::new(app, "overlay-0", WebviewUrl::App("overlay.html".into()))
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
                .focused(true)
                .build()?;
        let _ = fallback.set_always_on_top(true);
        return Ok(());
    }

    for (index, monitor) in monitors.iter().enumerate() {
        let label = format!("overlay-{index}");
        let scale = monitor.scale_factor();
        let position = monitor.position();
        let size = monitor.size();

        let window =
            WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
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
                .position(position.x as f64 / scale, position.y as f64 / scale)
                .inner_size(size.width as f64 / scale, size.height as f64 / scale)
                .focused(index == 0)
                .build()?;
        let _ = window.set_always_on_top(true);
    }

    Ok(())
}

pub fn close(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        for index in 0..MAX_MONITORS {
            if let Some(window) = handle.get_webview_window(&format!("overlay-{index}")) {
                let _ = window.destroy();
            }
        }
    });
}

pub fn any_open(app: &AppHandle) -> bool {
    (0..MAX_MONITORS).any(|index| {
        app.get_webview_window(&format!("overlay-{index}")).is_some()
    })
}
