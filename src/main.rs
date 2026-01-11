// Cargo.toml остаётся тем же

use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::Graphics::Gdi::*,
    Win32::Networking::WinHttp::*,
    Win32::System::LibraryLoader::GetModuleHandleW,
    Win32::System::SystemInformation::*,
    Win32::UI::WindowsAndMessaging::*,
};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use std::{fs, ptr, thread};

type StdResult<T, E> = std::result::Result<T, E>;

// ═══════════════════════════════════════════════════════════════════
// Глобальное состояние
// ═══════════════════════════════════════════════════════════════════

static APP_STATE: OnceLock<Arc<AppState>> = OnceLock::new();

struct AppState {
    clock: Clock,
    config: Mutex<Config>,
    ui: Mutex<UiHandles>,
}

#[derive(Default)]
struct UiHandles {
    combo: HWND,
    edit: HWND,
}

fn app() -> &'static Arc<AppState> {
    APP_STATE.get().expect("AppState not initialized")
}

// ═══════════════════════════════════════════════════════════════════
// Константы
// ═══════════════════════════════════════════════════════════════════

const CBS_DROPDOWNLIST: u32 = 0x0003;
const CBS_HASSTRINGS: u32 = 0x0200;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_GETLBTEXT: u32 = 0x0148;
const CB_SETCURSEL: u32 = 0x014E;
const CB_RESETCONTENT: u32 = 0x014B;

const ID_COMBO: i32 = 101;
const ID_FETCH: i32 = 102;
const ID_EDIT: i32 = 103;
const ID_ADD: i32 = 104;

const NETWORK_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

// ═══════════════════════════════════════════════════════════════════
// Config
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default = "Config::default_timezones")]
    timezones: Vec<String>,
    #[serde(default = "Config::default_window")]
    window: WindowConfig,
}

#[derive(Clone, Serialize, Deserialize)]
struct WindowConfig {
    #[serde(default = "WindowConfig::default_width")]
    width: i32,
    #[serde(default = "WindowConfig::default_height")]
    height: i32,
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    #[serde(default)]
    font_size: Option<i32>,
    #[serde(default = "WindowConfig::default_font")]
    font_name: String,
}

impl WindowConfig {
    fn default_width() -> i32 { 500 }
    fn default_height() -> i32 { 300 }
    fn default_font() -> String { "Consolas".into() }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: Self::default_width(),
            height: Self::default_height(),
            x: None,
            y: None,
            font_size: None,
            font_name: Self::default_font(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timezones: Self::default_timezones(),
            window: Self::default_window(),
        }
    }
}

impl Config {
    const PATH: &'static str = "clock_config.json";

    fn default_timezones() -> Vec<String> {
        vec![
            "Europe/Moscow".into(),
            "Europe/London".into(),
            "America/New_York".into(),
            "Asia/Tokyo".into(),
        ]
    }

    fn default_window() -> WindowConfig {
        WindowConfig::default()
    }

    fn load() -> Self {
        match fs::read_to_string(Self::PATH) {
            Ok(s) => {
                println!("[CONFIG] Loaded from {}", Self::PATH);
                let config: Config = serde_json::from_str(&s).unwrap_or_else(|e| {
                    println!("[CONFIG] Parse error: {}, using defaults", e);
                    Config::default()
                });
                println!("[CONFIG] Window: {}x{}, font: {} (size: {:?})",
                         config.window.width, config.window.height,
                         config.window.font_name, config.window.font_size);
                config
            }
            Err(_) => {
                println!("[CONFIG] No config file, creating default");
                let config = Config::default();
                config.save();
                config
            }
        }
    }

    fn save(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = fs::write(Self::PATH, &json) {
                    println!("[CONFIG] Failed to save: {}", e);
                } else {
                    println!("[CONFIG] Saved to {}", Self::PATH);
                }
            }
            Err(e) => println!("[CONFIG] Serialize error: {}", e),
        }
    }

    fn add_timezone(&mut self, tz: String) -> bool {
        if self.timezones.contains(&tz) {
            println!("[CONFIG] Timezone already exists: {}", tz);
            return false;
        }
        println!("[CONFIG] Adding timezone: {}", tz);
        self.timezones.push(tz);
        self.save();
        true
    }
}

// ═══════════════════════════════════════════════════════════════════
// Clock
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone)]
enum TimeSource {
    Local,
    Network { timezone: String, offset_min: i32 },
}

struct Clock {
    state: Mutex<ClockState>,
}

struct ClockState {
    source: TimeSource,
    display: String,
    last_sync: Instant,
}

impl Clock {
    fn new() -> Self {
        Self {
            state: Mutex::new(ClockState {
                source: TimeSource::Local,
                display: "00:00:00".into(),
                last_sync: Instant::now(),
            }),
        }
    }

    fn tick(&self) {
        let mut state = self.state.lock().unwrap();
        let (h, m, s) = get_local_time();
        let source = state.source.clone();

        match source {
            TimeSource::Local => {
                state.display = format!("Local: {:02}:{:02}:{:02}", h, m, s);
            }
            TimeSource::Network { ref timezone, mut offset_min } => {
                if state.last_sync.elapsed() > NETWORK_REFRESH_INTERVAL {
                    if let Ok(new_offset) = fetch_offset(timezone) {
                        offset_min = new_offset;
                        state.source = TimeSource::Network {
                            timezone: timezone.clone(),
                            offset_min: new_offset,
                        };
                        state.last_sync = Instant::now();
                    }
                }

                let total = (h as i32 * 60 + m as i32 + offset_min).rem_euclid(1440);
                let (ah, am) = (total / 60, total % 60);
                let loc = timezone.rsplit('/').next().unwrap_or(timezone);
                state.display = format!("{}: {:02}:{:02}:{:02}", loc, ah, am, s);
            }
        }
    }

    fn display(&self) -> String {
        self.state.lock().unwrap().display.clone()
    }

    fn set_display(&self, text: String) {
        self.state.lock().unwrap().display = text;
    }

    fn switch_to_local(&self) {
        println!("[CLOCK] Switching to local time");
        self.state.lock().unwrap().source = TimeSource::Local;
    }

    fn switch_to_network(&self, timezone: String) -> StdResult<(), String> {
        println!("[CLOCK] Switching to network: {}", timezone);
        let offset = fetch_offset(&timezone)?;
        println!("[CLOCK] Offset calculated: {} minutes", offset);
        let mut state = self.state.lock().unwrap();
        state.source = TimeSource::Network { timezone, offset_min: offset };
        state.last_sync = Instant::now();
        Ok(())
    }

    fn start_loop(hwnd: HWND) {
        let hwnd_val = hwnd.0 as isize;
        thread::spawn(move || loop {
            app().clock.tick();
            win::invalidate(HWND(hwnd_val as _));
            thread::sleep(Duration::from_secs(1));
        });
    }
}

fn get_local_time() -> (u16, u16, u16) {
    let st = unsafe { GetLocalTime() };
    (st.wHour, st.wMinute, st.wSecond)
}

// ═══════════════════════════════════════════════════════════════════
// Network
// ═══════════════════════════════════════════════════════════════════

struct WinHttpHandle(*mut c_void);

impl WinHttpHandle {
    fn new(h: *mut c_void) -> Option<Self> {
        if h.is_null() { None } else { Some(Self(h)) }
    }
    fn ptr(&self) -> *mut c_void { self.0 }
}

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        unsafe { let _ = WinHttpCloseHandle(self.0); }
    }
}

fn fetch_offset(timezone: &str) -> StdResult<i32, String> {
    println!("[NET] Fetching offset for: {}", timezone);
    let time_str = fetch_time_string(timezone)?;
    println!("[NET] Got time: {}", time_str);
    parse_offset(&time_str).ok_or_else(|| "parse failed".into())
}

fn winhttp_error_name(code: i32) -> &'static str {
    match code {
        0x00002EE2 => "TIMEOUT",
        0x00002EE3 => "INTERNAL_ERROR",
        0x00002EE5 => "INVALID_URL",
        0x00002EE6 => "UNRECOGNIZED_SCHEME",
        0x00002EE7 => "NAME_NOT_RESOLVED",
        0x00002EFD => "CANNOT_CONNECT",
        0x00002F06 => "CONNECTION_ERROR",
        0x00002F7D => "SECURE_FAILURE",
        _ => "UNKNOWN",
    }
}

fn fetch_time_string(timezone: &str) -> StdResult<String, String> {
    unsafe {
        println!("[NET] Opening session...");
        let session = WinHttpHandle::new(
            WinHttpOpen(w!("MinimalClock/1.0"), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, PCWSTR::null(), PCWSTR::null(), 0)
        ).ok_or("WinHttpOpen failed")?;

        println!("[NET] Connecting to timeapi.io:443...");
        let conn = WinHttpHandle::new(
            WinHttpConnect(session.ptr(), w!("timeapi.io"), 443, 0)
        ).ok_or("WinHttpConnect failed")?;

        let path: Vec<u16> = format!("/api/time/current/zone?timeZone={}", timezone)
            .encode_utf16().chain(Some(0)).collect();
        println!("[NET] GET /api/time/current/zone?timeZone={}", timezone);

        let req = WinHttpHandle::new(
            WinHttpOpenRequest(conn.ptr(), w!("GET"), PCWSTR(path.as_ptr()), PCWSTR::null(), PCWSTR::null(), ptr::null(), WINHTTP_FLAG_SECURE)
        ).ok_or("WinHttpOpenRequest failed")?;

        if let Err(e) = WinHttpSendRequest(req.ptr(), None, None, 0, 0, 0) {
            let code = e.code().0 & 0xFFFF;
            println!("[NET] ERROR: {} (0x{:08X})", winhttp_error_name(code), e.code().0);
            return Err(format!("{} (0x{:08X})", winhttp_error_name(code), e.code().0));
        }

        if let Err(e) = WinHttpReceiveResponse(req.ptr(), ptr::null_mut()) {
            let code = e.code().0 & 0xFFFF;
            println!("[NET] ERROR: {} (0x{:08X})", winhttp_error_name(code), e.code().0);
            return Err(format!("{} (0x{:08X})", winhttp_error_name(code), e.code().0));
        }

        let mut buf = vec![0u8; 2048];
        let mut read = 0u32;
        let mut data = Vec::new();

        while WinHttpReadData(req.ptr(), buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).is_ok() && read > 0 {
            data.extend_from_slice(&buf[..read as usize]);
            read = 0;
        }
        println!("[NET] Read {} bytes", data.len());

        let json = String::from_utf8_lossy(&data);
        parse_timeapi_response(&json).ok_or_else(|| "JSON parse failed".into())
    }
}

fn parse_timeapi_response(json: &str) -> Option<String> {
    let hour = extract_json_int(json, "\"hour\":")?;
    let minute = extract_json_int(json, "\"minute\":")?;
    let seconds = extract_json_int(json, "\"seconds\":")?;
    Some(format!("{:02}:{:02}:{:02}", hour, minute, seconds))
}

fn extract_json_int(json: &str, key: &str) -> Option<i32> {
    let start = json.find(key)? + key.len();
    let rest = &json[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 { return None; }
    rest[..end].parse().ok()
}

fn parse_offset(time: &str) -> Option<i32> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() < 2 { return None; }

    let rh: i32 = parts[0].parse().ok()?;
    let rm: i32 = parts[1].parse().ok()?;
    let (lh, lm, _) = get_local_time();

    let remote = rh * 60 + rm;
    let local = lh as i32 * 60 + lm as i32;
    let mut diff = remote - local;

    if diff > 720 { diff -= 1440; }
    else if diff < -720 { diff += 1440; }

    Some(diff)
}

// ═══════════════════════════════════════════════════════════════════
// Safe Windows API wrappers
// ═══════════════════════════════════════════════════════════════════

mod win {
    use super::*;

    pub fn invalidate(hwnd: HWND) {
        unsafe { let _ = InvalidateRect(hwnd, None, true); }
    }

    pub fn send_message(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
        unsafe { SendMessageW(hwnd, msg, WPARAM(wp), LPARAM(lp)).0 }
    }

    pub fn get_window_text(hwnd: HWND) -> String {
        let mut buf = vec![0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..len])
    }

    pub fn set_window_text(hwnd: HWND, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe { let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr())); }
    }

    pub fn create_control(parent: HWND, class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: i32) -> HWND {
        let cls: Vec<u16> = class.encode_utf16().chain(Some(0)).collect();
        let txt: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(cls.as_ptr()),
                PCWSTR(txt.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(style),
                x, y, w, h,
                parent,
                HMENU(id as _),
                GetModuleHandleW(None).unwrap(),
                None,
            )
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// GDI RAII
// ═══════════════════════════════════════════════════════════════════

struct GdiFont(HFONT);
impl GdiFont {
    fn new(size: i32, weight: i32, name: &str) -> Self {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        Self(unsafe { CreateFontW(size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 0, 0, PCWSTR(wide.as_ptr())) })
    }
    fn handle(&self) -> HGDIOBJ { HGDIOBJ(self.0.0) }
}
impl Drop for GdiFont {
    fn drop(&mut self) { unsafe { let _ = DeleteObject(HGDIOBJ(self.0.0)); } }
}

struct GdiBrush(HBRUSH);
impl GdiBrush {
    fn solid(color: u32) -> Self {
        Self(unsafe { CreateSolidBrush(COLORREF(color)) })
    }
    fn handle(&self) -> HBRUSH { self.0 }
}
impl Drop for GdiBrush {
    fn drop(&mut self) { unsafe { let _ = DeleteObject(HGDIOBJ(self.0.0)); } }
}

// ═══════════════════════════════════════════════════════════════════
// UI Helpers
// ═══════════════════════════════════════════════════════════════════

fn populate_combo(hwnd: HWND, config: &Config) {
    win::send_message(hwnd, CB_RESETCONTENT, 0, 0);

    let add = |s: &str| {
        let w: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
        win::send_message(hwnd, CB_ADDSTRING, 0, w.as_ptr() as _);
    };

    add("Local");
    for tz in &config.timezones {
        add(tz);
    }
    win::send_message(hwnd, CB_SETCURSEL, 0, 0);
}

fn get_combo_selection(hwnd: HWND) -> (usize, String) {
    let idx = win::send_message(hwnd, CB_GETCURSEL, 0, 0) as usize;
    let mut buf = vec![0u16; 256];
    win::send_message(hwnd, CB_GETLBTEXT, idx, buf.as_mut_ptr() as _);
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    (idx, String::from_utf16_lossy(&buf[..len]))
}

// ═══════════════════════════════════════════════════════════════════
// Window Procedure
// ═══════════════════════════════════════════════════════════════════

fn handle_create(hwnd: HWND) {
    let config = app().config.lock().unwrap();

    let combo = win::create_control(hwnd, "COMBOBOX", "", CBS_DROPDOWNLIST | CBS_HASSTRINGS, 10, 10, 180, 200, ID_COMBO);
    win::create_control(hwnd, "BUTTON", "Fetch", 0, 200, 10, 60, 25, ID_FETCH);
    let edit = win::create_control(hwnd, "EDIT", "", 0, 270, 10, 150, 25, ID_EDIT);
    win::create_control(hwnd, "BUTTON", "+", 0, 430, 10, 30, 25, ID_ADD);

    populate_combo(combo, &config);

    *app().ui.lock().unwrap() = UiHandles { combo, edit };

    drop(config);
    Clock::start_loop(hwnd);
}

fn handle_command(hwnd: HWND, id: i32) {
    let ui = app().ui.lock().unwrap();

    match id {
        ID_FETCH => {
            let (idx, text) = get_combo_selection(ui.combo);
            drop(ui);

            if idx == 0 {
                app().clock.switch_to_local();
            } else {
                app().clock.set_display("Loading...".into());
                win::invalidate(hwnd);

                if let Err(e) = app().clock.switch_to_network(text) {
                    app().clock.set_display(format!("Error: {}", e));
                }
            }
            win::invalidate(hwnd);
        }
        ID_ADD => {
            let tz = win::get_window_text(ui.edit).trim().to_string();
            if !tz.is_empty() {
                let mut config = app().config.lock().unwrap();
                if config.add_timezone(tz) {
                    populate_combo(ui.combo, &config);
                }
                win::set_window_text(ui.edit, "");
            }
        }
        _ => {}
    }
}

fn handle_paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);

        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);

        // Background
        let bg = GdiBrush::solid(0x00F0F0F0);
        FillRect(hdc, &rc, bg.handle());

        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00202020));

        // Настройки из конфига
        let config = app().config.lock().unwrap();
        let font_size = config.window.font_size.unwrap_or_else(|| {
            let min_dim = (rc.right - rc.left).min(rc.bottom - rc.top);
            (min_dim / 5).clamp(24, 200)
        });
        let font_name = config.window.font_name.clone();
        drop(config);

        // Main time
        let font = GdiFont::new(font_size, 700, &font_name);
        let old = SelectObject(hdc, font.handle());

        let text = app().clock.display();
        let mut wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut text_rc = RECT { top: 50, ..rc };
        DrawTextW(hdc, &mut wide, &mut text_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        SelectObject(hdc, old);

        // Hint
        let hint_font = GdiFont::new(11, 400, "Segoe UI");
        let old2 = SelectObject(hdc, hint_font.handle());
        SetTextColor(hdc, COLORREF(0x00808080));

        let hint = "Select timezone & Fetch | Add custom with +";
        let mut hw: Vec<u16> = hint.encode_utf16().chain(Some(0)).collect();
        let mut hint_rc = RECT { top: rc.bottom - 30, ..rc };
        DrawTextW(hdc, &mut hw, &mut hint_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        SelectObject(hdc, old2);
        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            handle_create(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(hwnd, (wp.0 & 0xFFFF) as i32);
            LRESULT(0)
        }
        WM_PAINT => {
            handle_paint(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            println!("[UI] Window closing");
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Entry point
// ═══════════════════════════════════════════════════════════════════

fn main() -> Result<()> {
    println!("=== Minimal Clock Starting ===");

    // Загружаем конфиг ДО создания окна
    let config = Config::load();
    let win_cfg = config.window.clone();

    // Инициализируем глобальное состояние
    let state = Arc::new(AppState {
        clock: Clock::new(),
        config: Mutex::new(config),
        ui: Mutex::new(UiHandles::default()),
    });
    let _ = APP_STATE.set(state);

    // Позиция окна
    let x = win_cfg.x.unwrap_or(CW_USEDEFAULT);
    let y = win_cfg.y.unwrap_or(CW_USEDEFAULT);

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("MinimalClockClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as _),
            ..Default::default()
        };
        RegisterClassW(&wc);

        println!("[UI] Creating window {}x{} at ({}, {})", win_cfg.width, win_cfg.height, x, y);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Minimal Clock"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x, y, win_cfg.width, win_cfg.height,
            None, None, instance, None,
        );

        if hwnd.0 == 0 {
            return Err(Error::from_win32());
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Ok(())
    }
}