mod overlay;
mod scheduler;
mod settings;

use std::sync::{Arc, Mutex};

use scheduler::{Cmd, Scheduler, StatusPayload};
use settings::Settings;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

pub struct AppShared {
    pub settings: Arc<Mutex<Settings>>,
    pub scheduler: Scheduler,
}

fn load_settings(app: &AppHandle) -> (Settings, bool) {
    match app.store(STORE_FILE) {
        Ok(store) => match store.get(STORE_KEY) {
            Some(value) => match serde_json::from_value::<Settings>(value) {
                Ok(settings) => (settings.sanitized(), true),
                Err(_) => (Settings::default(), true),
            },
            None => (Settings::default(), false),
        },
        Err(_) => (Settings::default(), false),
    }
}

fn persist_settings(app: &AppHandle, settings: &Settings) {
    if let Ok(store) = app.store(STORE_FILE) {
        if let Ok(value) = serde_json::to_value(settings) {
            let _ = store.set(STORE_KEY, value);
            let _ = store.save();
        }
    }
}

#[tauri::command]
fn get_settings(shared: tauri::State<'_, AppShared>) -> Settings {
    shared.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    shared: tauri::State<'_, AppShared>,
    new_settings: Settings,
) -> Result<Settings, String> {
    let sanitized = new_settings.sanitized();
    let previous = shared.settings.lock().unwrap().clone();

    if sanitized.autostart != previous.autostart {
        let autolaunch = app.autolaunch();
        let result = if sanitized.autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(err) = result {
            return Err(format!("could not update autostart: {err}"));
        }
    }

    persist_settings(&app, &sanitized);
    *shared.settings.lock().unwrap() = sanitized.clone();
    shared.scheduler.send(Cmd::SettingsChanged);
    Ok(sanitized)
}

#[tauri::command]
fn get_status(shared: tauri::State<'_, AppShared>) -> StatusPayload {
    shared.scheduler.status.lock().unwrap().clone()
}

#[tauri::command]
fn start_break_now(shared: tauri::State<'_, AppShared>) {
    shared.scheduler.send(Cmd::StartBreakNow);
}

#[tauri::command]
fn skip_break(shared: tauri::State<'_, AppShared>) {
    shared.scheduler.send(Cmd::SkipBreak);
}

#[tauri::command]
fn snooze_break(shared: tauri::State<'_, AppShared>) {
    shared.scheduler.send(Cmd::Snooze);
}

#[tauri::command]
fn pause_reminders(shared: tauri::State<'_, AppShared>, minutes: Option<u32>) {
    shared.scheduler.send(Cmd::Pause(minutes));
}

#[tauri::command]
fn resume_reminders(shared: tauri::State<'_, AppShared>) {
    shared.scheduler.send(Cmd::Resume);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn handle_tray_menu(app: &AppHandle, id: &str) {
    let shared = app.state::<AppShared>();
    match id {
        "take_break" => shared.scheduler.send(Cmd::StartBreakNow),
        "open_settings" => show_settings_window(app),
        "pause_15" => shared.scheduler.send(Cmd::Pause(Some(15))),
        "pause_30" => shared.scheduler.send(Cmd::Pause(Some(30))),
        "pause_60" => shared.scheduler.send(Cmd::Pause(Some(60))),
        "pause_inf" => shared.scheduler.send(Cmd::Pause(None)),
        "resume" => shared.scheduler.send(Cmd::Resume),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let pause_menu = SubmenuBuilder::new(app, "Pause reminders")
        .item(&MenuItemBuilder::with_id("pause_15", "For 15 minutes").build(app)?)
        .item(&MenuItemBuilder::with_id("pause_30", "For 30 minutes").build(app)?)
        .item(&MenuItemBuilder::with_id("pause_60", "For 1 hour").build(app)?)
        .item(&MenuItemBuilder::with_id("pause_inf", "Until I resume").build(app)?)
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("take_break", "Take a break now").build(app)?)
        .item(&MenuItemBuilder::with_id("open_settings", "Settings...").build(app)?)
        .separator()
        .item(&pause_menu)
        .item(&MenuItemBuilder::with_id("resume", "Resume reminders").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("quit", "Quit BlinkWell").build(app)?)
        .build()?;

    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("BlinkWell - 20-20-20 eye care")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_tray_menu(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_settings_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_settings_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_status,
            start_break_now,
            skip_break,
            snooze_break,
            pause_reminders,
            resume_reminders,
            quit_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let (settings, first_run) = load_settings(&handle);

            #[cfg(not(debug_assertions))]
            if settings.autostart {
                let _ = handle.autolaunch().enable();
            }

            let settings = Arc::new(Mutex::new(settings));
            let scheduler = scheduler::spawn(handle.clone(), Arc::clone(&settings));
            app.manage(AppShared {
                settings,
                scheduler,
            });

            build_tray(&handle)?;

            let hidden = std::env::args().any(|arg| arg == "--hidden");
            if first_run || !hidden {
                show_settings_window(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running BlinkWell");
}
