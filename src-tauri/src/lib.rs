use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{DynamicImage, ImageFormat, RgbaImage};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::Cursor,
    ptr::null_mut,
    sync::Mutex,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const DEFAULT_SCREENSHOT_HOTKEY: &str = "Super+F1";

struct Database(Mutex<Connection>);

struct ScreenshotStore(Mutex<Option<ScreenshotCapture>>);

struct ScreenshotCapture {
    image: Image<'static>,
    bounds: ScreenshotBounds,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardItem {
    id: i64,
    content: String,
    item_type: String,
    image_data: Option<String>,
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

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotSession {
    bounds: ScreenshotBounds,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotSelection {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_width: f64,
    viewport_height: f64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    listening_enabled: bool,
    autostart_enabled: bool,
    protect_sensitive: bool,
    hotkey: String,
    screenshot_hotkey: String,
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
            screenshot_hotkey: DEFAULT_SCREENSHOT_HOTKEY.to_string(),
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
    }) && tokens.last().is_some_and(|key| {
        (key.len() == 1 && key.chars().all(|char| char.is_ascii_alphanumeric()))
            || matches!(
                key.to_lowercase().as_str(),
                "f1" | "f2"
                    | "f3"
                    | "f4"
                    | "f5"
                    | "f6"
                    | "f7"
                    | "f8"
                    | "f9"
                    | "f10"
                    | "f11"
                    | "f12"
            )
    })
}

fn normalize_hotkey(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| match token.to_lowercase().as_str() {
            "win" | "windows" | "meta" => "Super".to_string(),
            "control" => "Ctrl".to_string(),
            _ => token.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
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
                image_data TEXT,
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
    let has_image_data = connection
        .prepare("PRAGMA table_info(clipboard_items)")
        .and_then(|mut statement| {
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
            Ok(columns
                .filter_map(Result::ok)
                .any(|column| column == "image_data"))
        })
        .map_err(|error| error.to_string())?;
    if !has_image_data {
        connection
            .execute("ALTER TABLE clipboard_items ADD COLUMN image_data TEXT", [])
            .map_err(|error| error.to_string())?;
    }
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
    settings.hotkey = normalize_hotkey(&settings.hotkey);
    settings.screenshot_hotkey = normalize_hotkey(&settings.screenshot_hotkey);
    if !valid_hotkey(&settings.hotkey) {
        settings.hotkey = Settings::default().hotkey;
    }
    if !valid_hotkey(&settings.screenshot_hotkey) {
        settings.screenshot_hotkey = Settings::default().screenshot_hotkey;
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
            INSERT INTO clipboard_items (content, item_type, image_data) VALUES (?1, ?2, NULL)
            ON CONFLICT(content) DO UPDATE SET created_at = datetime('now'), image_data = NULL
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

fn png_data_url(image: &Image<'_>) -> Result<String, String> {
    Ok(png_data_url_from_bytes(&png_bytes(image)?))
}

fn png_data_url_from_bytes(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", BASE64.encode(bytes))
}

fn png_bytes(image: &Image<'_>) -> Result<Vec<u8>, String> {
    let rgba = RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .ok_or_else(|| "无法读取图片像素".to_string())?;
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png.into_inner())
}

fn save_png_to_pictures(app: &AppHandle, png: &[u8]) -> Result<(), String> {
    let mut directory = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|error| error.to_string())?;
    directory.push("PasteBoost");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    directory.push(format!("pasteboost-screenshot-{timestamp}.png"));
    fs::write(directory, png).map_err(|error| error.to_string())
}

fn image_signature(image: &Image<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    image.width().hash(&mut hasher);
    image.height().hash(&mut hasher);
    image.rgba().hash(&mut hasher);
    hasher.finish()
}

fn image_content(image: &Image<'_>) -> String {
    format!(
        "[图片] {} x {} #{:016x}",
        image.width(),
        image.height(),
        image_signature(image)
    )
}

#[cfg(windows)]
fn clipboard_handle_from_bytes(bytes: &[u8]) -> Result<windows_sys::Win32::Foundation::HGLOBAL, String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return Err("无法分配剪贴板图片内存".to_string());
        }
        let memory = GlobalLock(handle);
        if memory.is_null() {
            GlobalFree(handle);
            return Err("无法锁定剪贴板图片内存".to_string());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
        GlobalUnlock(handle);
        Ok(handle)
    }
}

#[cfg(windows)]
fn dib_bytes(image: &Image<'_>) -> Result<Vec<u8>, String> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    if width == 0 || height == 0 {
        return Err("图片尺寸无效".to_string());
    }

    let row_stride = ((width * 3 + 3) / 4) * 4;
    let pixel_bytes = row_stride
        .checked_mul(height)
        .ok_or_else(|| "图片过大".to_string())?;
    let header_bytes = 40usize;
    let total_bytes = header_bytes
        .checked_add(pixel_bytes)
        .ok_or_else(|| "图片过大".to_string())?;
    let mut dib = vec![0u8; total_bytes];

    dib[0..4].copy_from_slice(&(header_bytes as u32).to_le_bytes());
    dib[4..8].copy_from_slice(&(image.width() as i32).to_le_bytes());
    dib[8..12].copy_from_slice(&(image.height() as i32).to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&24u16.to_le_bytes());
    dib[20..24].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());

    let rgba = image.rgba();
    for y in 0..height {
        let source_y = height - 1 - y;
        let source_row = source_y * width * 4;
        let target_row = header_bytes + y * row_stride;
        for x in 0..width {
            let source = source_row + x * 4;
            let target = target_row + x * 3;
            dib[target] = rgba[source + 2];
            dib[target + 1] = rgba[source + 1];
            dib[target + 2] = rgba[source];
        }
    }

    Ok(dib)
}

#[cfg(windows)]
fn write_image_to_clipboard(_app: &AppHandle, image: &Image<'_>, png: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::Foundation::GlobalFree;
    const CF_DIB: u32 = 8;

    let dib = dib_bytes(image)?;
    let dib_handle = clipboard_handle_from_bytes(&dib)?;
    let png_handle = clipboard_handle_from_bytes(png)?;

    unsafe {
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(null_mut()) != 0 {
                opened = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        if !opened {
            GlobalFree(dib_handle);
            GlobalFree(png_handle);
            return Err("无法打开 Windows 剪贴板".to_string());
        }

        EmptyClipboard();
        let dib_written = SetClipboardData(CF_DIB, dib_handle);
        if dib_written.is_null() {
            CloseClipboard();
            GlobalFree(dib_handle);
            GlobalFree(png_handle);
            return Err("无法写入 Windows 图片剪贴板".to_string());
        }
        let png_format = RegisterClipboardFormatW("PNG\0".encode_utf16().collect::<Vec<_>>().as_ptr());
        if png_format != 0 {
            let png_written = SetClipboardData(png_format, png_handle);
            if png_written.is_null() {
                GlobalFree(png_handle);
            }
        } else {
            GlobalFree(png_handle);
        }
        CloseClipboard();
    }

    Ok(())
}

#[cfg(not(windows))]
fn write_image_to_clipboard(app: &AppHandle, image: &Image<'_>, _png: &[u8]) -> Result<(), String> {
    app.clipboard()
        .write_image(image)
        .map_err(|error| error.to_string())
}

fn insert_image_with_data_url(
    app: &AppHandle,
    image: &Image<'_>,
    image_data: String,
) -> Result<bool, String> {
    let content = image_content(image);
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    let settings = settings_from_connection(&connection);
    let changed = connection
        .execute(
            "
            INSERT INTO clipboard_items (content, item_type, image_data) VALUES (?1, 'image', ?2)
            ON CONFLICT(content) DO UPDATE SET created_at = datetime('now'), image_data = excluded.image_data
            ",
            params![content, image_data],
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

fn insert_image(app: &AppHandle, image: &Image<'_>) -> Result<bool, String> {
    insert_image_with_data_url(app, image, png_data_url(image)?)
}

#[cfg(windows)]
fn capture_screen_region(bounds: ScreenshotBounds) -> Result<Image<'static>, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        RGBQUAD, SRCCOPY,
    };

    if bounds.width == 0 || bounds.height == 0 {
        return Err("截图区域无效".to_string());
    }

    unsafe {
        let screen_dc = GetDC(null_mut());
        if screen_dc.is_null() {
            return Err("无法读取屏幕内容".to_string());
        }

        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc.is_null() {
            ReleaseDC(null_mut(), screen_dc);
            return Err("无法创建截图缓冲区".to_string());
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, bounds.width as i32, bounds.height as i32);
        if bitmap.is_null() {
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
            return Err("无法创建截图位图".to_string());
        }

        let previous = SelectObject(memory_dc, bitmap);
        let copied = BitBlt(
            memory_dc,
            0,
            0,
            bounds.width as i32,
            bounds.height as i32,
            screen_dc,
            bounds.x,
            bounds.y,
            SRCCOPY,
        ) != 0;

        if !copied {
            SelectObject(memory_dc, previous);
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
            return Err("截图失败".to_string());
        }

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bounds.width as i32,
                biHeight: -(bounds.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: bounds.width * bounds.height * 4,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };
        let mut bgra = vec![0u8; (bounds.width * bounds.height * 4) as usize];
        let lines = GetDIBits(
            memory_dc,
            bitmap,
            0,
            bounds.height,
            bgra.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(memory_dc, previous);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        ReleaseDC(null_mut(), screen_dc);

        if lines == 0 {
            return Err("无法读取截图像素".to_string());
        }

        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        Ok(Image::new_owned(bgra, bounds.width, bounds.height))
    }
}

#[cfg(not(windows))]
fn capture_screen_region(_bounds: ScreenshotBounds) -> Result<Image<'static>, String> {
    Err("截图功能目前仅支持 Windows".to_string())
}

fn crop_image(image: &Image<'_>, selection: ScreenshotSelection) -> Result<Image<'static>, String> {
    if selection.width < 2.0
        || selection.height < 2.0
        || selection.viewport_width <= 0.0
        || selection.viewport_height <= 0.0
    {
        return Err("截图区域太小".to_string());
    }

    let scale_x = image.width() as f64 / selection.viewport_width;
    let scale_y = image.height() as f64 / selection.viewport_height;
    let left = (selection.x * scale_x)
        .round()
        .clamp(0.0, image.width() as f64) as u32;
    let top = (selection.y * scale_y)
        .round()
        .clamp(0.0, image.height() as f64) as u32;
    let right = ((selection.x + selection.width) * scale_x)
        .round()
        .clamp(0.0, image.width() as f64) as u32;
    let bottom = ((selection.y + selection.height) * scale_y)
        .round()
        .clamp(0.0, image.height() as f64) as u32;
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    if width == 0 || height == 0 {
        return Err("截图区域太小".to_string());
    }

    let source = image.rgba();
    let source_width = image.width() as usize;
    let mut cropped = Vec::with_capacity((width * height * 4) as usize);
    for row in top..bottom {
        let start = ((row as usize * source_width + left as usize) * 4) as usize;
        let end = start + width as usize * 4;
        cropped.extend_from_slice(&source[start..end]);
    }

    Ok(Image::new_owned(cropped, width, height))
}

fn show_screenshot_saved_feedback(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let success_icon = Image::new_owned(success_tray_icon_rgba(32), 32, 32);
        let default_icon = app
            .default_window_icon()
            .map(|icon| Image::new_owned(icon.rgba().to_vec(), icon.width(), icon.height()));
        let _ = tray.set_icon(Some(success_icon));
        let _ = tray.set_tooltip(Some("截图已保存至剪贴板"));
        let app: AppHandle = <AppHandle as Clone>::clone(app);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(2));
            if let Some(tray) = app.tray_by_id("main") {
                if let Some(icon) = default_icon {
                    let _ = tray.set_icon(Some(icon));
                }
                let _ = tray.set_tooltip(Some("PasteBoost - 轻量剪贴助手"));
            }
        });
    }
}

fn success_tray_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let radius = center - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy <= radius * radius {
                let offset = ((y * size + x) * 4) as usize;
                rgba[offset] = 40;
                rgba[offset + 1] = 184;
                rgba[offset + 2] = 124;
                rgba[offset + 3] = 255;
            }
        }
    }

    for (x, y) in [
        (9, 17),
        (10, 18),
        (11, 19),
        (12, 20),
        (13, 20),
        (14, 19),
        (15, 18),
        (16, 17),
        (17, 16),
        (18, 15),
        (19, 14),
        (20, 13),
        (21, 12),
        (22, 11),
    ] {
        for oy in 0..2 {
            for ox in 0..2 {
                let px = x + ox;
                let py = y + oy;
                if px < size && py < size {
                    let offset = ((py * size + px) * 4) as usize;
                    rgba[offset] = 255;
                    rgba[offset + 1] = 255;
                    rgba[offset + 2] = 255;
                    rgba[offset + 3] = 255;
                }
            }
        }
    }

    rgba
}

#[tauri::command]
async fn start_screenshot(app: AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到主窗口".to_string())?;
    let monitor = main
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(main.primary_monitor().map_err(|error| error.to_string())?)
        .ok_or_else(|| "找不到可截图的显示器".to_string())?;
    let position = monitor.position();
    let size = monitor.size();
    let bounds = ScreenshotBounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };

    let _ = main.hide();
    thread::sleep(Duration::from_millis(80));

    let image = match capture_screen_region(bounds) {
        Ok(image) => image,
        Err(error) => {
            show_window(app.get_webview_window("main"));
            return Err(error);
        }
    };
    {
        let store = app.state::<ScreenshotStore>();
        *store.0.lock().map_err(|error| error.to_string())? =
            Some(ScreenshotCapture { image, bounds });
    }

    let window = if let Some(existing) = app.get_webview_window("snip") {
        existing
    } else {
        WebviewWindowBuilder::new(&app, "snip", WebviewUrl::App("index.html?snip=1".into()))
            .title("PasteBoost 截图")
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .inner_size(bounds.width as f64, bounds.height as f64)
            .position(bounds.x as f64, bounds.y as f64)
            .build()
            .map_err(|error| {
                let store = app.state::<ScreenshotStore>();
                if let Ok(mut capture) = store.0.lock() {
                    *capture = None;
                }
                show_window(app.get_webview_window("main"));
                error.to_string()
            })?
    };
    let _ = window.set_position(tauri::Position::Physical(PhysicalPosition {
        x: bounds.x,
        y: bounds.y,
    }));
    let _ = window.set_size(tauri::Size::Physical(PhysicalSize {
        width: bounds.width,
        height: bounds.height,
    }));
    let _ = window.emit("screenshot-session-started", ());
    thread::sleep(Duration::from_millis(16));
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

#[tauri::command]
fn get_screenshot_session(app: AppHandle) -> Result<ScreenshotSession, String> {
    let store = app.state::<ScreenshotStore>();
    let capture = store.0.lock().map_err(|error| error.to_string())?;
    let capture = capture
        .as_ref()
        .ok_or_else(|| "截图会话已失效".to_string())?;
    Ok(ScreenshotSession {
        bounds: capture.bounds,
    })
}

#[tauri::command]
fn finish_screenshot(app: AppHandle, selection: ScreenshotSelection) -> Result<(), String> {
    let capture = {
        let store = app.state::<ScreenshotStore>();
        let mut guard = store.0.lock().map_err(|error| error.to_string())?;
        guard.take().ok_or_else(|| "截图会话已失效".to_string())?
    };
    if let Some(window) = app.get_webview_window("snip") {
        let _ = window.hide();
    }
    let image = crop_image(&capture.image, selection)?;
    let png = png_bytes(&image)?;
    write_image_to_clipboard(&app, &image, &png)?;
    show_screenshot_saved_feedback(&app);
    let app_for_history = app.clone();
    thread::spawn(move || {
        match insert_image_with_data_url(&app_for_history, &image, png_data_url_from_bytes(&png)) {
            Ok(true) => {
                let _ = app_for_history.emit("clipboard-updated", ());
            }
            Ok(false) => {}
            Err(error) => eprintln!("[PasteBoost] Failed to save screenshot history: {error}"),
        }
        if let Err(error) = save_png_to_pictures(&app_for_history, &png) {
            eprintln!("[PasteBoost] Failed to save screenshot: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_screenshot(app: AppHandle) -> Result<(), String> {
    let store = app.state::<ScreenshotStore>();
    *store.0.lock().map_err(|error| error.to_string())? = None;
    if let Some(window) = app.get_webview_window("snip") {
        let _ = window.hide();
    }
    show_window(app.get_webview_window("main"));
    Ok(())
}

#[tauri::command]
fn list_items(app: AppHandle, query: String) -> Result<Vec<ClipboardItem>, String> {
    let state = app.state::<Database>();
    let connection = state.0.lock().map_err(|error| error.to_string())?;
    let needle = format!("%{}%", query);
    let mut statement = connection
        .prepare(
            "
            SELECT id, content, item_type, image_data, is_favorite, created_at || 'Z', used_count
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
                image_data: row.get(3)?,
                is_favorite: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                used_count: row.get(6)?,
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
fn capture_current_clipboard(app: AppHandle) -> Result<(), String> {
    let changed = if let Ok(image) = app.clipboard().read_image() {
        insert_image(&app, &image)?
    } else if let Ok(content) = app.clipboard().read_text() {
        insert_text(&app, content)?
    } else {
        false
    };
    if changed {
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

fn register_global_shortcuts(
    app: &AppHandle,
    panel_hotkey: &str,
    screenshot_hotkey: &str,
) -> Result<(), String> {
    if panel_hotkey.eq_ignore_ascii_case(screenshot_hotkey) {
        return Err("截图快捷键不能和呼出快捷键相同".to_string());
    }
    app.global_shortcut()
        .on_shortcut(panel_hotkey, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                show_window(app.get_webview_window("main"));
                let _ = app.emit("panel-opened", ());
            }
        })
        .map_err(|error| error.to_string())?;
    app.global_shortcut()
        .on_shortcut(screenshot_hotkey, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = start_screenshot(app).await {
                        eprintln!("[PasteBoost] Failed to start screenshot: {error}");
                    }
                });
            }
        })
        .map_err(|error| format!("截图快捷键 {screenshot_hotkey} 注册失败：{error}"))
}

#[tauri::command]
fn save_settings(app: AppHandle, mut settings: Settings) -> Result<(), String> {
    let shortcut = settings.hotkey.trim().to_string();
    let shortcut = normalize_hotkey(&shortcut);
    if !valid_hotkey(&shortcut) {
        return Err("快捷键必须包含至少一个修饰键，例如 Ctrl+Shift+V".to_string());
    }
    let screenshot_shortcut = settings.screenshot_hotkey.trim().to_string();
    let screenshot_shortcut = normalize_hotkey(&screenshot_shortcut);
    if !valid_hotkey(&screenshot_shortcut) {
        return Err("截图快捷键必须包含至少一个修饰键，例如 Super+F1".to_string());
    }
    if shortcut.eq_ignore_ascii_case(&screenshot_shortcut) {
        return Err("截图快捷键不能和呼出快捷键相同".to_string());
    }
    settings.hotkey = shortcut.clone();
    settings.screenshot_hotkey = screenshot_shortcut.clone();
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

    let hotkey_changed =
        previous.hotkey != shortcut || previous.screenshot_hotkey != screenshot_shortcut;
    if hotkey_changed {
        if let Err(error) = app.global_shortcut().unregister_all() {
            if autostart_changed {
                let _ = set_autostart(&app, autostart_was_enabled);
            }
            return Err(error.to_string());
        }
        if let Err(error) =
            register_global_shortcuts(&app, shortcut.as_str(), screenshot_shortcut.as_str())
        {
            let _ = register_global_shortcuts(
                &app,
                previous.hotkey.as_str(),
                previous.screenshot_hotkey.as_str(),
            );
            if autostart_changed {
                let _ = set_autostart(&app, autostart_was_enabled);
            }
            return Err(error);
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
            let _ = register_global_shortcuts(
                &app,
                previous.hotkey.as_str(),
                previous.screenshot_hotkey.as_str(),
            );
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

#[tauri::command]
fn paste_image(app: AppHandle, id: i64, auto_paste: bool) -> Result<(), String> {
    let data_url = {
        let state = app.state::<Database>();
        let value = state
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .query_row(
                "SELECT image_data FROM clipboard_items WHERE id = ?1 AND item_type = 'image'",
                [id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten()
            .ok_or_else(|| "找不到图片记录".to_string())?;
        value
    };
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "图片格式无效".to_string())?;
    let bytes = BASE64.decode(encoded).map_err(|error| error.to_string())?;
    let rgba = image::load_from_memory(&bytes)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let clipboard_image = Image::new_owned(rgba.into_raw(), width, height);
    write_image_to_clipboard(&app, &clipboard_image, &bytes)?;
    if auto_paste {
        let state = app.state::<Database>();
        state
            .0
            .lock()
            .map_err(|error| error.to_string())?
            .execute(
                "UPDATE clipboard_items SET used_count = used_count + 1, last_used_at = datetime('now') WHERE id = ?1",
                [id],
            )
            .map_err(|error| error.to_string())?;
        if let Some(window) = app.get_webview_window("main") {
            window.hide().map_err(|error| error.to_string())?;
        }
        thread::sleep(Duration::from_millis(100));
        send_paste_keys();
    }
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
        let mut last_signature = String::new();
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
            if let Ok(image) = app.clipboard().read_image() {
                let signature = format!("image:{:016x}", image_signature(&image));
                if signature != last_signature {
                    last_signature = signature;
                    if insert_image(&app, &image).unwrap_or(false) {
                        let _ = app.emit("clipboard-updated", ());
                    }
                }
                continue;
            }
            if let Ok(content) = app.clipboard().read_text() {
                let signature = format!("text:{content}");
                if signature != last_signature {
                    last_signature = signature;
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
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
            app.manage(ScreenshotStore(Mutex::new(None)));
            register_global_shortcuts(
                app.handle(),
                settings.hotkey.as_str(),
                settings.screenshot_hotkey.as_str(),
            )
            .map_err(std::io::Error::other)?;

            let show = MenuItem::with_id(app, "show", "显示 PasteBoost", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "暂停 / 恢复监听", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &pause, &quit])?;
            TrayIconBuilder::with_id("main")
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
                .on_tray_icon_event(|tray, event| {
                    let should_show = matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        }
                    );
                    if should_show {
                        show_window(tray.app_handle().get_webview_window("main"));
                    }
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
            capture_current_clipboard,
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
            start_screenshot,
            get_screenshot_session,
            finish_screenshot,
            cancel_screenshot,
            paste_text,
            paste_image
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PasteBoost");
}
