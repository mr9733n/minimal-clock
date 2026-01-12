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
use std::sync::atomic::{AtomicBool, Ordering};

type StdResult<T, E> = std::result::Result<T, E>;

// ═══════════════════════════════════════════════════════════════════
// Global State
// ═══════════════════════════════════════════════════════════════════

static SHOULD_STOP: AtomicBool = AtomicBool::new(false);
static APP_STATE: OnceLock<Arc<AppState>> = OnceLock::new();

struct AppState {
    clock: Clock,
    config: Mutex<Config>,
    ui: Mutex<UiHandles>,
    providers: ProviderChain,
}

#[derive(Default)]
struct UiHandles {
    combo: HWND,
    edit: HWND,
    provider_combo: HWND,
}

fn app() -> &'static Arc<AppState> {
    APP_STATE.get().expect("AppState not initialized")
}

// ═══════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════

const CBS_DROPDOWNLIST: u32 = 0x0003;
const CBS_HASSTRINGS: u32 = 0x0200;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_GETLBTEXT: u32 = 0x0148;
const CB_SETCURSEL: u32 = 0x014E;
const CB_RESETCONTENT: u32 = 0x014B;
const CBN_SELCHANGE: i32 = 1;

const ID_COMBO: i32 = 101;
const ID_EDIT: i32 = 103;
const ID_ADD: i32 = 104;
const ID_PROVIDER_COMBO: i32 = 105;
const NETWORK_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

// ═══════════════════════════════════════════════════════════════════
// Config
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone, Serialize, Deserialize)]
struct ProviderConfig {
    #[serde(rename = "type")]
    provider_type: String,
    priority: u32,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    api_key: String,
}

fn default_enabled() -> bool { true }

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default = "Config::default_timezones")]
    timezones: Vec<String>,
    #[serde(default = "Config::default_providers")]
    providers: Vec<ProviderConfig>,
    #[serde(default = "Config::default_window")]
    window: WindowConfig,
    #[serde(default = "Config::default_iana_timezones")]
    iana_timezones: Vec<String>,
    #[serde(default)]
    last_selected_timezone: Option<String>,
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
    fn default_width() -> i32 { 800 }
    fn default_height() -> i32 { 200 }
    fn default_font() -> String { "Consolas".into() }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: Self::default_width(),
            height: Self::default_height(),
            x: Some(500),
            y: Some(500),
            font_size: Some(100),
            font_name: Self::default_font(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timezones: Self::default_timezones(),
            providers: Self::default_providers(),
            window: Self::default_window(),
            iana_timezones: Self::default_iana_timezones(),
            last_selected_timezone: None,
        }
    }
}

impl Config {
    const PATH: &'static str = "clock_config.json";

    fn set_last_timezone(&mut self, tz: Option<String>) {
        self.last_selected_timezone = tz;
        self.save();
    }

    fn default_timezones() -> Vec<String> {
        vec![
            "Europe/Moscow".into(),
            "Europe/London".into(),
            "America/New_York".into(),
            "Asia/Tokyo".into(),
            "Asia/Bangkok".into(),
            "Asia/Hong_Kong".into(),
        ]
    }

    fn default_iana_timezones() -> Vec<String> {
        vec![
            "Africa/Cairo", "Africa/Johannesburg", "Africa/Lagos", "Africa/Nairobi",
            "America/Anchorage", "America/Argentina/Buenos_Aires", "America/Chicago",
            "America/Denver", "America/Detroit", "America/Los_Angeles", "America/Mexico_City",
            "America/New_York", "America/Phoenix", "America/Sao_Paulo", "America/Toronto",
            "America/Vancouver", "Asia/Bangkok", "Asia/Colombo", "Asia/Dubai", "Asia/Hong_Kong",
            "Asia/Jakarta", "Asia/Jerusalem", "Asia/Karachi", "Asia/Kolkata", "Asia/Manila",
            "Asia/Riyadh", "Asia/Seoul", "Asia/Shanghai", "Asia/Singapore", "Asia/Taipei",
            "Asia/Tokyo", "Atlantic/Reykjavik", "Australia/Adelaide", "Australia/Brisbane",
            "Australia/Melbourne", "Australia/Perth", "Australia/Sydney", "Europe/Amsterdam",
            "Europe/Athens", "Europe/Berlin", "Europe/Brussels", "Europe/Budapest",
            "Europe/Copenhagen", "Europe/Dublin", "Europe/Helsinki", "Europe/Istanbul",
            "Europe/Lisbon", "Europe/London", "Europe/Madrid", "Europe/Moscow", "Europe/Oslo",
            "Europe/Paris", "Europe/Prague", "Europe/Rome", "Europe/Stockholm", "Europe/Vienna",
            "Europe/Warsaw", "Europe/Zurich", "Pacific/Auckland", "Pacific/Fiji",
            "Pacific/Honolulu", "UTC",
        ].iter().map(|s| s.to_string()).collect()
    }

    fn default_providers() -> Vec<ProviderConfig> {
        vec![
            ProviderConfig {
                provider_type: "worldtimeapi".into(),
                priority: 1,
                enabled: true,
                api_key: String::new(),
            },
            ProviderConfig {
                provider_type: "timeapi_io".into(),
                priority: 2,
                enabled: true,
                api_key: String::new(),
            },
            ProviderConfig {
                provider_type: "timezonedb".into(),
                priority: 3,
                enabled: false,
                api_key: String::new(),
            },
        ]
    }

    fn default_window() -> WindowConfig {
        WindowConfig::default()
    }

    fn load() -> Self {
        match fs::read_to_string(Self::PATH) {
            Ok(s) => {
                println!("[CONFIG] Loaded from {}", Self::PATH);
                let mut config: Config = serde_json::from_str(&s).unwrap_or_else(|e| {
                    println!("[CONFIG] Parse error: {}, using defaults", e);
                    Config::default()
                });

                // Migrate: update IANA timezones list if outdated
                let default_iana = Self::default_iana_timezones();
                if config.iana_timezones.len() != default_iana.len() {
                    println!("[CONFIG] Updating IANA timezones list: {} -> {} zones",
                             config.iana_timezones.len(), default_iana.len());
                    config.iana_timezones = default_iana;
                    config.save();
                }

                println!("[CONFIG] Window: {}x{}, providers: {}, iana zones: {}",
                         config.window.width, config.window.height,
                         config.providers.len(), config.iana_timezones.len());
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
    fn find_timezone(&self, query: &str) -> Option<String> {
        let query_lower = query.to_lowercase();
        println!("[CONFIG] Searching for timezone: '{}'", query);

        // Exact match first
        if let Some(tz) = self.iana_timezones.iter()
            .find(|t| t.to_lowercase() == query_lower) {
            println!("[CONFIG] Found exact match: {}", tz);
            return Some(tz.clone());
        }

        // Match by city name (after last /)
        if let Some(tz) = self.iana_timezones.iter()
            .find(|t| {
                if let Some(city) = t.split('/').last() {
                    let matches = city.to_lowercase() == query_lower;
                    if matches {
                        println!("[CONFIG] Found by city name: {} (from {})", city, t);
                    }
                    matches
                } else {
                    false
                }
            }) {
            return Some(tz.clone());
        }

        // Partial match in city name
        if let Some(tz) = self.iana_timezones.iter()
            .find(|t| {
                if let Some(city) = t.split('/').last() {
                    let matches = city.to_lowercase().contains(&query_lower);
                    if matches {
                        println!("[CONFIG] Found partial match in city: {} (from {})", city, t);
                    }
                    matches
                } else {
                    false
                }
            }) {
            return Some(tz.clone());
        }

        // Fallback: contains anywhere
        if let Some(tz) = self.iana_timezones.iter()
            .find(|t| {
                let matches = t.to_lowercase().contains(&query_lower);
                if matches {
                    println!("[CONFIG] Found contains match: {}", t);
                }
                matches
            }) {
            return Some(tz.clone());
        }

        println!("[CONFIG] No match found for '{}'", query);
        None
    }

    fn add_timezone(&mut self, tz: String) -> StdResult<(), String> {
        let tz_trimmed = tz.trim().to_string();

        // Try to find matching timezone
        let matched = match self.find_timezone(&tz_trimmed) {
            Some(found) => found,
            None => {
                // Generate suggestions
                let suggestions: Vec<String> = self.iana_timezones.iter()
                    .filter(|t| {
                        let lower = t.to_lowercase();
                        let query_lower = tz_trimmed.to_lowercase();
                        lower.contains(&query_lower) ||
                            t.split('/').last()
                                .map(|city| city.to_lowercase().contains(&query_lower))
                                .unwrap_or(false)
                    })
                    .take(5)
                    .cloned()
                    .collect();

                if suggestions.is_empty() {
                    return Err(format!("Timezone '{}' not found. Use IANA format (e.g., America/Detroit)", tz_trimmed));
                } else {
                    return Err(format!("Timezone '{}' not found. Did you mean:\n{}",
                                       tz_trimmed, suggestions.join("\n")));
                }
            }
        };

        if self.timezones.contains(&matched) {
            return Err(format!("Timezone '{}' already exists", matched));
        }

        println!("[CONFIG] Adding timezone: {} (matched from '{}')", matched, tz_trimmed);
        self.timezones.push(matched);
        self.save();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// Time Provider Abstraction
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct TimeInfo {
    hour: u8,
    minute: u8,
}

trait TimeProvider: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_time(&self, timezone: &str) -> StdResult<TimeInfo, String>;
}

// ─────────────────────────────────────────────────────────────────
// WorldTimeAPI Provider
// ─────────────────────────────────────────────────────────────────

struct WorldTimeApiProvider;

impl TimeProvider for WorldTimeApiProvider {
    fn name(&self) -> &str { "WorldTimeAPI" }

    fn fetch_time(&self, timezone: &str) -> StdResult<TimeInfo, String> {
        println!("[{}] Fetching {}", self.name(), timezone);
        let json = http_get("worldtimeapi.org", 443,
                            &format!("/api/timezone/{}", timezone))?;

        println!("[{}] Response preview: {}...", self.name(),
                 &json[..json.len().min(100)].replace('\n', " "));

        let datetime = extract_json_string(&json, "\"datetime\":\"")
            .ok_or("datetime field not found")?;

        // datetime format: "2026-01-12T23:42:54.352147+07:00"
        let time_part = datetime.split('T').nth(1)
            .and_then(|t| t.split('+').next())
            .and_then(|t| t.split('-').next())
            .ok_or("Invalid datetime format")?;

        parse_time_string(time_part)
    }
}

// ─────────────────────────────────────────────────────────────────
// TimeAPI.io Provider
// ─────────────────────────────────────────────────────────────────

struct TimeApiIoProvider;

impl TimeProvider for TimeApiIoProvider {
    fn name(&self) -> &str { "TimeAPI.io" }

    fn fetch_time(&self, timezone: &str) -> StdResult<TimeInfo, String> {
        println!("[{}] Fetching {}", self.name(), timezone);
        let json = http_get("timeapi.io", 443,
                            &format!("/api/time/current/zone?timeZone={}", timezone))?;

        println!("[{}] Response preview: {}...", self.name(),
                 &json[..json.len().min(100)].replace('\n', " "));

        let hour = extract_json_int(&json, "\"hour\":").ok_or("hour not found")?;
        let minute = extract_json_int(&json, "\"minute\":").ok_or("minute not found")?;

        Ok(TimeInfo {
            hour: hour as u8,
            minute: minute as u8,
        })
    }
}

// ─────────────────────────────────────────────────────────────────
// TimezoneDB Provider (requires API key)
// ─────────────────────────────────────────────────────────────────

struct TimezoneDbProvider {
    api_key: String,
}

impl TimeProvider for TimezoneDbProvider {
    fn name(&self) -> &str { "TimezoneDB" }

    fn fetch_time(&self, timezone: &str) -> StdResult<TimeInfo, String> {
        if self.api_key.is_empty() {
            return Err("API key not configured".into());
        }

        println!("[{}] Fetching {}", self.name(), timezone);
        let json = http_get("api.timezonedb.com", 443,
                            &format!("/v2.1/get-time-zone?key={}&format=json&by=zone&zone={}",
                                     self.api_key, timezone))?;

        println!("[{}] Response preview: {}...", self.name(),
                 &json[..json.len().min(100)].replace('\n', " "));

        // Check for API errors
        if json.contains("\"status\":\"FAILED\"") || json.contains("\"status\": \"FAILED\"") {
            let message = extract_json_string(&json, "\"message\":\"")
                .or_else(|| extract_json_string(&json, "\"message\": \""))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(format!("API error: {}", message));
        }

        // Try formatted field first
        if let Some(formatted) = extract_json_string(&json, "\"formatted\":\"")
            .or_else(|| extract_json_string(&json, "\"formatted\": \"")) {
            println!("[{}] Using formatted field: {}", self.name(), formatted);
            let time_part = formatted.split(' ').nth(1)
                .ok_or("Invalid formatted time")?;
            return parse_time_string(time_part);
        }

        // Fallback to gmtOffset calculation
        println!("[{}] No formatted field, using gmtOffset", self.name());
        let gmt_offset = extract_json_int_flexible(&json, "gmtOffset")
            .ok_or_else(|| format!("gmtOffset not found"))?;
        let timestamp = extract_json_int_flexible(&json, "timestamp")
            .ok_or_else(|| format!("timestamp not found"))?;

        // Calculate local time from UTC timestamp + offset
        let local_seconds = (timestamp + gmt_offset) % 86400;
        let hour = ((local_seconds / 3600) % 24) as u8;
        let minute = ((local_seconds % 3600) / 60) as u8;

        println!("[{}] Calculated time: {:02}:{:02}", self.name(), hour, minute);
        Ok(TimeInfo { hour, minute })
    }
}

// ─────────────────────────────────────────────────────────────────
// Provider Chain with Fallback
// ─────────────────────────────────────────────────────────────────

struct ProviderChain {
    providers: Vec<Box<dyn TimeProvider>>,
    active_index: Mutex<Option<usize>>, // None = auto fallback
}

impl ProviderChain {
    fn from_config(config: &Config) -> Self {
        let mut providers: Vec<Box<dyn TimeProvider>> = Vec::new();
        let mut configs = config.providers.clone();
        configs.sort_by_key(|p| p.priority);

        for pconf in configs {
            if !pconf.enabled {
                continue;
            }

            let provider: Box<dyn TimeProvider> = match pconf.provider_type.as_str() {
                "worldtimeapi" => Box::new(WorldTimeApiProvider),
                "timeapi_io" => Box::new(TimeApiIoProvider),
                "timezonedb" => Box::new(TimezoneDbProvider {
                    api_key: pconf.api_key.clone()
                }),
                _ => continue,
            };

            println!("[CHAIN] Registered provider: {} (priority {})",
                     provider.name(), pconf.priority);
            providers.push(provider);
        }

        Self {
            providers,
            active_index: Mutex::new(None), // Auto mode by default
        }
    }

    fn provider_names(&self) -> Vec<String> {
        let mut names = vec!["Auto (fallback)".to_string()];
        names.extend(self.providers.iter().map(|p| p.name().to_string()));
        names
    }

    fn set_active_provider(&self, index: usize) {
        let mut active = self.active_index.lock().unwrap();
        if index == 0 {
            *active = None; // Auto mode
            println!("[CHAIN] Switched to Auto (fallback) mode");
        } else if index - 1 < self.providers.len() {
            *active = Some(index - 1);
            println!("[CHAIN] Manually selected provider: {}",
                     self.providers[index - 1].name());
        }
    }

    fn get_active_index(&self) -> usize {
        match *self.active_index.lock().unwrap() {
            None => 0,
            Some(idx) => idx + 1,
        }
    }

    fn fetch_time(&self, timezone: &str) -> StdResult<TimeInfo, String> {
        let active_idx = self.active_index.lock().unwrap().clone();

        // Manual mode - use specific provider
        if let Some(idx) = active_idx {
            if idx < self.providers.len() {
                let provider = &self.providers[idx];
                println!("[CHAIN] Using manually selected: {}", provider.name());
                return provider.fetch_time(timezone)
                    .map_err(|e| format!("{}: {}", provider.name(), e));
            }
        }

        // Auto mode - fallback chain
        println!("[CHAIN] Auto mode: trying all providers");
        let mut errors = Vec::new();

        for provider in &self.providers {
            match provider.fetch_time(timezone) {
                Ok(info) => {
                    println!("[CHAIN] Success with {}", provider.name());
                    return Ok(info);
                }
                Err(e) => {
                    println!("[CHAIN] {} failed: {}", provider.name(), e);
                    errors.push(format!("{}: {}", provider.name(), e));
                }
            }
        }

        Err(format!("All providers failed:\n{}", errors.join("\n")))
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
                    if let Ok(new_offset) = calculate_offset(timezone) {
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
        let offset = calculate_offset(&timezone)?;
        println!("[CLOCK] Offset calculated: {} minutes", offset);
        let mut state = self.state.lock().unwrap();
        state.source = TimeSource::Network { timezone, offset_min: offset };
        state.last_sync = Instant::now();
        Ok(())
    }



    fn start_loop(hwnd: HWND) {
        let hwnd_val = hwnd.0 as isize;
        thread::spawn(move || {
            while !SHOULD_STOP.load(Ordering::Relaxed) {
                app().clock.tick();
                win::invalidate(HWND(hwnd_val as _));
                thread::sleep(Duration::from_secs(1));
            }
            println!("[CLOCK] Thread stopped cleanly");
        });
    }
}

fn get_local_time() -> (u16, u16, u16) {
    let st = unsafe { GetLocalTime() };
    (st.wHour, st.wMinute, st.wSecond)
}

fn calculate_offset(timezone: &str) -> StdResult<i32, String> {
    let info = app().providers.fetch_time(timezone)?;
    let (lh, lm, _) = get_local_time();

    let remote = info.hour as i32 * 60 + info.minute as i32;
    let local = lh as i32 * 60 + lm as i32;
    let mut diff = remote - local;

    if diff > 720 { diff -= 1440; }
    else if diff < -720 { diff += 1440; }

    Ok(diff)
}

// ═══════════════════════════════════════════════════════════════════
// HTTP Helper
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

fn http_get(host: &str, port: u16, path: &str) -> StdResult<String, String> {
    unsafe {
        let host_wide: Vec<u16> = host.encode_utf16().chain(Some(0)).collect();
        let path_wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();

        let session = WinHttpHandle::new(
            WinHttpOpen(w!("MinimalClock/2.0"),
                        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                        PCWSTR::null(), PCWSTR::null(), 0)
        ).ok_or("WinHttpOpen failed")?;

        let conn = WinHttpHandle::new(
            WinHttpConnect(session.ptr(),
                           PCWSTR(host_wide.as_ptr()),
                           port, 0)
        ).ok_or("WinHttpConnect failed")?;

        let req = WinHttpHandle::new(
            WinHttpOpenRequest(conn.ptr(), w!("GET"),
                               PCWSTR(path_wide.as_ptr()),
                               PCWSTR::null(), PCWSTR::null(),
                               ptr::null(), WINHTTP_FLAG_SECURE)
        ).ok_or("WinHttpOpenRequest failed")?;

        WinHttpSendRequest(req.ptr(), None, None, 0, 0, 0)
            .map_err(|e| format!("Send failed: 0x{:08X}", e.code().0))?;

        WinHttpReceiveResponse(req.ptr(), ptr::null_mut())
            .map_err(|e| format!("Receive failed: 0x{:08X}", e.code().0))?;

        let mut buf = vec![0u8; 4096];
        let mut read = 0u32;
        let mut data = Vec::new();

        while WinHttpReadData(req.ptr(), buf.as_mut_ptr().cast(),
                              buf.len() as u32, &mut read).is_ok() && read > 0 {
            data.extend_from_slice(&buf[..read as usize]);
            read = 0;
        }

        Ok(String::from_utf8_lossy(&data).into_owned())
    }
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let start = json.find(key)? + key.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_json_int(json: &str, key: &str) -> Option<i32> {
    let start = json.find(key)? + key.len();
    let rest = &json[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    if end == 0 { return None; }
    rest[..end].parse().ok()
}

fn extract_json_int_flexible(json: &str, field_name: &str) -> Option<i32> {
    // Try both "field": and "field" : (with spaces)
    let pattern1 = format!("\"{}\":", field_name);
    let pattern2 = format!("\"{}\": ", field_name);
    let pattern3 = format!("\"{}\"\r\n:", field_name);

    for pattern in &[pattern1, pattern2, pattern3] {
        if let Some(start) = json.find(pattern) {
            let rest = &json[start + pattern.len()..];
            // Skip whitespace and newlines
            let trimmed = rest.trim_start();
            let end = trimmed.find(|c: char| !c.is_ascii_digit() && c != '-')
                .unwrap_or(trimmed.len());
            if end > 0 {
                if let Ok(val) = trimmed[..end].parse() {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn parse_time_string(time: &str) -> StdResult<TimeInfo, String> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() < 2 {
        return Err("Invalid time format".into());
    }

    Ok(TimeInfo {
        hour: parts[0].parse().map_err(|_| "Invalid hour")?,
        minute: parts[1].parse().map_err(|_| "Invalid minute")?,
    })
}

// ═══════════════════════════════════════════════════════════════════
// Windows API Wrappers
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

    pub fn create_control(parent: HWND, class: &str, text: &str, style: u32,
                          x: i32, y: i32, w: i32, h: i32, id: i32) -> HWND {
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

    pub fn message_box(hwnd: HWND, text: &str, title: &str, flags: u32) {
        let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let h: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(hwnd, PCWSTR(t.as_ptr()), PCWSTR(h.as_ptr()),
                        MESSAGEBOX_STYLE(flags));
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
        Self(unsafe {
            CreateFontW(size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 0, 0,
                        PCWSTR(wide.as_ptr()))
        })
    }
    fn handle(&self) -> HGDIOBJ { HGDIOBJ(self.0.0) }
}
impl Drop for GdiFont {
    fn drop(&mut self) {
        unsafe { let _ = DeleteObject(HGDIOBJ(self.0.0)); }
    }
}

struct GdiBrush(HBRUSH);
impl GdiBrush {
    fn solid(color: u32) -> Self {
        Self(unsafe { CreateSolidBrush(COLORREF(color)) })
    }
    fn handle(&self) -> HBRUSH { self.0 }
}
impl Drop for GdiBrush {
    fn drop(&mut self) {
        unsafe { let _ = DeleteObject(HGDIOBJ(self.0.0)); }
    }
}

// ═══════════════════════════════════════════════════════════════════
// UI Helpers
// ═══════════════════════════════════════════════════════════════════

fn populate_combo_with_selection(hwnd: HWND, timezones: &[String], selected_tz: Option<&str>) {
    win::send_message(hwnd, CB_RESETCONTENT, 0, 0);

    let add = |s: &str| {
        let w: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
        win::send_message(hwnd, CB_ADDSTRING, 0, w.as_ptr() as _);
    };

    // NO "Local" in combo anymore!
    for tz in timezones {
        add(tz);
    }

    // Find correct index (no +1 offset now)
    let selected_index = if let Some(tz) = selected_tz {
        timezones.iter()
            .position(|t| t == tz)
            .unwrap_or(0)  // Default to first timezone
    } else {
        0  // Default to first timezone if no selection
    };

    win::send_message(hwnd, CB_SETCURSEL, selected_index, 0);
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
    // Phase 1: Clone ALL needed data upfront
    let (timezones, last_tz) = {
        let config = app().config.lock().unwrap();
        (config.timezones.clone(), config.last_selected_timezone.clone())
    };

    let provider_names = app().providers.provider_names();
    let active_provider_index = app().providers.get_active_index();

    // Phase 2: Create all controls (NO "Fetch" button anymore!)
    let combo = win::create_control(hwnd, "COMBOBOX", "",
                                    CBS_DROPDOWNLIST | CBS_HASSTRINGS,
                                    10, 10, 200, 300,  // ← Высота 300 = 15 элементов
                                    ID_COMBO);

    let edit = win::create_control(hwnd, "EDIT", "", 0, 220, 10, 150, 25, ID_EDIT);
    win::create_control(hwnd, "BUTTON", "+", 0, 380, 10, 30, 25, ID_ADD);

    let provider_combo = win::create_control(hwnd, "COMBOBOX", "",
                                             CBS_DROPDOWNLIST | CBS_HASSTRINGS,
                                             420, 10, 180, 150,  // ← 150 = ~7 элементов
                                             ID_PROVIDER_COMBO);
    // Phase 3: Populate timezone combo with correct selection
    populate_combo_with_selection(combo, &timezones, last_tz.as_deref());

    println!("[INIT] Set combo selection to {}",
             last_tz.as_ref().map(|s| s.as_str()).unwrap_or("<none>"));

    // Phase 4: Populate provider combo
    win::send_message(provider_combo, CB_RESETCONTENT, 0, 0);
    for name in &provider_names {
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        win::send_message(provider_combo, CB_ADDSTRING, 0, w.as_ptr() as _);
    }
    win::send_message(provider_combo, CB_SETCURSEL, active_provider_index, 0);

    // Phase 5: Store UI handles
    *app().ui.lock().unwrap() = UiHandles { combo, edit, provider_combo };

    // Phase 6: Start clock
    Clock::start_loop(hwnd);

    // Phase 7: Restore timezone asynchronously
    if let Some(tz) = last_tz {
        let hwnd_val = hwnd.0 as isize;
        thread::spawn(move || {
            let hwnd = HWND(hwnd_val as _);

            // Give UI time to render + wait for user interaction window
            thread::sleep(Duration::from_millis(500));

            // Check if user already changed timezone
            let needs_fetch = {
                let state = app().clock.state.lock().unwrap();
                !matches!(&state.source, TimeSource::Network { timezone, .. } if timezone == &tz)
            };

            if needs_fetch {
                if let Ok(_) = app().clock.switch_to_network(tz.clone()) {
                    println!("[INIT] Restored timezone: {}", tz);
                }
            } else {
                println!("[INIT] Already on timezone: {}", tz);
            }

            win::invalidate(hwnd);
        });
    }
}

fn handle_command(hwnd: HWND, id: i32, notify_code: i32) {
    let ui = app().ui.lock().unwrap();

    match id {
        ID_COMBO => {
            if notify_code == CBN_SELCHANGE {
                // Auto-fetch on selection change
                let (_idx, text) = get_combo_selection(ui.combo);
                drop(ui);

                app().clock.set_display("Loading...".into());
                win::invalidate(hwnd);

                let hwnd_val = hwnd.0 as isize;
                thread::spawn(move || {
                    let hwnd = HWND(hwnd_val as _);

                    match app().clock.switch_to_network(text.clone()) {
                        Ok(_) => {
                            let mut config = app().config.lock().unwrap();
                            config.set_last_timezone(Some(text));
                            println!("[COMBO] Auto-fetched on selection change");
                        }
                        Err(e) => {
                            app().clock.set_display(format!("Error: {}", e));
                            println!("[COMBO] Failed: {}", e);
                        }
                    }

                    win::invalidate(hwnd);
                });
            }
        }
        ID_ADD => {
            let tz = win::get_window_text(ui.edit).trim().to_string();
            drop(ui);

            if tz.is_empty() { return; }

            let add_result = {
                let mut config = app().config.lock().unwrap();
                config.add_timezone(tz.clone())
            };

            match add_result {
                Ok(_) => {
                    let current_tz = {
                        let state = app().clock.state.lock().unwrap();
                        match &state.source {
                            TimeSource::Local => None,
                            TimeSource::Network { timezone, .. } => Some(timezone.clone()),
                        }
                    };

                    let (timezones, combo_hwnd, edit_hwnd) = {
                        let config = app().config.lock().unwrap();
                        let ui = app().ui.lock().unwrap();
                        (config.timezones.clone(), ui.combo, ui.edit)
                    };

                    populate_combo_with_selection(combo_hwnd, &timezones, current_tz.as_deref());
                    win::set_window_text(edit_hwnd, "");

                    println!("[ADD] Added timezone, preserved selection: {:?}", current_tz);
                }
                Err(e) => {
                    win::message_box(hwnd, &e, "Invalid Timezone", 0x30);
                }
            }
        }
        ID_PROVIDER_COMBO => {
            if notify_code == CBN_SELCHANGE {
                let (idx, name) = get_combo_selection(ui.provider_combo);
                drop(ui);

                app().providers.set_active_provider(idx);
                println!("[UI] Provider switched to: {}", name);
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

        let bg = GdiBrush::solid(0x00F0F0F0);
        FillRect(hdc, &rc, bg.handle());

        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00202020));

        let (font_size, font_name) = {
            let config = app().config.lock().unwrap();
            let size = config.window.font_size.unwrap_or_else(|| {
                let min_dim = (rc.right - rc.left).min(rc.bottom - rc.top);
                (min_dim / 5).clamp(24, 200)
            });
            (size, config.window.font_name.clone())
        };

        let font = GdiFont::new(font_size, 700, &font_name);
        let old = SelectObject(hdc, font.handle());

        let text = app().clock.display();
        let mut wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut text_rc = RECT { top: 50, ..rc };
        DrawTextW(hdc, &mut wide, &mut text_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        SelectObject(hdc, old);

        let hint_font = GdiFont::new(11, 400, "Segoe UI");
        let old2 = SelectObject(hdc, hint_font.handle());
        SetTextColor(hdc, COLORREF(0x00808080));

        let hint = "Click clock to toggle Local/Timezone | Add IANA format (Asia/Bangkok)";
        let mut hw: Vec<u16> = hint.encode_utf16().chain(Some(0)).collect();
        let mut hint_rc = RECT { top: rc.bottom - 30, ..rc };
        DrawTextW(hdc, &mut hw, &mut hint_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        SelectObject(hdc, old2);
        EndPaint(hwnd, &ps);
    }
}

fn handle_lbutton_down(hwnd: HWND) {
    // Toggle between Local and last network timezone
    let current_state = {
        let state = app().clock.state.lock().unwrap();
        state.source.clone()
    };

    match current_state {
        TimeSource::Local => {
            // Switch to last network timezone
            let last_tz = {
                let config = app().config.lock().unwrap();
                config.last_selected_timezone.clone()
            };

            if let Some(tz) = last_tz {
                println!("[CLICK] Switching Local -> {}", tz);
                app().clock.set_display("Loading...".into());
                win::invalidate(hwnd);

                let hwnd_val = hwnd.0 as isize;
                thread::spawn(move || {
                    let hwnd = HWND(hwnd_val as _);

                    if let Err(e) = app().clock.switch_to_network(tz.clone()) {
                        app().clock.set_display(format!("Error: {}", e));
                        println!("[CLICK] Failed: {}", e);
                    }

                    win::invalidate(hwnd);
                });
            } else {
                println!("[CLICK] No saved timezone to switch to");
            }
        }
        TimeSource::Network { timezone, .. } => {
            // Switch to Local (instant, no fetch needed)
            println!("[CLICK] Switching {} -> Local", timezone);
            app().clock.switch_to_local();
            win::invalidate(hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            handle_create(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as i32;
            let notify_code = ((wp.0 >> 16) & 0xFFFF) as i32;
            handle_command(hwnd, id, notify_code);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            handle_lbutton_down(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            handle_paint(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            println!("[UI] Window closing");
            SHOULD_STOP.store(true, Ordering::Relaxed);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp)
    }
}
fn main() -> Result<()> {
    println!("=== Minimal Clock v2.0 (Fallback Providers) ===");

    let config = Config::load();
    let win_cfg = config.window.clone();
    let providers = ProviderChain::from_config(&config);

    let state = Arc::new(AppState {
        clock: Clock::new(),
        config: Mutex::new(config),
        ui: Mutex::new(UiHandles::default()),
        providers,
    });
    let _ = APP_STATE.set(state);

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

        println!("[UI] Creating window {}x{} at ({}, {})",
                 win_cfg.width, win_cfg.height, x, y);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Minimal Clock v2.0"),
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