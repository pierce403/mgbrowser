//! Modern Web Platform wire contracts between the parent browser and Boa page worker.
//! Absent from strict legacy v1 frames and omitted when empty on modern frames.
use serde::{Deserialize, Serialize};

pub const MAX_STORAGE_QUOTA_BYTES: usize = 512 * 1024;
pub const MAX_STORAGE_OPS_PER_TX: usize = 256;
pub const MAX_COOKIE_WRITES_PER_TX: usize = 64;
pub const MAX_COOKIE_WRITE_BYTES: usize = 4096;
pub const MAX_HISTORY_OPS_PER_TX: usize = 64;
pub const MAX_HISTORY_URL_BYTES: usize = 16 * 1024;
pub const MAX_HISTORY_STATE_BYTES: usize = 256 * 1024;
pub const MAX_FETCH_REQUESTS_PER_TX: usize = 16;
pub const MAX_FETCH_HEADERS: usize = 64;
pub const MAX_FETCH_HEADER_BYTES: usize = 8192;
pub const MAX_FETCH_URL_BYTES: usize = 16 * 1024;
pub const MAX_FETCH_REQ_BODY_B64_BYTES: usize = 96 * 1024;
pub const MAX_FETCH_RESP_BODY_B64_BYTES: usize = 12 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewportState {
    pub inner_width: u32,
    pub inner_height: u32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub device_pixel_ratio: f32,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            inner_width: 1024,
            inner_height: 768,
            scroll_x: 0.0,
            scroll_y: 0.0,
            device_pixel_ratio: 1.0,
        }
    }
}

impl ViewportState {
    pub fn validate(&self) -> Result<(), String> {
        if !self.scroll_x.is_finite()
            || !self.scroll_y.is_finite()
            || !self.device_pixel_ratio.is_finite()
            || self.device_pixel_ratio <= 0.0
        {
            return Err("Invalid viewport metrics".into());
        }
        Ok(())
    }
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformInit {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_storage: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_storage: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub initial_cookies: String,
    #[serde(default)]
    pub viewport: ViewportState,
    #[serde(default)]
    pub history_length: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_state_json: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub time_origin_ms: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automation_only: bool,
}

impl PlatformInit {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        self.viewport.validate()?;
        let local_bytes: usize = self
            .local_storage
            .iter()
            .map(|(k, v)| k.len().saturating_add(v.len()))
            .sum();
        let session_bytes: usize = self
            .session_storage
            .iter()
            .map(|(k, v)| k.len().saturating_add(v.len()))
            .sum();
        if local_bytes > MAX_STORAGE_QUOTA_BYTES || session_bytes > MAX_STORAGE_QUOTA_BYTES {
            return Err("Initial storage exceeds 512 KiB quota".into());
        }
        if self.initial_cookies.len() > 32 * 1024 {
            return Err("Initial cookie snapshot exceeds limit".into());
        }
        if self
            .history_state_json
            .as_ref()
            .is_some_and(|s| s.len() > MAX_HISTORY_STATE_BYTES)
        {
            return Err("Initial history state exceeds limit".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum StorageOp {
    Set { key: String, value: String },
    Remove { key: String },
    Clear,
}

impl StorageOp {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Set { key, value } = self
            && key.len().saturating_add(value.len()) > MAX_STORAGE_QUOTA_BYTES
        {
            return Err("Storage entry exceeds 512 KiB quota".into());
        }
        if let Self::Remove { key } = self
            && key.len() > MAX_STORAGE_QUOTA_BYTES
        {
            return Err("Storage key exceeds 512 KiB quota".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum HistoryOp {
    PushState {
        url: String,
        state_id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_json: Option<String>,
    },
    ReplaceState {
        url: String,
        state_id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_json: Option<String>,
    },
    Go {
        delta: i32,
    },
}

impl HistoryOp {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::PushState {
                url, state_json, ..
            }
            | Self::ReplaceState {
                url, state_json, ..
            } => {
                if url.is_empty() || url.len() > MAX_HISTORY_URL_BYTES {
                    return Err("History URL exceeds limit or is empty".into());
                }
                if state_json
                    .as_ref()
                    .is_some_and(|s| s.len() > MAX_HISTORY_STATE_BYTES)
                {
                    return Err("History state exceeds limit".into());
                }
                Ok(())
            }
            Self::Go { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptFetchRequest {
    pub request_id: u64,
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_b64: Option<String>,
}

impl ScriptFetchRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.request_id == 0
            || self.method.is_empty()
            || self.method.len() > 32
            || self.url.is_empty()
            || self.url.len() > MAX_FETCH_URL_BYTES
            || self.headers.len() > MAX_FETCH_HEADERS
        {
            return Err("Invalid script fetch request metadata".into());
        }
        for (name, value) in &self.headers {
            if name.is_empty()
                || name.len() > MAX_FETCH_HEADER_BYTES
                || value.len() > MAX_FETCH_HEADER_BYTES
            {
                return Err("Invalid script fetch request header".into());
            }
        }
        if self
            .body_b64
            .as_ref()
            .is_some_and(|b| b.len() > MAX_FETCH_REQ_BODY_B64_BYTES)
        {
            return Err("Script fetch request body exceeds limit".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptFetchResponse {
    pub request_id: u64,
    pub ok_transport: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub status: u16,
    pub status_text: String,
    pub final_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_b64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_cookies: Option<String>,
}

impl ScriptFetchResponse {
    pub fn network_error(request_id: u64, error: impl Into<String>) -> Self {
        let mut msg = error.into();
        if msg.len() > 2048 {
            msg.truncate(2048);
        }
        Self {
            request_id,
            ok_transport: false,
            error: Some(msg),
            status: 0,
            status_text: String::new(),
            final_url: String::new(),
            headers: Vec::new(),
            body_b64: String::new(),
            updated_cookies: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.request_id == 0
            || self.status_text.len() > 512
            || self.final_url.len() > MAX_FETCH_URL_BYTES
            || self.headers.len() > 128
            || self.body_b64.len() > MAX_FETCH_RESP_BODY_B64_BYTES
        {
            return Err("Invalid script fetch response".into());
        }
        if self.error.as_ref().is_some_and(|e| e.len() > 2048) {
            return Err("Fetch error diagnostic exceeds limit".into());
        }
        Ok(())
    }
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// RFC 4648 standard Base64 encoding (used for `btoa` and wire `body_b64`).
pub fn base64_encode(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(B64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(B64_ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// WHATWG forgiving-base64 decode (used for `atob` and wire `body_b64`).
pub fn base64_decode_forgiving(input: &str) -> Result<Vec<u8>, &'static str> {
    let mut clean: Vec<u8> = input
        .bytes()
        .filter(|b| !matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' '))
        .collect();
    if clean.len() % 4 == 0 {
        if clean.ends_with(b"==") {
            clean.truncate(clean.len() - 2);
        } else if clean.ends_with(b"=") {
            clean.pop();
        }
    }
    if clean.len() % 4 == 1 {
        return Err("The string to be decoded is not correctly encoded.");
    }
    if clean.iter().any(|&b| b64_val(b).is_none()) {
        return Err("The string to be decoded contains characters outside of the Base64 range.");
    }
    let mut out = Vec::with_capacity((clean.len() * 3) / 4);
    for chunk in clean.chunks(4) {
        let v0 = b64_val(chunk[0]).unwrap();
        let v1 = b64_val(chunk[1]).unwrap();
        out.push((v0 << 2) | (v1 >> 4));
        if chunk.len() > 2 {
            let v2 = b64_val(chunk[2]).unwrap();
            out.push(((v1 & 0x0f) << 4) | (v2 >> 2));
            if chunk.len() > 3 {
                let v3 = b64_val(chunk[3]).unwrap();
                out.push(((v2 & 0x03) << 6) | v3);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ScrollRequest {
    Absolute {
        x: f32,
        y: f32,
    },
    By {
        dx: f32,
        dy: f32,
    },
    IntoView {
        node: usize,
        align_top: bool,
    },
    FocusControl {
        node: Option<usize>,
        select_all: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        into_view: Option<(usize, bool)>,
    },
}

impl ScrollRequest {
    pub fn validate(&self) -> Result<(), String> {
        match *self {
            Self::Absolute { x, y } if !x.is_finite() || !y.is_finite() => {
                Err("Non-finite scroll coordinates".into())
            }
            Self::By { dx, dy } if !dx.is_finite() || !dy.is_finite() => {
                Err("Non-finite relative scroll delta".into())
            }
            Self::FocusControl {
                node, into_view, ..
            } if node.is_some_and(|n| n >= 50_000)
                || into_view.is_some_and(|(n, _)| n >= 50_000) =>
            {
                Err("Invalid focus control node".into())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum PlatformCommand {
    FetchComplete {
        response: ScriptFetchResponse,
    },
    HistoryTraverse {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_id: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_json: Option<String>,
        history_length: usize,
    },
    ViewportSync {
        viewport: ViewportState,
    },
    SelectOption {
        select_node: usize,
        selected_index: i32,
    },
}

impl PlatformCommand {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::FetchComplete { response } => response.validate(),
            Self::HistoryTraverse {
                url, state_json, ..
            } => {
                if url.is_empty() || url.len() > MAX_HISTORY_URL_BYTES {
                    return Err("History traversal URL exceeds limit or is empty".into());
                }
                if state_json
                    .as_ref()
                    .is_some_and(|s| s.len() > MAX_HISTORY_STATE_BYTES)
                {
                    return Err("History traversal state exceeds limit".into());
                }
                Ok(())
            }
            Self::ViewportSync { viewport } => viewport.validate(),
            Self::SelectOption { selected_index, .. } => {
                if *selected_index < -1 || *selected_index > 100_000 {
                    return Err("Select option index out of bounds".into());
                }
                Ok(())
            }
        }
    }
}
