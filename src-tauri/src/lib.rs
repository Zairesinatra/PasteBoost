use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, sync::Mutex, thread, time::Duration};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

struct Database(Mutex<Connection>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardItem {
    id: i64,
    content: String,
    item_type: String,
    is_favorite: bool,
    created_at: String,
    used_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snippet {
    id: i64,
    title: String,
    content: String,
    category: String,
    created_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    listening_enabled: bool,
    autostart_enabled: bool,
    protect_sensitive: bool,
    hotkey: String,
    max_items: i64,
    theme: String,
    language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            listening_enabled: true,
            autostart_enabled: false,
            protect_sensitive: true,
            hotkey: "Ctrl+Shift+V".to_string(),
            max_items: 500,
            theme: "light".to_string(),
            language: "zh-CN".to_string(),
        }
    }
}

fn valid_hotkey(hotkey: &str) -> bool {
    let tokens: Vec<&str> = hotkey
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.len() < 2 {
        return false;
    }
    tokens[..tokens.len() - 1].iter().all(|token| {
        ["ctrl", "control", "shift", "alt", "super", "meta"]
            .contains(&token.to_lowercase().as_str())
    }) && tokens
        .last()
        .is_some_and(|key| key.len() == 1 && key.chars().all(|char| char.is_ascii_alphanumeric()))
}

fn database(app: &AppHandle) -> Result<Connection, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let connection =
        Connection::open(directory.join("pasteboost.db")).map_err(|error| error.to_string())?;
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS clipboard_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL UNIQUE,
                item_type TEXT NOT NULL,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_used_at TEXT,
                used_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS snippets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT '常用',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn infer_type(content: &str) -> &'static str {
    let trimmed = content.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        "json"
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        "url"
    } else if trimmed.contains('@') && !trimmed.contains(char::is_whitespace) {
        "email"
    } else if [
        "select ",
        "const ",
        "function ",
        "import ",
        "insert ",
        "update ",
    ]
    .iter()
    .any(|token| trimmed.to_lowercase().contains(token))
    {
        "code"
    } else {
        "text"
    }
}

fn appears_sensitive(content: &str) -> bool {
    let lowercase = content.to_lowercase();
    let contains_secret_label = ["password", "passwd", "pwd=", "token=", "secret=", "验证码"]
        .iter()
        .any(|needle| lowercase.contains(needle));
    let looks_like_otp =
        content.trim().len() == 6 && content.trim().chars().all(|char| char.is_ascii_digit());
    contains_secret_label || looks_like_otp
}

fn settings_from_connection(connection: &Connection) -> Settings {
    let stored = connection
        .query_row("SELECT value FROM settings WHERE key = 'app'", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten();
    let mut settings: Settings = stored
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default();
    if !valid_hotkey(&settings.hotkey) {
        settings.hotkey = Settings::default().hotkey;
    }
    settings
}

fn insert_text(app: &AppHandle, content: String) -> Result<bool, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    let settings = settings_from_connection(&connection);
    if settings.protect_sensitive && appears_sensitive(trimmed) {
        return Ok(false);
    }
    let changed = connection
        .execute(
            "
            INSERT INTO clipboard_items (content, item_type) VALUES (?1, ?2)
            ON CONFLICT(content) DO UPDATE SET created_at = datetime('now')
            ",
            params![trimmed, infer_type(trimmed)],
        )
        .map_err(|error| error.to_string())?
        > 0;
    connection
        .execute(
            "
            DELETE FROM clipboard_items
            WHERE is_favorite = 0 AND id NOT IN (
                SELECT id FROM clipboard_items ORDER BY created_at DESC LIMIT ?1
            )
            ",
            [settings.max_items],
        )
        .map_err(|error| error.to_string())?;
    Ok(changed)
}

#[tauri::command]
fn list_items(app: AppHandle, query: String) -> Result<Vec<ClipboardItem>, String> {
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    let needle = format!("%{}%", query);
    let mut statement = connection
        .prepare(
            "
            SELECT id, content, item_type, is_favorite, created_at || 'Z', used_count
            FROM clipboard_items
            WHERE content LIKE ?1
            ORDER BY is_favorite DESC, datetime(created_at) DESC
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([needle], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content: row.get(1)?,
                item_type: row.get(2)?,
                is_favorite: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
                used_count: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn capture_text(app: AppHandle, content: String) -> Result<(), String> {
    if insert_text(&app, content)? {
        app.emit("clipboard-updated", ())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn toggle_favorite(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<Database>();
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute(
            "UPDATE clipboard_items SET is_favorite = CASE is_favorite WHEN 0 THEN 1 ELSE 0 END WHERE id = ?1",
            [id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_item(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<Database>();
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute("DELETE FROM clipboard_items WHERE id = ?1", [id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_items(app: AppHandle, ids: Vec<i64>) -> Result<(), String> {
    let state = app.state::<Database>();
    let mut connection = state.0.lock().map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for id in ids {
        transaction
            .execute("DELETE FROM clipboard_items WHERE id = ?1", [id])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn clear_unpinned(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Database>();
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute("DELETE FROM clipboard_items WHERE is_favorite = 0", [])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_snippets(app: AppHandle) -> Result<Vec<Snippet>, String> {
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare("SELECT id, title, content, category, created_at || 'Z' FROM snippets ORDER BY datetime(created_at) DESC")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Snippet {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_snippet(
    app: AppHandle,
    title: String,
    content: String,
    category: String,
) -> Result<(), String> {
    let state = app.state::<Database>();
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute(
            "INSERT INTO snippets (title, content, category) VALUES (?1, ?2, ?3)",
            params![title, content, category],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_snippet(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<Database>();
    state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute("DELETE FROM snippets WHERE id = ?1", [id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    Ok(settings_from_connection(&connection))
}

fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_settings(app: AppHandle, mut settings: Settings) -> Result<(), String> {
    let shortcut = settings.hotkey.trim().to_string();
    if !valid_hotkey(&shortcut) {
        return Err("快捷键必须包含至少一个修饰键，例如 Ctrl+Shift+V".to_string());
    }
    settings.hotkey = shortcut.clone();
    let previous = {
        let state = app.state::<Database>();
        let connection = state.0.lock().map_err(|error| error.to_string())?;
        settings_from_connection(&connection)
    };

    let autostart_was_enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?;
    let autostart_changed = autostart_was_enabled != settings.autostart_enabled;
    if autostart_changed {
        set_autostart(&app, settings.autostart_enabled)?;
    }

    let hotkey_changed = previous.hotkey != shortcut;
    if hotkey_changed {
        if let Err(error) = app.global_shortcut().unregister_all() {
            if autostart_changed {
                let _ = set_autostart(&app, autostart_was_enabled);
            }
            return Err(error.to_string());
        }
        if let Err(error) = app.global_shortcut().register(shortcut.as_str()) {
            let _ = app.global_shortcut().register(previous.hotkey.as_str());
            if autostart_changed {
                let _ = set_autostart(&app, autostart_was_enabled);
            }
            return Err(error.to_string());
        }
    }

    let state = app.state::<Database>();
    let value = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    if let Err(error) = state
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            [value],
        )
    {
        if hotkey_changed {
            let _ = app.global_shortcut().unregister_all();
            let _ = app.global_shortcut().register(previous.hotkey.as_str());
        }
        if autostart_changed {
            let _ = set_autostart(&app, autostart_was_enabled);
        }
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
fn frontend_ready() {
    println!("[PasteBoost] React frontend mounted");
}

#[cfg(windows)]
fn send_paste_keys() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };
    fn input(key: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    let events = [
        input(VK_CONTROL, 0),
        input(VK_V, 0),
        input(VK_V, KEYEVENTF_KEYUP),
        input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

#[cfg(not(windows))]
fn send_paste_keys() {}

#[tauri::command]
fn paste_text(app: AppHandle, content: String) -> Result<(), String> {
    app.clipboard()
        .write_text(content.clone())
        .map_err(|error| error.to_string())?;
    {
        let state = app.state::<Database>();
        state
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .execute(
                "UPDATE clipboard_items SET used_count = used_count + 1, last_used_at = datetime('now') WHERE content = ?1",
                [content],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|error| error.to_string())?;
    }
    thread::sleep(Duration::from_millis(100));
    send_paste_keys();
    Ok(())
}

fn show_window(window: Option<WebviewWindow>) {
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn start_clipboard_listener(app: AppHandle) {
    thread::spawn(move || {
        let mut last_content = String::new();
        loop {
            thread::sleep(Duration::from_millis(650));
            let listening = {
                let state = app.state::<Database>();
                let connection = match state.0.lock() {
                    Ok(connection) => connection,
                    Err(_) => continue,
                };
                settings_from_connection(&connection).listening_enabled
            };
            if !listening {
                continue;
            }
            if let Ok(content) = app.clipboard().read_text() {
                if content != last_content {
                    last_content = content.clone();
                    if insert_text(&app, content).unwrap_or(false) {
                        let _ = app.emit("clipboard-updated", ());
                    }
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        show_window(app.get_webview_window("main"));
                        let _ = app.emit("panel-opened", ());
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_page_load(|_webview, payload| {
            println!("[PasteBoost] WebView page loaded: {}", payload.url());
        })
        .setup(|app| {
            let connection = database(app.handle()).map_err(std::io::Error::other)?;
            let settings = settings_from_connection(&connection);
            let settings_value = serde_json::to_string(&settings).map_err(std::io::Error::other)?;
            connection
                .execute(
                    "INSERT INTO settings (key, value) VALUES ('app', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                    [settings_value],
                )
                .map_err(std::io::Error::other)?;
            app.manage(Database(Mutex::new(connection)));
            app.global_shortcut()
                .register(settings.hotkey.as_str())
                .map_err(std::io::Error::other)?;

            let show = MenuItem::with_id(app, "show", "显示 PasteBoost", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "暂停 / 恢复监听", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &pause, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("application icon").clone())
                .tooltip("PasteBoost - 轻量剪贴助手")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_window(app.get_webview_window("main")),
                    "pause" => {
                        let state = app.state::<Database>();
                        if let Ok(connection) = state.0.lock() {
                            let mut settings = settings_from_connection(&connection);
                            settings.listening_enabled = !settings.listening_enabled;
                            if let Ok(value) = serde_json::to_string(&settings) {
                                let _ = connection.execute(
                                    "INSERT INTO settings (key, value) VALUES ('app', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                                    [value],
                                );
                                let _ = app.emit("settings-updated", settings);
                            }
                        };
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, _event| {
                    show_window(tray.app_handle().get_webview_window("main"));
                })
                .build(app)?;
            start_clipboard_listener(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_items,
            capture_text,
            toggle_favorite,
            delete_item,
            delete_items,
            clear_unpinned,
            list_snippets,
            save_snippet,
            delete_snippet,
            get_settings,
            save_settings,
            frontend_ready,
            paste_text
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PasteBoost");
}
