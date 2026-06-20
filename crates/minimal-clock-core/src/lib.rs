use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::ptr;
use std::time::{Duration, Instant};
use windows::{
    core::*, Win32::Networking::WinHttp::*, Win32::System::SystemInformation::GetLocalTime,
};

pub type ClockResult<T> = std::result::Result<T, String>;

pub const DEFAULT_NETWORK_REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub priority: u32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClockCoreConfig {
    #[serde(default = "ClockCoreConfig::default_timezones")]
    pub timezones: Vec<String>,
    #[serde(default = "ClockCoreConfig::default_providers")]
    pub providers: Vec<ProviderConfig>,
    #[serde(default = "ClockCoreConfig::default_iana_timezones")]
    pub iana_timezones: Vec<String>,
    #[serde(default)]
    pub last_selected_timezone: Option<String>,
}

impl Default for ClockCoreConfig {
    fn default() -> Self {
        Self {
            timezones: Self::default_timezones(),
            providers: Self::default_providers(),
            iana_timezones: Self::default_iana_timezones(),
            last_selected_timezone: None,
        }
    }
}

impl ClockCoreConfig {
    pub fn default_timezones() -> Vec<String> {
        vec![
            "Europe/Moscow".into(),
            "Europe/London".into(),
            "America/New_York".into(),
            "Asia/Tokyo".into(),
            "Asia/Bangkok".into(),
            "Asia/Hong_Kong".into(),
        ]
    }

    pub fn default_providers() -> Vec<ProviderConfig> {
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

    pub fn default_iana_timezones() -> Vec<String> {
        vec![
            "Africa/Cairo",
            "Africa/Johannesburg",
            "Africa/Lagos",
            "Africa/Nairobi",
            "America/Anchorage",
            "America/Argentina/Buenos_Aires",
            "America/Chicago",
            "America/Denver",
            "America/Detroit",
            "America/Los_Angeles",
            "America/Mexico_City",
            "America/New_York",
            "America/Phoenix",
            "America/Sao_Paulo",
            "America/Toronto",
            "America/Vancouver",
            "Asia/Bangkok",
            "Asia/Colombo",
            "Asia/Dubai",
            "Asia/Hong_Kong",
            "Asia/Jakarta",
            "Asia/Jerusalem",
            "Asia/Karachi",
            "Asia/Kolkata",
            "Asia/Manila",
            "Asia/Riyadh",
            "Asia/Seoul",
            "Asia/Shanghai",
            "Asia/Singapore",
            "Asia/Taipei",
            "Asia/Tokyo",
            "Atlantic/Reykjavik",
            "Australia/Adelaide",
            "Australia/Brisbane",
            "Australia/Melbourne",
            "Australia/Perth",
            "Australia/Sydney",
            "Europe/Amsterdam",
            "Europe/Athens",
            "Europe/Berlin",
            "Europe/Brussels",
            "Europe/Budapest",
            "Europe/Copenhagen",
            "Europe/Dublin",
            "Europe/Helsinki",
            "Europe/Istanbul",
            "Europe/Lisbon",
            "Europe/London",
            "Europe/Madrid",
            "Europe/Moscow",
            "Europe/Oslo",
            "Europe/Paris",
            "Europe/Prague",
            "Europe/Rome",
            "Europe/Stockholm",
            "Europe/Vienna",
            "Europe/Warsaw",
            "Europe/Zurich",
            "Pacific/Auckland",
            "Pacific/Fiji",
            "Pacific/Honolulu",
            "UTC",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect()
    }

    pub fn normalize(&mut self) {
        if self.timezones.is_empty() {
            self.timezones = Self::default_timezones();
        }

        if self.providers.is_empty() {
            self.providers = Self::default_providers();
        }

        let default_iana = Self::default_iana_timezones();
        if self.iana_timezones.len() != default_iana.len() {
            self.iana_timezones = default_iana;
        }
    }

    pub fn find_timezone(&self, query: &str) -> Option<String> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }

        let query_lower = query.to_lowercase();

        if let Some(timezone) = self
            .iana_timezones
            .iter()
            .find(|timezone| timezone.to_lowercase() == query_lower)
        {
            return Some(timezone.clone());
        }

        if let Some(timezone) = self.iana_timezones.iter().find(|timezone| {
            timezone
                .rsplit('/')
                .next()
                .map(|city| city.to_lowercase() == query_lower)
                .unwrap_or(false)
        }) {
            return Some(timezone.clone());
        }

        if let Some(timezone) = self.iana_timezones.iter().find(|timezone| {
            timezone
                .rsplit('/')
                .next()
                .map(|city| city.to_lowercase().contains(&query_lower))
                .unwrap_or(false)
        }) {
            return Some(timezone.clone());
        }

        self.iana_timezones
            .iter()
            .find(|timezone| timezone.to_lowercase().contains(&query_lower))
            .cloned()
    }

    pub fn add_timezone(&mut self, query: &str) -> ClockResult<String> {
        let matched = self.find_timezone(query).ok_or_else(|| {
            let suggestions: Vec<String> = self
                .iana_timezones
                .iter()
                .filter(|timezone| {
                    let query_lower = query.to_lowercase();
                    timezone.to_lowercase().contains(&query_lower)
                        || timezone
                            .rsplit('/')
                            .next()
                            .map(|city| city.to_lowercase().contains(&query_lower))
                            .unwrap_or(false)
                })
                .take(5)
                .cloned()
                .collect();

            if suggestions.is_empty() {
                format!("Timezone '{}' not found. Use IANA format.", query)
            } else {
                format!(
                    "Timezone '{}' not found. Did you mean:\n{}",
                    query,
                    suggestions.join("\n")
                )
            }
        })?;

        if self.timezones.contains(&matched) {
            return Err(format!("Timezone '{}' already exists", matched));
        }

        self.timezones.push(matched.clone());
        Ok(matched)
    }
}

#[derive(Clone, Debug)]
pub enum ClockSource {
    Local,
    Network { timezone: String, offset_min: i32 },
}

#[derive(Clone, Debug)]
pub struct ClockSnapshot {
    pub ok: bool,
    pub label: String,
    pub text: String,
    pub tooltip: String,
    pub source: String,
    pub timezone: Option<String>,
    pub provider: Option<String>,
    pub error: Option<String>,
}

impl ClockSnapshot {
    fn error(message: String) -> Self {
        Self {
            ok: false,
            label: "Clock".into(),
            text: "Clock error".into(),
            tooltip: message.clone(),
            source: "error".into(),
            timezone: None,
            provider: None,
            error: Some(message),
        }
    }
}

#[derive(Clone, Debug)]
struct TimeInfo {
    hour: u8,
    minute: u8,
}

#[derive(Clone, Debug)]
struct ProviderTimeInfo {
    provider_name: String,
    info: TimeInfo,
}

trait TimeProvider: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_time(&self, timezone: &str) -> ClockResult<TimeInfo>;
}

pub struct ClockCore {
    config: ClockCoreConfig,
    providers: ProviderChain,
    source: ClockSource,
    last_sync: Instant,
    last_provider: Option<String>,
    network_refresh_interval: Duration,
}

impl ClockCore {
    pub fn new(mut config: ClockCoreConfig) -> Self {
        config.normalize();
        let providers = ProviderChain::from_config(&config);
        Self {
            config,
            providers,
            source: ClockSource::Local,
            last_sync: Instant::now(),
            last_provider: None,
            network_refresh_interval: DEFAULT_NETWORK_REFRESH_INTERVAL,
        }
    }

    pub fn config(&self) -> &ClockCoreConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut ClockCoreConfig {
        &mut self.config
    }

    pub fn source(&self) -> &ClockSource {
        &self.source
    }

    pub fn provider_names(&self) -> Vec<String> {
        self.providers.provider_names()
    }

    pub fn active_provider_index(&self) -> usize {
        self.providers.active_index()
    }

    pub fn set_active_provider_index(&mut self, index: usize) {
        self.providers.set_active_provider(index);
    }

    pub fn set_network_refresh_interval(&mut self, interval: Duration) {
        self.network_refresh_interval = interval;
    }

    pub fn add_timezone(&mut self, query: &str) -> ClockResult<String> {
        self.config.add_timezone(query)
    }

    pub fn switch_to_local(&mut self) -> ClockSnapshot {
        self.source = ClockSource::Local;
        self.tick()
    }

    pub fn switch_to_last_timezone(&mut self) -> ClockResult<ClockSnapshot> {
        let timezone = self
            .config
            .last_selected_timezone
            .clone()
            .ok_or_else(|| "No saved timezone".to_string())?;
        self.switch_to_timezone(timezone)
    }

    pub fn switch_to_timezone(&mut self, timezone: String) -> ClockResult<ClockSnapshot> {
        let offset = self.calculate_offset(&timezone)?;
        self.source = ClockSource::Network {
            timezone: timezone.clone(),
            offset_min: offset,
        };
        self.config.last_selected_timezone = Some(timezone);
        self.last_sync = Instant::now();
        Ok(self.tick())
    }

    pub fn refresh_offset(&mut self) -> ClockResult<()> {
        let timezone = match self.source.clone() {
            ClockSource::Local => return Ok(()),
            ClockSource::Network { timezone, .. } => timezone,
        };

        let offset = self.calculate_offset(&timezone)?;
        self.source = ClockSource::Network {
            timezone,
            offset_min: offset,
        };
        self.last_sync = Instant::now();
        Ok(())
    }

    pub fn tick(&mut self) -> ClockSnapshot {
        if matches!(self.source, ClockSource::Network { .. })
            && self.last_sync.elapsed() > self.network_refresh_interval
        {
            if let Err(error) = self.refresh_offset() {
                return ClockSnapshot::error(error);
            }
        }

        let (hour, minute, second) = local_time();
        match self.source.clone() {
            ClockSource::Local => ClockSnapshot {
                ok: true,
                label: "Local".into(),
                text: format!("Local {:02}:{:02}:{:02}", hour, minute, second),
                tooltip: "Local system time".into(),
                source: "local".into(),
                timezone: None,
                provider: None,
                error: None,
            },
            ClockSource::Network {
                timezone,
                offset_min,
            } => {
                let total = (hour as i32 * 60 + minute as i32 + offset_min).rem_euclid(1440);
                let remote_hour = total / 60;
                let remote_minute = total % 60;
                let label = timezone_label(&timezone);
                let provider = self.last_provider.clone();
                ClockSnapshot {
                    ok: true,
                    label: label.clone(),
                    text: format!(
                        "{} {:02}:{:02}:{:02}",
                        label, remote_hour, remote_minute, second
                    ),
                    tooltip: format!(
                        "{}{}",
                        timezone,
                        provider
                            .as_ref()
                            .map(|provider| format!(" via {}", provider))
                            .unwrap_or_default()
                    ),
                    source: "network".into(),
                    timezone: Some(timezone),
                    provider,
                    error: None,
                }
            }
        }
    }

    fn calculate_offset(&mut self, timezone: &str) -> ClockResult<i32> {
        let provider_time = self.providers.fetch_time(timezone)?;
        self.last_provider = Some(provider_time.provider_name);

        let (local_hour, local_minute, _) = local_time();
        let remote = provider_time.info.hour as i32 * 60 + provider_time.info.minute as i32;
        let local = local_hour as i32 * 60 + local_minute as i32;
        let mut diff = remote - local;

        if diff > 720 {
            diff -= 1440;
        } else if diff < -720 {
            diff += 1440;
        }

        Ok(diff)
    }
}

struct ProviderChain {
    providers: Vec<Box<dyn TimeProvider>>,
    active_index: Option<usize>,
}

impl ProviderChain {
    fn from_config(config: &ClockCoreConfig) -> Self {
        let mut providers: Vec<Box<dyn TimeProvider>> = Vec::new();
        let mut configs = config.providers.clone();
        configs.sort_by_key(|provider| provider.priority);

        for provider_config in configs {
            if !provider_config.enabled {
                continue;
            }

            let provider: Box<dyn TimeProvider> = match provider_config.provider_type.as_str() {
                "worldtimeapi" => Box::new(WorldTimeApiProvider),
                "timeapi_io" => Box::new(TimeApiIoProvider),
                "timezonedb" => Box::new(TimezoneDbProvider {
                    api_key: provider_config.api_key.clone(),
                }),
                _ => continue,
            };

            providers.push(provider);
        }

        Self {
            providers,
            active_index: None,
        }
    }

    fn provider_names(&self) -> Vec<String> {
        let mut names = vec!["Auto (fallback)".to_string()];
        names.extend(
            self.providers
                .iter()
                .map(|provider| provider.name().to_string()),
        );
        names
    }

    fn set_active_provider(&mut self, index: usize) {
        self.active_index = if index == 0 || index - 1 >= self.providers.len() {
            None
        } else {
            Some(index - 1)
        };
    }

    fn active_index(&self) -> usize {
        self.active_index.map(|index| index + 1).unwrap_or(0)
    }

    fn fetch_time(&self, timezone: &str) -> ClockResult<ProviderTimeInfo> {
        if let Some(index) = self.active_index {
            let provider = self
                .providers
                .get(index)
                .ok_or_else(|| "Selected provider is not available".to_string())?;
            return provider
                .fetch_time(timezone)
                .map(|info| ProviderTimeInfo {
                    provider_name: provider.name().to_string(),
                    info,
                })
                .map_err(|error| format!("{}: {}", provider.name(), error));
        }

        let mut errors = Vec::new();
        for provider in &self.providers {
            match provider.fetch_time(timezone) {
                Ok(info) => {
                    return Ok(ProviderTimeInfo {
                        provider_name: provider.name().to_string(),
                        info,
                    });
                }
                Err(error) => errors.push(format!("{}: {}", provider.name(), error)),
            }
        }

        Err(format!("All providers failed:\n{}", errors.join("\n")))
    }
}

struct WorldTimeApiProvider;

impl TimeProvider for WorldTimeApiProvider {
    fn name(&self) -> &str {
        "WorldTimeAPI"
    }

    fn fetch_time(&self, timezone: &str) -> ClockResult<TimeInfo> {
        let json = http_get(
            "worldtimeapi.org",
            443,
            &format!("/api/timezone/{}", timezone),
        )?;
        let datetime = extract_json_string(&json, "\"datetime\":\"")
            .ok_or_else(|| "datetime field not found".to_string())?;
        let time_part = datetime
            .split('T')
            .nth(1)
            .and_then(|value| value.split('+').next())
            .and_then(|value| value.split('-').next())
            .ok_or_else(|| "Invalid datetime format".to_string())?;
        parse_time_string(time_part)
    }
}

struct TimeApiIoProvider;

impl TimeProvider for TimeApiIoProvider {
    fn name(&self) -> &str {
        "TimeAPI.io"
    }

    fn fetch_time(&self, timezone: &str) -> ClockResult<TimeInfo> {
        let json = http_get(
            "timeapi.io",
            443,
            &format!("/api/time/current/zone?timeZone={}", timezone),
        )?;
        let hour =
            extract_json_int(&json, "\"hour\":").ok_or_else(|| "hour not found".to_string())?;
        let minute =
            extract_json_int(&json, "\"minute\":").ok_or_else(|| "minute not found".to_string())?;
        Ok(TimeInfo {
            hour: hour as u8,
            minute: minute as u8,
        })
    }
}

struct TimezoneDbProvider {
    api_key: String,
}

impl TimeProvider for TimezoneDbProvider {
    fn name(&self) -> &str {
        "TimezoneDB"
    }

    fn fetch_time(&self, timezone: &str) -> ClockResult<TimeInfo> {
        if self.api_key.is_empty() {
            return Err("API key not configured".into());
        }

        let json = http_get(
            "api.timezonedb.com",
            443,
            &format!(
                "/v2.1/get-time-zone?key={}&format=json&by=zone&zone={}",
                self.api_key, timezone
            ),
        )?;

        if json.contains("\"status\":\"FAILED\"") || json.contains("\"status\": \"FAILED\"") {
            let message = extract_json_string(&json, "\"message\":\"")
                .or_else(|| extract_json_string(&json, "\"message\": \""))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(format!("API error: {}", message));
        }

        if let Some(formatted) = extract_json_string(&json, "\"formatted\":\"")
            .or_else(|| extract_json_string(&json, "\"formatted\": \""))
        {
            let time_part = formatted
                .split(' ')
                .nth(1)
                .ok_or_else(|| "Invalid formatted time".to_string())?;
            return parse_time_string(time_part);
        }

        let gmt_offset = extract_json_int_flexible(&json, "gmtOffset")
            .ok_or_else(|| "gmtOffset not found".to_string())?;
        let timestamp = extract_json_int_flexible(&json, "timestamp")
            .ok_or_else(|| "timestamp not found".to_string())?;
        let local_seconds = (timestamp + gmt_offset).rem_euclid(86400);
        Ok(TimeInfo {
            hour: ((local_seconds / 3600) % 24) as u8,
            minute: ((local_seconds % 3600) / 60) as u8,
        })
    }
}

struct WinHttpHandle(*mut c_void);

impl WinHttpHandle {
    fn new(handle: *mut c_void) -> Option<Self> {
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }

    fn ptr(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn http_get(host: &str, port: u16, path: &str) -> ClockResult<String> {
    unsafe {
        let host_wide: Vec<u16> = host.encode_utf16().chain(Some(0)).collect();
        let path_wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();

        let session = WinHttpHandle::new(WinHttpOpen(
            w!("MinimalClockCore/0.1"),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))
        .ok_or_else(|| "WinHttpOpen failed".to_string())?;

        let connection = WinHttpHandle::new(WinHttpConnect(
            session.ptr(),
            PCWSTR(host_wide.as_ptr()),
            port,
            0,
        ))
        .ok_or_else(|| "WinHttpConnect failed".to_string())?;

        let request = WinHttpHandle::new(WinHttpOpenRequest(
            connection.ptr(),
            w!("GET"),
            PCWSTR(path_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))
        .ok_or_else(|| "WinHttpOpenRequest failed".to_string())?;

        WinHttpSendRequest(request.ptr(), None, None, 0, 0, 0)
            .map_err(|error| format!("Send failed: 0x{:08X}", error.code().0))?;
        WinHttpReceiveResponse(request.ptr(), ptr::null_mut())
            .map_err(|error| format!("Receive failed: 0x{:08X}", error.code().0))?;

        let mut buffer = vec![0u8; 4096];
        let mut read = 0u32;
        let mut data = Vec::new();

        while WinHttpReadData(
            request.ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut read,
        )
        .is_ok()
            && read > 0
        {
            data.extend_from_slice(&buffer[..read as usize]);
            read = 0;
        }

        Ok(String::from_utf8_lossy(&data).into_owned())
    }
}

fn local_time() -> (u16, u16, u16) {
    let system_time = unsafe { GetLocalTime() };
    (system_time.wHour, system_time.wMinute, system_time.wSecond)
}

fn timezone_label(timezone: &str) -> String {
    timezone
        .rsplit('/')
        .next()
        .unwrap_or(timezone)
        .replace('_', " ")
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
    let end = rest
        .find(|value: char| !value.is_ascii_digit() && value != '-')
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }

    rest[..end].parse().ok()
}

fn extract_json_int_flexible(json: &str, field_name: &str) -> Option<i32> {
    let patterns = [
        format!("\"{}\":", field_name),
        format!("\"{}\": ", field_name),
        format!("\"{}\"\r\n:", field_name),
    ];

    for pattern in patterns {
        if let Some(start) = json.find(&pattern) {
            let rest = &json[start + pattern.len()..];
            let trimmed = rest.trim_start();
            let end = trimmed
                .find(|value: char| !value.is_ascii_digit() && value != '-')
                .unwrap_or(trimmed.len());
            if end > 0 {
                if let Ok(value) = trimmed[..end].parse() {
                    return Some(value);
                }
            }
        }
    }

    None
}

fn parse_time_string(time: &str) -> ClockResult<TimeInfo> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() < 2 {
        return Err("Invalid time format".into());
    }

    Ok(TimeInfo {
        hour: parts[0].parse().map_err(|_| "Invalid hour".to_string())?,
        minute: parts[1].parse().map_err(|_| "Invalid minute".to_string())?,
    })
}
