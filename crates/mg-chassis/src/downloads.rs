//! Bounded download metadata, Content-Disposition parsing, and filename sanitization.
//!
//! Chassis holds only bounded in-memory `PendingDownload` payloads and metadata;
//! the platform host writes files safely into the user's Downloads directory.

pub const MAX_DOWNLOAD_HISTORY: usize = 64;
pub const MAX_DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadState {
    InProgress,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadEntry {
    pub id: u64,
    pub url: String,
    pub filename: String,
    pub saved_path: Option<String>,
    pub mime: String,
    pub bytes: usize,
    pub state: DownloadState,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PendingDownload {
    pub id: u64,
    pub url: String,
    pub suggested_filename: String,
    pub mime: String,
    pub body: Vec<u8>,
}

/// Parse an HTTP `Content-Disposition` header value, returning `(is_attachment, filename)`.
pub fn parse_content_disposition(header: &str) -> (bool, Option<String>) {
    let mut parts = header.split(';');
    let disp_type = parts
        .next()
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let is_attachment = disp_type == "attachment";
    let mut filename = None;
    let mut filename_ext = None;
    for raw_param in parts {
        let Some((k, v)) = raw_param.split_once('=') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if key == "filename*" {
            // RFC 5987 / 8187: UTF-8''percent-encoded
            let encoded = if let Some((_, after_lang)) = val.split_once("''") {
                after_lang
            } else {
                val.trim_matches('"')
            };
            let decoded = percent_decode_utf8(encoded);
            if !decoded.trim().is_empty() {
                filename_ext = Some(decoded);
            }
        } else if key == "filename" {
            let unquoted = val
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .map(|s| s.replace("\\\"", "\"").replace("\\\\", "\\"))
                .unwrap_or_else(|| val.to_owned());
            if !unquoted.trim().is_empty() {
                filename = Some(unquoted);
            }
        }
    }
    (is_attachment, filename_ext.or(filename))
}

fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// True when a top-level HTTP response should be downloaded rather than replacing the page DOM.
pub fn should_download_response(content_type: &str, content_disposition: Option<&str>) -> bool {
    if let Some(cd) = content_disposition {
        let (is_attachment, _) = parse_content_disposition(cd);
        if is_attachment {
            return true;
        }
    }
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() || mime.contains("html") || mime.starts_with("text/") {
        return false;
    }
    matches!(
        mime.as_str(),
        "application/pdf"
            | "application/zip"
            | "application/x-zip-compressed"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-tar"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
            | "application/wasm"
            | "application/msword"
            | "application/epub+zip"
    ) || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime.starts_with("font/")
}

/// Sanitize an untrusted filename into a safe, non-hidden basename (`<= 120` UTF-8 bytes).
pub fn sanitize_download_filename(
    preferred: Option<&str>,
    fallback_url: &str,
    mime: &str,
) -> String {
    let candidate = preferred
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            url::Url::parse(fallback_url).ok().and_then(|u| {
                u.path_segments()
                    .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
                    .map(percent_decode_utf8)
            })
        })
        .unwrap_or_default();

    // Strip any path separators (`/` and `\`) to take only the final basename.
    let base = candidate
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("");

    let mut clean = String::new();
    for ch in base.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            clean.push('_');
        } else {
            clean.push(ch);
        }
    }
    let trimmed = clean
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();

    let mut result = if trimmed.is_empty() {
        let host = url::Url::parse(fallback_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| "download".into());
        host.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
    } else {
        trimmed
    };

    if result.is_empty() {
        result = "download".into();
    }

    // Bound length to 108 bytes before optional extension.
    if result.len() > 108 {
        let mut cut = 108;
        while cut > 0 && !result.is_char_boundary(cut) {
            cut -= 1;
        }
        result.truncate(cut);
    }

    if !result.contains('.')
        && let Some(ext) = default_extension_for_mime(mime)
    {
        result.push('.');
        result.push_str(ext);
    }
    result
}

fn default_extension_for_mime(mime: &str) -> Option<&'static str> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "text/html" | "application/xhtml+xml" => Some("html"),
        "text/plain" => Some("txt"),
        "text/css" => Some("css"),
        "text/csv" => Some("csv"),
        "application/json" => Some("json"),
        "application/javascript" | "text/javascript" => Some("js"),
        "application/pdf" => Some("pdf"),
        "application/zip" | "application/x-zip-compressed" => Some("zip"),
        "application/gzip" | "application/x-gzip" => Some("gz"),
        "application/x-tar" => Some("tar"),
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "application/octet-stream" => Some("bin"),
        _ => None,
    }
}

pub(crate) struct FetchedDownload {
    pub id: u64,
    pub url: String,
    pub suggested: Option<String>,
    pub result: Result<crate::net::Response, String>,
}

impl crate::Browser {
    pub fn downloads(&self) -> &[DownloadEntry] {
        &self.downloads
    }

    pub fn set_downloads(&mut self, entries: Vec<DownloadEntry>) {
        self.downloads = entries;
        if self.downloads.len() > MAX_DOWNLOAD_HISTORY {
            let drain = self.downloads.len() - MAX_DOWNLOAD_HISTORY;
            self.downloads.drain(0..drain);
        }
        self.refresh_current_internal_page();
        self.dirty = true;
    }

    pub fn take_pending_downloads(&mut self) -> Vec<PendingDownload> {
        std::mem::take(&mut self.pending_downloads)
    }

    pub fn complete_download(&mut self, id: u64, result: Result<String, String>) {
        if let Some(entry) = self.downloads.iter_mut().find(|d| d.id == id) {
            match result {
                Ok(path) => {
                    entry.state = DownloadState::Completed;
                    entry.saved_path = Some(path.clone());
                    entry.error = None;
                    self.status = format!("Saved {} → {}", entry.filename, path);
                }
                Err(err) => {
                    entry.state = DownloadState::Failed;
                    entry.error = Some(err.clone());
                    self.status = format!("Download failed: {err}");
                }
            }
            self.refresh_current_internal_page();
            self.dirty = true;
        }
    }

    pub(crate) fn link_download_attribute(&self, mut node: usize) -> Option<String> {
        while node < self.document.nodes.len() {
            let n = &self.document.nodes[node];
            if n.tag == "a" {
                if n.has("download") {
                    return Some(n.attr("download").unwrap_or("").to_owned());
                }
                return None;
            }
            if node == 0 {
                break;
            }
            node = n.parent;
        }
        None
    }

    pub(crate) fn start_download(&mut self, url: &str, suggested: Option<&str>) {
        let Ok(parsed) = url::Url::parse(url) else {
            self.status = "Invalid download URL".into();
            self.dirty = true;
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            self.status = "Only HTTP(S) URLs can be downloaded".into();
            self.dirty = true;
            return;
        }
        if self.dl_inflight >= 4 {
            self.status = "Four downloads are already in progress; please wait".into();
            self.dirty = true;
            return;
        }
        self.next_download_id = self.next_download_id.saturating_add(1);
        let id = self.next_download_id;
        let clean_url = parsed.to_string();
        let initial_name =
            sanitize_download_filename(suggested, &clean_url, "application/octet-stream");
        self.push_download_entry(DownloadEntry {
            id,
            url: clean_url.clone(),
            filename: initial_name.clone(),
            saved_path: None,
            mime: "application/octet-stream".into(),
            bytes: 0,
            state: DownloadState::InProgress,
            error: None,
        });
        self.dl_inflight += 1;
        self.status = format!("Downloading {initial_name}...");
        self.dirty = true;
        let tx = self.dl_tx.clone();
        let session = self.session.clone();
        let suggested_owned = suggested.map(str::to_owned);
        std::thread::spawn(move || {
            let result = session.fetch(&clean_url);
            let _ = tx.send(FetchedDownload {
                id,
                url: clean_url,
                suggested: suggested_owned,
                result,
            });
        });
    }

    pub(crate) fn enqueue_loaded_download(
        &mut self,
        url: &str,
        content_disposition: Option<&str>,
        content_type: &str,
        body: Vec<u8>,
    ) {
        self.next_download_id = self.next_download_id.saturating_add(1);
        let id = self.next_download_id;
        let cd_name = content_disposition.and_then(|cd| parse_content_disposition(cd).1);
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .trim()
            .to_owned();
        let filename = sanitize_download_filename(cd_name.as_deref(), url, &mime);
        let bytes = body.len();
        if bytes > MAX_DOWNLOAD_BYTES {
            self.push_download_entry(DownloadEntry {
                id,
                url: url.to_owned(),
                filename,
                saved_path: None,
                mime,
                bytes,
                state: DownloadState::Failed,
                error: Some("Download exceeds 8 MiB limit".into()),
            });
            self.status = "Download exceeds 8 MiB limit".into();
            self.dirty = true;
            return;
        }
        self.push_download_entry(DownloadEntry {
            id,
            url: url.to_owned(),
            filename: filename.clone(),
            saved_path: None,
            mime: mime.clone(),
            bytes,
            state: DownloadState::InProgress,
            error: None,
        });
        if self.pending_downloads.len() < MAX_DOWNLOAD_HISTORY {
            self.pending_downloads.push(PendingDownload {
                id,
                url: url.to_owned(),
                suggested_filename: filename.clone(),
                mime,
                body,
            });
        }
        self.status = format!("Downloaded {filename} ({bytes} bytes) · saving...");
        self.refresh_current_internal_page();
        self.dirty = true;
    }

    pub(crate) fn save_current_page(&mut self) {
        if self.page_url == "about:blank" {
            self.status = "Nothing to save on about:blank".into();
            self.dirty = true;
            return;
        }
        let html = if !self.devtools.page_source.is_empty() {
            self.devtools.page_source.clone()
        } else {
            self.serialize_outer_html(0)
        };
        let bytes = html.into_bytes();
        self.enqueue_loaded_download(&self.page_url.clone(), None, "text/html", bytes);
    }

    pub(crate) fn poll_downloads(&mut self) {
        while let Ok(msg) = self.dl_rx.try_recv() {
            self.dl_inflight = self.dl_inflight.saturating_sub(1);
            match msg.result {
                Ok(resp) if (200..300).contains(&resp.status) => {
                    let cd_name = resp
                        .content_disposition
                        .as_deref()
                        .and_then(|cd| parse_content_disposition(cd).1);
                    let preferred = msg.suggested.as_deref().or(cd_name.as_deref());
                    let mime = resp
                        .content_type
                        .split(';')
                        .next()
                        .unwrap_or("application/octet-stream")
                        .trim()
                        .to_owned();
                    let final_url = resp.url.to_string();
                    let filename = sanitize_download_filename(preferred, &final_url, &mime);
                    let len = resp.body.len();
                    if let Some(entry) = self.downloads.iter_mut().find(|d| d.id == msg.id) {
                        entry.url = final_url.clone();
                        entry.filename = filename.clone();
                        entry.mime = mime.clone();
                        entry.bytes = len;
                        if len > MAX_DOWNLOAD_BYTES {
                            entry.state = DownloadState::Failed;
                            entry.error = Some("Download exceeds 8 MiB limit".into());
                        } else if self.pending_downloads.len() < MAX_DOWNLOAD_HISTORY {
                            self.pending_downloads.push(PendingDownload {
                                id: msg.id,
                                url: final_url,
                                suggested_filename: filename.clone(),
                                mime,
                                body: resp.body,
                            });
                            self.status =
                                format!("Downloaded {filename} ({len} bytes) · saving...");
                        }
                    }
                    self.devtools
                        .sync_network_exchanges(&self.session.recent_exchanges());
                }
                Ok(resp) => {
                    let err = format!("HTTP {}", resp.status);
                    if let Some(entry) = self.downloads.iter_mut().find(|d| d.id == msg.id) {
                        entry.state = DownloadState::Failed;
                        entry.error = Some(err.clone());
                    }
                    self.status = format!("Download failed ({err}): {}", msg.url);
                }
                Err(err) => {
                    if let Some(entry) = self.downloads.iter_mut().find(|d| d.id == msg.id) {
                        entry.state = DownloadState::Failed;
                        entry.error = Some(err.clone());
                    }
                    self.status = format!("Download failed: {err}");
                }
            }
            self.refresh_current_internal_page();
            self.dirty = true;
        }
    }

    fn push_download_entry(&mut self, entry: DownloadEntry) {
        if self.downloads.len() >= MAX_DOWNLOAD_HISTORY {
            self.downloads.remove(0);
        }
        self.downloads.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_disposition_and_sanitizes_traversal_or_dotfiles() {
        assert_eq!(
            parse_content_disposition("attachment; filename=\"report 2026.pdf\""),
            (true, Some("report 2026.pdf".into()))
        );
        assert_eq!(
            parse_content_disposition(
                "attachment; filename=\"fallback.txt\"; filename*=UTF-8''caf%C3%A9-notes.txt"
            ),
            (true, Some("café-notes.txt".into()))
        );
        assert_eq!(
            parse_content_disposition("inline; filename=preview.png"),
            (false, Some("preview.png".into()))
        );

        assert_eq!(
            sanitize_download_filename(
                Some("../../etc/.ssh/authorized_keys"),
                "https://example.com/a",
                "application/octet-stream"
            ),
            "authorized_keys.bin"
        );
        assert_eq!(
            sanitize_download_filename(Some(".bashrc"), "https://example.com/", "text/plain"),
            "bashrc.txt"
        );
        assert_eq!(
            sanitize_download_filename(
                None,
                "https://example.com/files/archive.tar.gz?x=1",
                "application/gzip"
            ),
            "archive.tar.gz"
        );
        assert_eq!(
            sanitize_download_filename(None, "https://example.com/", "text/html"),
            "example_com.html"
        );
        assert!(should_download_response("application/pdf", None));
        assert!(should_download_response(
            "text/html",
            Some("attachment; filename=\"saved.html\"")
        ));
        assert!(!should_download_response("text/html; charset=utf-8", None));
    }
}
