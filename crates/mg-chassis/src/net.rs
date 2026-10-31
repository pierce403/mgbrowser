//! Bounded HTTP/1.1 transport for the research browser.
//!
//! TLS always uses our explicitly selected RustCrypto provider and public roots.
//! Cookies stay in memory for one browser session. There is no authentication,
//! proxy, or HTTP/2 support.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime};

use mg_sparkle::page_session::platform::{
    MAX_STORAGE_QUOTA_BYTES, ScriptFetchRequest, ScriptFetchResponse, StorageOp,
    base64_decode_forgiving, base64_encode,
};
use url::Url;

const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_HEADERS: usize = 64 * 1024;
const MAX_REQUEST: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 8;
const MAX_COOKIES: usize = 128;
const MAX_SITE_COOKIES: usize = 32;
const MAX_COOKIE_BYTES: usize = 4096;
const MAX_COOKIE_HEADER: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
pub use mg_sparkle::USER_AGENT;

#[derive(Debug)]
pub struct Response {
    pub url: Url,
    pub status: u16,
    pub content_type: String,
    pub content_disposition: Option<String>,
    pub body: Vec<u8>,
    /// Parent resource proof only: every visited URL stayed on the supplied
    /// document origin. False for non-resource/unknown provenance.
    pub resource_same_origin_chain: bool,
}

const MAX_EXCHANGES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExchangeRecord {
    pub method: &'static str,
    pub url: String,
    pub status: u16,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub body_preview: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookieSummary {
    pub name: String,
    pub value_bytes: usize,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub session_only: bool,
    pub ttl_secs: Option<u64>,
}

/// Shared, memory-only browser session. Clones share the same bounded cookie jar.
#[derive(Clone, Default)]
pub struct Session {
    cookies: Arc<Mutex<CookieJar>>,
    exchanges: Arc<Mutex<VecDeque<ExchangeRecord>>>,
    disable_cache: Arc<AtomicBool>,
    local_storage: Arc<Mutex<BTreeMap<String, Vec<(String, String)>>>>,
}

/// Automatic resource authority, separate from top-level navigation and updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourceKind {
    Stylesheet,
    Image,
    Script,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_disable_cache(&self, disabled: bool) {
        self.disable_cache.store(disabled, Ordering::Relaxed);
    }

    pub fn cache_disabled(&self) -> bool {
        self.disable_cache.load(Ordering::Relaxed)
    }

    pub fn recent_exchanges(&self) -> Vec<ExchangeRecord> {
        self.exchanges
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear_exchanges(&self) {
        if let Ok(mut q) = self.exchanges.lock() {
            q.clear();
        }
    }

    pub fn cookies_snapshot(&self) -> Vec<CookieSummary> {
        let Ok(mut jar) = self.cookies.lock() else {
            return Vec::new();
        };
        jar.prune();
        let now = SystemTime::now();
        jar.cookies
            .iter()
            .map(|c| CookieSummary {
                name: c.name.clone(),
                value_bytes: c.value.len(),
                domain: c.domain.clone(),
                host_only: c.host_only,
                path: c.path.clone(),
                secure: c.secure,
                session_only: c.expires.is_none(),
                ttl_secs: c
                    .expires
                    .and_then(|exp| exp.duration_since(now).ok().map(|d| d.as_secs())),
            })
            .collect()
    }

    pub fn clear_cookies(&self) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.cookies.clear();
        }
    }

    pub(crate) fn cdp_cookies(&self) -> Vec<serde_json::Value> {
        let Ok(mut jar) = self.cookies.lock() else {
            return Vec::new();
        };
        jar.prune();
        jar.cookies
            .iter()
            .map(|c| {
                let expires = c
                    .expires
                    .and_then(|exp| exp.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(-1.0);
                let domain = if c.host_only {
                    c.domain.clone()
                } else {
                    format!(".{}", c.domain)
                };
                serde_json::json!({
                    "name": c.name,
                    "value": c.value,
                    "domain": domain,
                    "path": c.path,
                    "expires": expires,
                    "size": c.name.len() + c.value.len(),
                    "httpOnly": c.http_only,
                    "secure": c.secure,
                    "session": c.expires.is_none(),
                    "sameSite": "Lax",
                })
            })
            .collect()
    }

    pub fn clear_local_storage(&self) {
        if let Ok(mut map) = self.local_storage.lock() {
            map.clear();
        }
    }

    pub fn local_storage_for_origin(&self, url: &str) -> Vec<(String, String)> {
        let Some(origin) = storage_origin_key(url) else {
            return Vec::new();
        };
        self.local_storage
            .lock()
            .ok()
            .and_then(|map| map.get(&origin).cloned())
            .unwrap_or_default()
    }

    pub fn apply_local_storage_ops(&self, url: &str, ops: &[StorageOp]) {
        if ops.is_empty() {
            return;
        }
        let Some(origin) = storage_origin_key(url) else {
            return;
        };
        let Ok(mut map) = self.local_storage.lock() else {
            return;
        };
        if !map.contains_key(&origin) && map.len() >= 256 {
            return;
        }
        let entries = map.entry(origin.clone()).or_default();
        apply_storage_ops(entries, ops);
        if entries.is_empty() {
            map.remove(&origin);
        }
    }

    pub fn script_cookie_header(&self, url: &str) -> String {
        let Ok(parsed) = Url::parse(url) else {
            return String::new();
        };
        if validate_url(&parsed).is_err() {
            return String::new();
        }
        self.cookies
            .lock()
            .map(|mut jar| jar.script_header(&parsed))
            .unwrap_or_default()
    }

    pub fn set_script_cookie(&self, url: &str, raw_cookie: &str) {
        let Ok(parsed) = Url::parse(url) else {
            return;
        };
        if validate_url(&parsed).is_err() {
            return;
        }
        if let Ok(mut jar) = self.cookies.lock() {
            jar.store_from_script(&parsed, raw_cookie);
        }
    }

    pub fn fetch_script_same_origin(
        &self,
        page_url: &str,
        req: ScriptFetchRequest,
    ) -> ScriptFetchResponse {
        let req_id = req.request_id.max(1);
        if let Err(err) = req.validate() {
            return ScriptFetchResponse::network_error(req_id, err);
        }
        let Ok(origin) = Url::parse(page_url) else {
            return ScriptFetchResponse::network_error(req_id, "Invalid document origin");
        };
        if let Err(err) = validate_url(&origin) {
            return ScriptFetchResponse::network_error(req_id, err);
        }
        let Ok(mut url) = Url::parse(&req.url) else {
            return ScriptFetchResponse::network_error(req_id, "Invalid request URL");
        };
        if let Err(err) = validate_url(&url) {
            return ScriptFetchResponse::network_error(req_id, err);
        }
        if origin.origin() != url.origin() {
            return ScriptFetchResponse::network_error(req_id, "Same-origin policy violation");
        }
        let mut method = req.method.trim().to_owned();
        if method.is_empty()
            || method.len() > 32
            || !method
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return ScriptFetchResponse::network_error(req_id, "Invalid HTTP method");
        }
        for std_m in ["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] {
            if method.eq_ignore_ascii_case(std_m) {
                method = std_m.to_owned();
                break;
            }
        }
        if matches!(
            method.to_ascii_uppercase().as_str(),
            "CONNECT" | "TRACE" | "TRACK"
        ) {
            return ScriptFetchResponse::network_error(req_id, "Forbidden HTTP method");
        }
        let mut body_bytes = match &req.body_b64 {
            Some(b64) if !b64.is_empty() => match base64_decode_forgiving(b64) {
                Ok(decoded) if decoded.len() <= 72 * 1024 => Some(decoded),
                Ok(_) => {
                    return ScriptFetchResponse::network_error(
                        req_id,
                        "Request body exceeds limit",
                    );
                }
                Err(err) => return ScriptFetchResponse::network_error(req_id, err),
            },
            _ => None,
        };
        if matches!(method.as_str(), "GET" | "HEAD") {
            body_bytes = None;
        }
        let mut custom_headers = Vec::new();
        for (raw_name, raw_val) in &req.headers {
            let name = raw_name.trim().to_ascii_lowercase();
            let val = raw_val.trim();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                || val.bytes().any(|b| (b < b' ' && b != b'\t') || b == 127)
                || is_forbidden_script_request_header(&name)
            {
                continue;
            }
            custom_headers.push((name, val.to_owned()));
        }

        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = match crate::tls_client_config(roots) {
            Ok(cfg) => cfg,
            Err(err) => {
                return ScriptFetchResponse::network_error(
                    req_id,
                    format!("TLS configuration: {err}"),
                );
            }
        };
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let config = Arc::new(config);
        let deadline = Instant::now() + Duration::from_secs(15);
        let max_body: usize = 1024 * 1024;
        let no_cache = self.cache_disabled();

        for redirects in 0..=5 {
            if let Err(err) = validate_url(&url) {
                return ScriptFetchResponse::network_error(req_id, err);
            }
            if origin.origin() != url.origin() {
                return ScriptFetchResponse::network_error(req_id, "Same-origin policy violation");
            }
            let cookie_header = match self.cookies.lock() {
                Ok(mut jar) => jar.header(&url),
                Err(_) => return ScriptFetchResponse::network_error(req_id, "Cookie lock failed"),
            };
            let req_wire = match script_request_bytes(
                &method,
                &url,
                &custom_headers,
                body_bytes.as_deref(),
                &cookie_header,
                no_cache,
            ) {
                Ok(bytes) => bytes,
                Err(err) => return ScriptFetchResponse::network_error(req_id, err),
            };
            let socket = match connect(&url, deadline) {
                Ok(s) => s,
                Err(err) => return ScriptFetchResponse::network_error(req_id, err),
            };
            let mut stream: Box<dyn Transport> = if url.scheme() == "https" {
                let hostname = match url.host() {
                    Some(url::Host::Domain(h)) => h.to_owned(),
                    Some(url::Host::Ipv4(ip)) => ip.to_string(),
                    Some(url::Host::Ipv6(ip)) => ip.to_string(),
                    None => return ScriptFetchResponse::network_error(req_id, "URL has no host"),
                };
                let server_name = match rustls::pki_types::ServerName::try_from(hostname) {
                    Ok(sn) => sn,
                    Err(err) => {
                        return ScriptFetchResponse::network_error(
                            req_id,
                            format!("Invalid TLS server name: {err}"),
                        );
                    }
                };
                let client = match rustls::ClientConnection::new(config.clone(), server_name) {
                    Ok(c) => c,
                    Err(err) => {
                        return ScriptFetchResponse::network_error(
                            req_id,
                            format!("TLS initialization: {err}"),
                        );
                    }
                };
                Box::new(rustls::StreamOwned::new(client, socket))
            } else {
                Box::new(socket)
            };
            if let Err(err) = stream.write_all(&req_wire).and_then(|()| stream.flush()) {
                return ScriptFetchResponse::network_error(
                    req_id,
                    format!("Request write failed: {err}"),
                );
            }
            let is_head = method.eq_ignore_ascii_case("HEAD");
            let (head, raw_bytes) = match read_response_progress_with_mode(
                BufReader::new(stream),
                max_body,
                is_head,
                &mut |_, _| {},
            ) {
                Ok(res) => res,
                Err(err) => return ScriptFetchResponse::network_error(req_id, err),
            };

            if let Ok(mut log) = self.exchanges.lock() {
                let static_method = match method.as_str() {
                    "GET" => "GET",
                    "POST" => "POST",
                    "PUT" => "PUT",
                    "DELETE" => "DELETE",
                    "PATCH" => "PATCH",
                    "HEAD" => "HEAD",
                    "OPTIONS" => "OPTIONS",
                    _ => "FETCH",
                };
                let mut req_headers = Vec::new();
                if let Some(h) = url.host() {
                    let mut host_val = h.to_string();
                    if let Some(p) = url.port() {
                        host_val.push_str(&format!(":{p}"));
                    }
                    req_headers.push(("host".into(), host_val));
                }
                req_headers.push(("user-agent".into(), USER_AGENT.to_owned()));
                for (k, v) in &custom_headers {
                    let logged = if matches!(k.as_str(), "authorization" | "proxy-authorization") {
                        format!("<redacted {} bytes>", v.len())
                    } else {
                        v.chars().take(256).collect()
                    };
                    req_headers.push((k.clone(), logged));
                }
                if !cookie_header.is_empty() {
                    req_headers.push((
                        "cookie".into(),
                        format!("<redacted {} bytes>", cookie_header.len()),
                    ));
                }
                let resp_headers = head
                    .headers
                    .iter()
                    .take(32)
                    .map(|(k, v)| {
                        let val = if matches!(
                            k.as_str(),
                            "set-cookie"
                                | "set-cookie2"
                                | "authorization"
                                | "proxy-authorization"
                                | "www-authenticate"
                        ) {
                            format!("<redacted {} bytes>", v.len())
                        } else {
                            v.chars().take(256).collect()
                        };
                        (k.clone(), val)
                    })
                    .collect();
                if log.len() >= MAX_EXCHANGES {
                    log.pop_front();
                }
                log.push_back(ExchangeRecord {
                    method: static_method,
                    url: url.as_str().chars().take(512).collect(),
                    status: head.status,
                    request_headers: req_headers,
                    response_headers: resp_headers,
                    body_preview: None,
                });
            }

            if let Ok(mut jar) = self.cookies.lock() {
                for (_, value) in head.headers.iter().filter(|(name, _)| name == "set-cookie") {
                    jar.store(&url, value);
                }
            }

            if matches!(head.status, 301 | 302 | 303 | 307 | 308)
                && let Some(location) = head.header("location")
            {
                if redirects == 5 {
                    return ScriptFetchResponse::network_error(
                        req_id,
                        "Redirect limit (5) exceeded",
                    );
                }
                let Ok(next) = url.join(location) else {
                    return ScriptFetchResponse::network_error(req_id, "Invalid redirect URL");
                };
                if let Err(err) = validate_url(&next) {
                    return ScriptFetchResponse::network_error(req_id, err);
                }
                if origin.origin() != next.origin() {
                    return ScriptFetchResponse::network_error(
                        req_id,
                        "Cross-origin redirect blocked by same-origin policy",
                    );
                }
                if head.status == 303
                    || (matches!(head.status, 301 | 302) && method.eq_ignore_ascii_case("POST"))
                {
                    method = "GET".into();
                    body_bytes = None;
                    custom_headers.retain(|(k, _)| {
                        !matches!(
                            k.as_str(),
                            "content-type" | "content-language" | "content-location"
                        )
                    });
                }
                url = next;
                continue;
            }

            let decoded = if is_head || raw_bytes.is_empty() {
                Vec::new()
            } else {
                match decode_content_limited(raw_bytes, head.header("content-encoding"), max_body) {
                    Ok(b) => b,
                    Err(err) => return ScriptFetchResponse::network_error(req_id, err),
                }
            };
            let had_encoding = head.header("content-encoding").is_some();
            if let Ok(mut log) = self.exchanges.lock()
                && let Some(last) = log.back_mut()
            {
                let ct = head
                    .header("content-type")
                    .unwrap_or("application/octet-stream")
                    .to_ascii_lowercase();
                let textual = ct.starts_with("text/")
                    || ct.contains("javascript")
                    || ct.contains("ecmascript")
                    || ct.contains("json")
                    || ct.contains("xml")
                    || ct.contains("svg");
                if textual && !decoded.is_empty() {
                    let sample = &decoded[..decoded.len().min(32 * 1024)];
                    last.body_preview = Some(String::from_utf8_lossy(sample).into_owned());
                }
            }
            let exposed_headers: Vec<(String, String)> = head
                .headers
                .into_iter()
                .filter(|(k, _)| {
                    !matches!(k.as_str(), "set-cookie" | "set-cookie2")
                        && (!had_encoding
                            || !matches!(
                                k.as_str(),
                                "content-encoding" | "content-length" | "transfer-encoding"
                            ))
                })
                .take(128)
                .collect();
            let updated_cookies = Some(self.script_cookie_header(origin.as_str()));
            return ScriptFetchResponse {
                request_id: req_id,
                ok_transport: true,
                error: None,
                status: head.status,
                status_text: head.status_text,
                final_url: url.as_str().to_owned(),
                headers: exposed_headers,
                body_b64: base64_encode(&decoded),
                updated_cookies,
            };
        }
        ScriptFetchResponse::network_error(req_id, "Redirect limit exceeded")
    }

    pub fn fetch(&self, url: &str) -> Result<Response, String> {
        self.submit(url, None)
    }

    pub fn submit(&self, url: &str, body: Option<&str>) -> Result<Response, String> {
        self.submit_with_cache_override(url, body, false)
    }

    pub fn submit_with_cache_override(
        &self,
        url: &str,
        body: Option<&str>,
        force_no_cache: bool,
    ) -> Result<Response, String> {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config =
            crate::tls_client_config(roots).map_err(|e| format!("TLS configuration: {e}"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        submit_with_limits_recorded(
            url,
            body,
            Arc::new(config),
            &self.cookies,
            RequestLimits {
                https_only: false,
                origin: None,
                resource_kind: ResourceKind::Stylesheet,
                max_body: MAX_BODY,
                deadline: Instant::now() + REQUEST_TIMEOUT,
            },
            Some(&self.exchanges),
            force_no_cache || self.cache_disabled(),
            &mut |_, _| {},
        )
    }

    /// Bounded automatic request. Cross-origin stylesheets, images and scripts
    /// use credential-free HTTPS. Every redirect is checked before I/O.
    pub(crate) fn fetch_resource(
        &self,
        url: &str,
        page_url: &str,
        max_body: usize,
        deadline: Instant,
        kind: ResourceKind,
    ) -> Result<Response, String> {
        let origin = Url::parse(page_url).map_err(|e| format!("Invalid resource origin: {e}"))?;
        validate_url(&origin)?;
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = crate::tls_client_config(roots).map_err(|e| e.to_string())?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        submit_with_limits_recorded(
            url,
            None,
            Arc::new(config),
            &self.cookies,
            RequestLimits {
                https_only: false,
                origin: Some(origin),
                resource_kind: kind,
                max_body: max_body.min(MAX_BODY),
                deadline,
            },
            Some(&self.exchanges),
            self.cache_disabled(),
            &mut |_, _| {},
        )
    }
}

/// Fetch a public HTTP(S) resource with bounded redirects, time, and body size.
pub fn fetch(url: &str) -> Result<Response, String> {
    submit(url, None)
}

/// Submit a URL-encoded form, or perform a GET when `body` is `None`.
pub fn submit(url: &str, body: Option<&str>) -> Result<Response, String> {
    Session::new().submit(url, body)
}

#[cfg(test)]
fn submit_with_config(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
) -> Result<Response, String> {
    submit_with_config_policy(url, body, config, cookies, false)
}

/// Isolated release transport: no browser cookies and HTTPS on every redirect.
pub fn fetch_release(url: &str) -> Result<Response, String> {
    fetch_release_with_progress(url, &mut |_, _| {})
}

/// Report received body bytes for a successful release response. Missing lengths
/// remain unknown; redirects, TLS verification and response limits are unchanged.
pub fn fetch_release_with_progress(
    url: &str,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Response, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = crate::tls_client_config(roots).map_err(|e| e.to_string())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    submit_with_limits_progress(
        url,
        None,
        Arc::new(config),
        &Mutex::new(CookieJar::default()),
        RequestLimits {
            https_only: true,
            origin: None,
            resource_kind: ResourceKind::Stylesheet,
            max_body: MAX_BODY,
            deadline: Instant::now() + REQUEST_TIMEOUT,
        },
        progress,
    )
}

#[cfg(test)]
fn submit_with_config_policy(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
    https_only: bool,
) -> Result<Response, String> {
    submit_with_limits(
        url,
        body,
        config,
        cookies,
        RequestLimits {
            https_only,
            origin: None,
            resource_kind: ResourceKind::Stylesheet,
            max_body: MAX_BODY,
            deadline: Instant::now() + REQUEST_TIMEOUT,
        },
    )
}

struct RequestLimits {
    https_only: bool,
    origin: Option<Url>,
    resource_kind: ResourceKind,
    max_body: usize,
    deadline: Instant,
}

#[cfg(test)]
fn submit_with_limits(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
    limits: RequestLimits,
) -> Result<Response, String> {
    submit_with_limits_progress(url, body, config, cookies, limits, &mut |_, _| {})
}

fn submit_with_limits_progress(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
    limits: RequestLimits,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Response, String> {
    submit_with_limits_recorded(url, body, config, cookies, limits, None, false, progress)
}

#[allow(clippy::too_many_arguments)]
fn submit_with_limits_recorded(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
    limits: RequestLimits,
    exchanges: Option<&Mutex<VecDeque<ExchangeRecord>>>,
    no_cache: bool,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Response, String> {
    let mut url = Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let mut body = body;
    // Image credentials never resume after the first origin crossing, even if
    // a later redirect returns to the page's origin. HTTPS likewise never
    // downgrades to the same-origin HTTP exception once visited.
    let mut credentials_allowed = true;
    let mut visited_https = false;
    // This resource proof is independent of cookie permission. Once a hop
    // leaves the accepted document origin, a later return cannot upgrade it.
    let mut resource_same_origin_chain = limits.origin.is_some();
    for redirects in 0..=MAX_REDIRECTS {
        validate_url(&url)?;
        if limits.https_only && url.scheme() != "https" {
            return Err("Release download requires HTTPS, including redirects".into());
        }
        if let Some(origin) = &limits.origin {
            let same_origin = origin.origin() == url.origin();
            resource_same_origin_chain &= same_origin;
            match limits.resource_kind {
                ResourceKind::Stylesheet => {
                    if url.scheme() != "https" && (!same_origin || visited_https) {
                        return Err(
                            "Stylesheets require HTTPS across origins and cannot downgrade from HTTPS, including redirects".into(),
                        );
                    }
                    credentials_allowed &= same_origin;
                }
                ResourceKind::Image => {
                    if url.scheme() != "https" && (!same_origin || visited_https) {
                        return Err(
                            "Images require HTTPS across origins and cannot downgrade from HTTPS, including redirects".into(),
                        );
                    }
                    credentials_allowed &= same_origin;
                }
                ResourceKind::Script => {
                    if url.scheme() != "https" && (!same_origin || visited_https) {
                        return Err(
                            "Scripts require HTTPS across origins and cannot downgrade from HTTPS, including redirects".into(),
                        );
                    }
                    credentials_allowed &= same_origin;
                }
            }
        }
        visited_https |= url.scheme() == "https";
        let cookie_header = if credentials_allowed {
            cookies
                .lock()
                .map_err(|_| "Cookie session lock failed")?
                .header(&url)
        } else {
            String::new()
        };
        let (head, bytes) = request(
            &url,
            body,
            config.clone(),
            limits.deadline,
            &cookie_header,
            no_cache,
            limits.max_body,
            progress,
        )?;
        if let Some(exchanges) = exchanges
            && let Ok(mut log) = exchanges.lock()
        {
            let method = if body.is_some() { "POST" } else { "GET" };
            let mut req_headers = Vec::with_capacity(9);
            if let Some(h) = url.host() {
                let mut host_val = h.to_string();
                if let Some(p) = url.port() {
                    host_val.push_str(&format!(":{p}"));
                }
                req_headers.push(("host".into(), host_val));
            }
            req_headers.push(("user-agent".into(), USER_AGENT.to_owned()));
            req_headers.push((
                "accept".into(),
                "text/html,application/xhtml+xml,image/png;q=0.8,*/*;q=0.5".into(),
            ));
            req_headers.push(("accept-language".into(), "en-US,en;q=0.8".into()));
            req_headers.push(("accept-encoding".into(), "identity".into()));
            req_headers.push(("connection".into(), "close".into()));
            if no_cache {
                req_headers.push(("cache-control".into(), "no-cache".into()));
                req_headers.push(("pragma".into(), "no-cache".into()));
            }
            if !cookie_header.is_empty() {
                req_headers.push((
                    "cookie".into(),
                    format!("<redacted {} bytes>", cookie_header.len()),
                ));
            }
            if let Some(b) = body {
                req_headers.push((
                    "content-type".into(),
                    "application/x-www-form-urlencoded".into(),
                ));
                req_headers.push(("content-length".into(), b.len().to_string()));
            }
            let resp_headers = head
                .headers
                .iter()
                .take(32)
                .map(|(k, v)| {
                    let val = if matches!(
                        k.as_str(),
                        "set-cookie" | "authorization" | "proxy-authorization" | "www-authenticate"
                    ) {
                        format!("<redacted {} bytes>", v.len())
                    } else {
                        v.chars().take(256).collect()
                    };
                    (k.clone(), val)
                })
                .collect();
            if log.len() >= MAX_EXCHANGES {
                log.pop_front();
            }
            log.push_back(ExchangeRecord {
                method,
                url: url.as_str().chars().take(512).collect(),
                status: head.status,
                request_headers: req_headers,
                response_headers: resp_headers,
                body_preview: None,
            });
        }
        if credentials_allowed {
            let mut jar = cookies.lock().map_err(|_| "Cookie session lock failed")?;
            for (_, value) in head.headers.iter().filter(|(name, _)| name == "set-cookie") {
                jar.store(&url, value);
            }
        }
        if matches!(head.status, 301 | 302 | 303 | 307 | 308)
            && let Some(location) = head.header("location")
        {
            if redirects == MAX_REDIRECTS {
                return Err(format!("Redirect limit ({MAX_REDIRECTS}) exceeded"));
            }
            let next = url
                .join(location)
                .map_err(|e| format!("Invalid redirect URL: {e}"))?;
            validate_url(&next)?;
            if url.scheme() == "https"
                && next.scheme() == "http"
                && body.is_some()
                && matches!(head.status, 307 | 308)
            {
                return Err("Refusing to resend an HTTPS form over unencrypted HTTP".into());
            }
            if matches!(head.status, 301 | 302 | 303) {
                body = None;
            }
            url = next;
            continue;
        }
        let content_type = head
            .header("content-type")
            .unwrap_or("application/octet-stream")
            .to_owned();
        let bytes =
            decode_content_limited(bytes, head.header("content-encoding"), limits.max_body)?;
        if let Some(exchanges) = exchanges
            && let Ok(mut log) = exchanges.lock()
            && let Some(last) = log.back_mut()
        {
            let ct = content_type.to_ascii_lowercase();
            let textual = ct.starts_with("text/")
                || ct.contains("javascript")
                || ct.contains("ecmascript")
                || ct.contains("json")
                || ct.contains("xml")
                || ct.contains("svg");
            if textual && !bytes.is_empty() {
                let sample = &bytes[..bytes.len().min(32 * 1024)];
                last.body_preview = Some(String::from_utf8_lossy(sample).into_owned());
            }
        }
        let content_disposition = head.header("content-disposition").map(str::to_owned);
        return Ok(Response {
            url,
            status: head.status,
            content_type,
            content_disposition,
            body: bytes,
            resource_same_origin_chain,
        });
    }
    unreachable!("bounded redirect loop always returns")
}

#[derive(Default)]
struct CookieJar {
    cookies: Vec<Cookie>,
}

struct Cookie {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
    secure: bool,
    http_only: bool,
    expires: Option<SystemTime>,
}

impl CookieJar {
    fn prune(&mut self) {
        let now = SystemTime::now();
        self.cookies
            .retain(|cookie| cookie.expires.is_none_or(|expires| expires > now));
    }

    fn store(&mut self, url: &Url, header: &str) {
        self.store_inner(url, header, false);
    }

    fn store_from_script(&mut self, url: &Url, header: &str) {
        self.store_inner(url, header, true);
    }

    fn store_inner(&mut self, url: &Url, header: &str, from_script: bool) {
        if header.len() > MAX_COOKIE_BYTES {
            return;
        }
        let Some(host) = url.host_str() else {
            return;
        };
        let mut parts = header.split(';');
        let Some((name, value)) = parts.next().and_then(|pair| pair.trim().split_once('=')) else {
            return;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return;
        }
        // Restrict stored values to the RFC cookie-octet range, including quoted
        // values. Never copy controls, delimiters, or non-ASCII into a request.
        let unquoted = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value);
        if !unquoted.bytes().all(|byte| {
            byte == 0x21
                || (0x23..=0x2b).contains(&byte)
                || (0x2d..=0x3a).contains(&byte)
                || (0x3c..=0x5b).contains(&byte)
                || (0x5d..=0x7e).contains(&byte)
        }) {
            return;
        }
        let mut cookie = Cookie {
            name: name.to_owned(),
            value: value.to_owned(),
            domain: host.to_owned(),
            host_only: true,
            path: default_cookie_path(url.path()).to_owned(),
            secure: false,
            http_only: false,
            expires: None,
        };
        let mut max_age = None;
        let mut explicit_path = false;
        for attribute in parts {
            let (name, value) = attribute
                .trim()
                .split_once('=')
                .unwrap_or((attribute.trim(), ""));
            match name.trim().to_ascii_lowercase().as_str() {
                "domain" => {
                    let domain = value
                        .trim()
                        .strip_prefix('.')
                        .unwrap_or(value.trim())
                        .to_ascii_lowercase();
                    if !matches!(url.host(), Some(url::Host::Domain(_)))
                        || !domain_matches(host, &domain)
                        || domain.ends_with('.')
                        || psl::domain(domain.as_bytes()).is_none()
                    {
                        return;
                    }
                    cookie.domain = domain;
                    cookie.host_only = false;
                }
                "path" if value.starts_with('/') => {
                    cookie.path = value.to_owned();
                    explicit_path = true;
                }
                "secure" => cookie.secure = true,
                "httponly" => {
                    if from_script {
                        return;
                    }
                    cookie.http_only = true;
                }
                "max-age" => {
                    max_age = value.trim().parse::<i64>().ok();
                }
                "expires" => {
                    cookie.expires = httpdate::parse_http_date(value.trim()).ok();
                }
                // Partition keys are outside this transport.
                "partitioned" => return,
                _ => {}
            }
        }
        if cookie.secure && url.scheme() != "https" {
            return;
        }
        if cookie.name.starts_with("__Secure-") && !cookie.secure {
            return;
        }
        if cookie.name.starts_with("__Host-")
            && (!cookie.secure || !cookie.host_only || !explicit_path || cookie.path != "/")
        {
            return;
        }
        if let Some(seconds) = max_age {
            cookie.expires = if seconds <= 0 {
                Some(SystemTime::UNIX_EPOCH)
            } else {
                SystemTime::now().checked_add(Duration::from_secs(
                    (seconds as u64).min(400 * 24 * 60 * 60),
                ))
            };
        }
        self.prune();
        if from_script
            && self.cookies.iter().any(|existing| {
                existing.http_only
                    && existing.name == cookie.name
                    && (domain_matches(&existing.domain, &cookie.domain)
                        || domain_matches(&cookie.domain, &existing.domain))
                    && (cookie_path_matches(&cookie.path, &existing.path)
                        || cookie_path_matches(&existing.path, &cookie.path))
            })
        {
            return;
        }
        // An unencrypted response cannot overwrite a secure cookie of the same
        // name on this domain/path or introduce a more specific shadow cookie.
        if url.scheme() != "https"
            && self.cookies.iter().any(|existing| {
                existing.secure
                    && existing.name == cookie.name
                    && (domain_matches(&existing.domain, &cookie.domain)
                        || domain_matches(&cookie.domain, &existing.domain))
                    && cookie_path_matches(&cookie.path, &existing.path)
            })
        {
            return;
        }
        let position = self.cookies.iter().position(|existing| {
            existing.name == cookie.name
                && existing.domain == cookie.domain
                && existing.path == cookie.path
        });
        if cookie
            .expires
            .is_some_and(|expires| expires <= SystemTime::now())
        {
            if let Some(position) = position {
                self.cookies.remove(position);
            }
            return;
        }
        if let Some(position) = position {
            self.cookies[position] = cookie;
            return;
        }
        let site = cookie_site(&cookie.domain);
        if self
            .cookies
            .iter()
            .filter(|existing| cookie_site(&existing.domain) == site)
            .count()
            >= MAX_SITE_COOKIES
            && let Some(position) = self
                .cookies
                .iter()
                .position(|existing| cookie_site(&existing.domain) == site)
        {
            self.cookies.remove(position);
        }
        if self.cookies.len() >= MAX_COOKIES {
            self.cookies.remove(0);
        }
        self.cookies.push(cookie);
    }

    fn header(&mut self, url: &Url) -> String {
        self.header_filtered(url, true)
    }

    fn script_header(&mut self, url: &Url) -> String {
        self.header_filtered(url, false)
    }

    fn header_filtered(&mut self, url: &Url, include_http_only: bool) -> String {
        self.prune();
        let Some(host) = url.host_str() else {
            return String::new();
        };
        let mut matches = self
            .cookies
            .iter()
            .filter(|cookie| {
                (include_http_only || !cookie.http_only)
                    && (!cookie.secure || url.scheme() == "https")
                    && if cookie.host_only {
                        host == cookie.domain
                    } else {
                        domain_matches(host, &cookie.domain)
                    }
                    && cookie_path_matches(url.path(), &cookie.path)
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        let mut header = String::new();
        for cookie in matches {
            let separator = if header.is_empty() { "" } else { "; " };
            if header.len() + separator.len() + cookie.name.len() + 1 + cookie.value.len()
                > MAX_COOKIE_HEADER
            {
                break;
            }
            header.push_str(separator);
            header.push_str(&cookie.name);
            header.push('=');
            header.push_str(&cookie.value);
        }
        header
    }
}

pub fn storage_origin_key(url_str: &str) -> Option<String> {
    let parsed = Url::parse(url_str).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    Some(match parsed.port() {
        Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
        None => format!("{}://{host}", parsed.scheme()),
    })
}

pub fn apply_storage_ops(entries: &mut Vec<(String, String)>, ops: &[StorageOp]) {
    for op in ops {
        match op {
            StorageOp::Set { key, value } => {
                let mut total: usize = entries
                    .iter()
                    .filter(|(k, _)| k != key)
                    .map(|(k, v)| k.len().saturating_add(v.len()))
                    .sum();
                total = total.saturating_add(key.len()).saturating_add(value.len());
                if total > MAX_STORAGE_QUOTA_BYTES {
                    continue;
                }
                if let Some(existing) = entries.iter_mut().find(|(k, _)| k == key) {
                    existing.1 = value.clone();
                } else {
                    entries.push((key.clone(), value.clone()));
                }
            }
            StorageOp::Remove { key } => {
                entries.retain(|(k, _)| k != key);
            }
            StorageOp::Clear => {
                entries.clear();
            }
        }
    }
}

fn is_forbidden_script_request_header(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie"
            | "cookie2"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
            | "user-agent"
    ) || lower_name.starts_with("proxy-")
        || lower_name.starts_with("sec-")
}

fn script_request_bytes(
    method: &str,
    url: &Url,
    custom_headers: &[(String, String)],
    body: Option<&[u8]>,
    cookie_header: &str,
    no_cache: bool,
) -> Result<Vec<u8>, String> {
    let mut target = url.path().to_owned();
    if target.is_empty() {
        target.push('/');
    }
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    let mut host = url.host().ok_or("URL has no host")?.to_string();
    if let Some(port) = url.port() {
        host.push_str(&format!(":{port}"));
    }
    let has_accept = custom_headers.iter().any(|(k, _)| k == "accept");
    let has_accept_lang = custom_headers.iter().any(|(k, _)| k == "accept-language");
    let has_cache_ctrl = custom_headers.iter().any(|(k, _)| k == "cache-control");
    let mut head =
        format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\n");
    if !has_accept {
        head.push_str("Accept: */*\r\n");
    }
    if !has_accept_lang {
        head.push_str("Accept-Language: en-US,en;q=0.8\r\n");
    }
    head.push_str("Accept-Encoding: br, gzip, deflate\r\nConnection: close\r\n");
    if no_cache && !has_cache_ctrl {
        head.push_str("Cache-Control: no-cache\r\nPragma: no-cache\r\n");
    }
    for (k, v) in custom_headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    if !cookie_header.is_empty() {
        head.push_str("Cookie: ");
        head.push_str(cookie_header);
        head.push_str("\r\n");
    }
    if let Some(body) = body {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    } else if matches!(method, "POST" | "PUT" | "PATCH") {
        head.push_str("Content-Length: 0\r\n");
    }
    head.push_str("\r\n");
    let total = head.len().saturating_add(body.map_or(0, <[u8]>::len));
    if total > 96 * 1024 {
        return Err("Script request exceeds 96 KiB wire limit".into());
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(head.as_bytes());
    if let Some(body) = body {
        out.extend_from_slice(body);
    }
    Ok(out)
}

fn cookie_site(domain: &str) -> &[u8] {
    psl::domain(domain.as_bytes()).map_or(domain.as_bytes(), |domain| domain.as_bytes())
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn default_cookie_path(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
        .unwrap_or("/")
}

fn cookie_path_matches(path: &str, cookie_path: &str) -> bool {
    path == cookie_path
        || path
            .strip_prefix(cookie_path)
            .is_some_and(|suffix| cookie_path.ends_with('/') || suffix.starts_with('/'))
}

fn validate_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("Unsupported URL scheme: {}", url.scheme()));
    }
    if url.host().is_none() {
        return Err("URL has no host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs containing credentials are not supported".into());
    }
    Ok(())
}

trait Transport: Read + Write {}
impl<T: Read + Write> Transport for T {}

/// Enforce an overall deadline even when a peer keeps sending small fragments.
struct TimedSocket {
    socket: TcpStream,
    deadline: Instant,
}

impl TimedSocket {
    fn remaining(&self) -> io::Result<Duration> {
        remaining(self.deadline).map(|duration| duration.min(IO_TIMEOUT))
    }
}

impl Read for TimedSocket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}

impl Write for TimedSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline exceeded"))
}

fn connect(url: &Url, deadline: Instant) -> Result<TimedSocket, String> {
    let host = match url.host().ok_or("URL has no host")? {
        url::Host::Domain(host) => host.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    let port = url.port_or_known_default().ok_or("URL has no port")?;
    // Platform DNS has no portable timeout API. Bound the caller's wait; a slow
    // resolver thread exits when the platform resolver eventually returns.
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = (host.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| addresses.collect::<Vec<_>>());
        let _ = send.send(result);
    });
    let mut addresses = receive
        .recv_timeout(
            remaining(deadline)
                .map_err(|e| e.to_string())?
                .min(IO_TIMEOUT),
        )
        .map_err(|e| format!("DNS lookup did not complete: {e}"))?
        .map_err(|e| format!("DNS lookup failed: {e}"))?;
    // Try IPv4 first where IPv6 routes may be advertised but unavailable.
    addresses.sort_by_key(|address| address.is_ipv6());
    let mut last_error = "host resolved to no addresses".to_owned();
    for address in addresses.into_iter().take(16) {
        let timeout = remaining(deadline)
            .map_err(|e| e.to_string())?
            .min(IO_TIMEOUT);
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(socket) => return Ok(TimedSocket { socket, deadline }),
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("Connection failed: {last_error}"))
}

#[allow(clippy::too_many_arguments)]
fn request(
    url: &Url,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    deadline: Instant,
    cookie_header: &str,
    no_cache: bool,
    max_body: usize,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<(Head, Vec<u8>), String> {
    let request = request_bytes_with_options(url, body, cookie_header, no_cache)?;
    let socket = connect(url, deadline)?;
    let mut stream: Box<dyn Transport> = if url.scheme() == "https" {
        let hostname = match url.host().ok_or("URL has no host")? {
            url::Host::Domain(host) => host.to_owned(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => ip.to_string(),
        };
        let server_name = rustls::pki_types::ServerName::try_from(hostname)
            .map_err(|e| format!("Invalid TLS server name: {e}"))?;
        let client = rustls::ClientConnection::new(config, server_name)
            .map_err(|e| format!("TLS initialization: {e}"))?;
        Box::new(rustls::StreamOwned::new(client, socket))
    } else {
        Box::new(socket)
    };
    stream
        .write_all(&request)
        .map_err(|e| format!("Request or TLS handshake failed: {e}"))?;
    stream
        .flush()
        .map_err(|e| format!("Request flush failed: {e}"))?;
    read_response_progress(BufReader::new(stream), max_body, progress)
}

#[cfg(test)]
fn request_bytes(url: &Url, body: Option<&str>) -> Result<Vec<u8>, String> {
    request_bytes_with_options(url, body, "", false)
}

#[cfg(test)]
#[allow(dead_code)]
fn request_bytes_with_cookies(
    url: &Url,
    body: Option<&str>,
    cookie_header: &str,
) -> Result<Vec<u8>, String> {
    request_bytes_with_options(url, body, cookie_header, false)
}

fn request_bytes_with_options(
    url: &Url,
    body: Option<&str>,
    cookie_header: &str,
    no_cache: bool,
) -> Result<Vec<u8>, String> {
    let mut target = url.path().to_owned();
    if target.is_empty() {
        target.push('/');
    }
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    let mut host = url.host().ok_or("URL has no host")?.to_string();
    if let Some(port) = url.port() {
        host.push_str(&format!(":{port}"));
    }
    let method = if body.is_some() { "POST" } else { "GET" };
    let mut request = format!(
        "{method} {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: text/html,application/xhtml+xml,image/png;q=0.8,*/*;q=0.5\r\nAccept-Language: en-US,en;q=0.8\r\nAccept-Encoding: br, gzip, deflate\r\nConnection: close\r\n"
    );
    if no_cache {
        request.push_str("Cache-Control: no-cache\r\nPragma: no-cache\r\n");
    }
    if !cookie_header.is_empty() {
        request.push_str("Cookie: ");
        request.push_str(cookie_header);
        request.push_str("\r\n");
    }
    if let Some(body) = body {
        if body.len() > MAX_REQUEST {
            return Err("Form body exceeds 64 KiB request limit".into());
        }
        request.push_str(&format!(
            "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    if let Some(body) = body {
        request.push_str(body);
    }
    if request.len() > MAX_REQUEST {
        return Err("Request exceeds 64 KiB limit".into());
    }
    Ok(request.into_bytes())
}

#[derive(Debug)]
struct Head {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn limited_line(reader: &mut impl BufRead, remaining: &mut usize) -> Result<Vec<u8>, String> {
    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| format!("Response read failed: {e}"))?;
        if available.is_empty() {
            return Err("Unexpected EOF in response framing".into());
        }
        let end = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1);
        let count = end.unwrap_or(available.len());
        if count > *remaining {
            return Err("Response headers or chunk framing exceed limit".into());
        }
        *remaining -= count;
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            if !line.ends_with(b"\r\n") {
                return Err("HTTP framing requires CRLF line endings".into());
            }
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

fn read_head(reader: &mut impl BufRead, budget: &mut usize) -> Result<Head, String> {
    let status = limited_line(reader, budget)?;
    let status = std::str::from_utf8(&status).map_err(|_| "Invalid HTTP status line")?;
    let mut fields = status.splitn(3, ' ');
    if !matches!(fields.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err("Unsupported HTTP version".into());
    }
    let code = fields.next().ok_or("Missing HTTP status")?;
    let status_text = fields
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(256)
        .collect::<String>();
    if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Invalid HTTP status".into());
    }
    let status = code.parse::<u16>().map_err(|_| "Invalid HTTP status")?;
    if !(100..=599).contains(&status) {
        return Err("Invalid HTTP status range".into());
    }
    let mut headers = Vec::new();
    loop {
        let line = limited_line(reader, budget)?;
        if line.is_empty() {
            break;
        }
        if headers.len() >= 100 {
            return Err("Too many HTTP headers".into());
        }
        let line =
            std::str::from_utf8(&line).map_err(|_| "Unsupported non-UTF-8 response header")?;
        let (key, value) = line.split_once(':').ok_or("Malformed HTTP header")?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err("Invalid HTTP header name".into());
        }
        if value
            .bytes()
            .any(|byte| byte < b' ' && byte != b'\t' || byte == 127)
        {
            return Err("Invalid HTTP header value".into());
        }
        let key = key.to_ascii_lowercase();
        if headers.iter().any(|(existing, _)| existing == &key)
            && matches!(
                key.as_str(),
                "content-length" | "transfer-encoding" | "content-encoding" | "location"
            )
        {
            return Err(format!("Ambiguous duplicate {key} header"));
        }
        headers.push((key, value.trim().to_owned()));
    }
    Ok(Head {
        status,
        status_text,
        headers,
    })
}

#[cfg(test)]
fn read_response(reader: impl BufRead) -> Result<(Head, Vec<u8>), String> {
    read_response_limited(reader, MAX_BODY)
}

#[cfg(test)]
fn read_response_limited(reader: impl BufRead, max_body: usize) -> Result<(Head, Vec<u8>), String> {
    read_response_progress(reader, max_body, &mut |_, _| {})
}

fn read_response_progress(
    reader: impl BufRead,
    max_body: usize,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<(Head, Vec<u8>), String> {
    read_response_progress_with_mode(reader, max_body, false, progress)
}

fn read_response_progress_with_mode(
    mut reader: impl BufRead,
    max_body: usize,
    head_only: bool,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<(Head, Vec<u8>), String> {
    let mut budget = MAX_HEADERS;
    let mut head = read_head(&mut reader, &mut budget)?;
    let mut interim = 0;
    while (100..200).contains(&head.status) {
        if head.status == 101 {
            return Err("HTTP protocol upgrades are unsupported".into());
        }
        interim += 1;
        if interim > 5 {
            return Err("Too many informational responses".into());
        }
        head = read_head(&mut reader, &mut budget)?;
    }
    if head_only || matches!(head.status, 204 | 304) {
        return Ok((head, Vec::new()));
    }
    let mut report = |received, total| {
        if head.status == 200 {
            progress(received, total);
        }
    };
    let bytes = if let Some(encoding) = head.header("transfer-encoding") {
        if head.header("content-length").is_some() {
            return Err("Ambiguous HTTP body length".into());
        }
        if !encoding.eq_ignore_ascii_case("chunked") {
            return Err(format!("Unsupported transfer encoding: {encoding}"));
        }
        report(0, None);
        read_chunked_progress(&mut reader, max_body, &mut report)?
    } else if let Some(length) = head.header("content-length") {
        if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Invalid Content-Length".into());
        }
        let length = length
            .parse::<usize>()
            .map_err(|_| "Invalid Content-Length")?;
        if length > max_body {
            return Err(format!("Response body exceeds {max_body} byte limit"));
        }
        let mut bytes = vec![0; length];
        report(0, Some(length));
        read_body_progress(&mut reader, &mut bytes, 0, Some(length), &mut report)?;
        bytes
    } else {
        report(0, None);
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 8192];
            let count = reader
                .read(&mut buffer)
                .map_err(|e| format!("Response body read failed: {e}"))?;
            if count == 0 {
                break;
            }
            if count > max_body.saturating_sub(bytes.len()) {
                return Err(format!("Response body exceeds {max_body} byte limit"));
            }
            bytes.extend_from_slice(&buffer[..count]);
            report(bytes.len(), None);
        }
        bytes
    };
    Ok((head, bytes))
}

fn read_body_progress(
    reader: &mut impl Read,
    bytes: &mut [u8],
    offset: usize,
    total: Option<usize>,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<(), String> {
    let mut received = 0;
    for chunk in bytes.chunks_mut(8192) {
        reader
            .read_exact(chunk)
            .map_err(|e| format!("Incomplete response body: {e}"))?;
        received += chunk.len();
        progress(offset + received, total);
    }
    Ok(())
}

fn read_chunked_progress(
    reader: &mut impl BufRead,
    max_body: usize,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut framing_budget = MAX_HEADERS;
    loop {
        let line = limited_line(reader, &mut framing_budget)?;
        let line = std::str::from_utf8(&line).map_err(|_| "Invalid chunk size")?;
        let size = line.split(';').next().unwrap_or("").trim();
        if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid chunk size".into());
        }
        let size = usize::from_str_radix(size, 16).map_err(|_| "Invalid chunk size")?;
        if size == 0 {
            // Trailers cannot alter the already selected response framing.
            while !limited_line(reader, &mut framing_budget)?.is_empty() {}
            return Ok(bytes);
        }
        if size > max_body.saturating_sub(bytes.len()) {
            return Err(format!("Response body exceeds {max_body} byte limit"));
        }
        let previous = bytes.len();
        bytes.resize(previous + size, 0);
        read_body_progress(reader, &mut bytes[previous..], previous, None, progress)?;
        let mut ending = [0; 2];
        reader
            .read_exact(&mut ending)
            .map_err(|e| format!("Incomplete chunk ending: {e}"))?;
        if ending != *b"\r\n" {
            return Err("Invalid chunk ending".into());
        }
    }
}

fn read_limited(reader: &mut impl Read, max_body: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((max_body + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Response body read failed: {e}"))?;
    if bytes.len() > max_body {
        return Err(format!("Response body exceeds {max_body} byte limit"));
    }
    Ok(bytes)
}

#[cfg(test)]
fn decode_content(bytes: Vec<u8>, encoding: Option<&str>) -> Result<Vec<u8>, String> {
    decode_content_limited(bytes, encoding, MAX_BODY)
}

fn decode_content_limited(
    mut bytes: Vec<u8>,
    encoding: Option<&str>,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let raw = encoding.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return Ok(bytes);
    }
    let codings: Vec<String> = raw
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if codings.len() > 3 {
        return Err("Too many chained content encodings".into());
    }
    for coding in codings.into_iter().rev() {
        bytes = match coding.as_str() {
            "identity" => bytes,
            "br" => mg_sparkle::brotli_decompress_limited(&bytes, max_body)?,
            "gzip" | "x-gzip" => read_limited(
                &mut flate2::read::MultiGzDecoder::new(bytes.as_slice()),
                max_body,
            )?,
            "deflate" => match read_limited(
                &mut flate2::read::ZlibDecoder::new(bytes.as_slice()),
                max_body,
            ) {
                Ok(out) => out,
                Err(err) if err.contains("byte limit") => return Err(err),
                Err(_) => read_limited(
                    &mut flate2::read::DeflateDecoder::new(bytes.as_slice()),
                    max_body,
                )?,
            },
            other => return Err(format!("Unsupported content encoding: {other}")),
        };
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;

    fn parse(bytes: &[u8]) -> Result<(Head, Vec<u8>), String> {
        read_response(BufReader::new(Cursor::new(bytes)))
    }

    #[test]
    fn release_progress_counts_body_bytes_without_redirects_or_framing() {
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Length: 20000\r\n\r\n".to_vec();
        response.extend(vec![b'x'; 20000]);
        let mut events = Vec::new();
        let (_, bytes) =
            read_response_progress(Cursor::new(response), MAX_BODY, &mut |n, total| {
                events.push((n, total))
            })
            .unwrap();
        assert_eq!(bytes.len(), 20000);
        assert_eq!(
            events,
            vec![
                (0, Some(20000)),
                (8192, Some(20000)),
                (16384, Some(20000)),
                (20000, Some(20000))
            ]
        );
        for response in [
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\n\r\nabcde"[..],
        ] {
            events.clear();
            let (_, bytes) = read_response_progress(Cursor::new(response), 5, &mut |n, total| events.push((n, total))).unwrap();
            assert_eq!(bytes, b"abcde");
            assert_eq!(events.first(), Some(&(0, None)));
            assert_eq!(events.last(), Some(&(5, None)));
            assert!(events.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        }
        events.clear();
        read_response_progress(
            Cursor::new(b"HTTP/1.1 302 Found\r\nContent-Length: 3\r\n\r\nabc"),
            3,
            &mut |n, total| events.push((n, total)),
        )
        .unwrap();
        assert!(events.is_empty());
        assert!(
            read_response_progress(
                Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabc"),
                3,
                &mut |_, _| {}
            )
            .is_err()
        );
        assert!(
            read_response_progress(
                Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nabc"),
                4,
                &mut |_, _| {}
            )
            .is_err()
        );
    }

    #[test]
    fn content_length_stops_without_waiting_for_close() {
        let (head, bytes) = parse(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/html\r\n\r\nhelloignored",
        )
        .unwrap();
        assert_eq!(head.status, 200);
        assert_eq!(head.header("content-type"), Some("text/html"));
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn chunks_extensions_trailers_and_informational_response() {
        let (_, body) = parse(b"HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;example=yes\r\nabc\r\n2\r\nde\r\n0\r\nExample: trailer\r\n\r\n").unwrap();
        assert_eq!(body, b"abcde");
    }

    #[test]
    fn rejects_ambiguous_truncated_and_oversized_responses() {
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Length: 2\r\n\r\nhello",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nshort",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nnot-hex\r\n",
            "HTTP/1.1 200 OK\nContent-Length: 0\n\n",
            "HTTP/1.1 101 Switching Protocols\r\n\r\n",
        ] {
            assert!(parse(response.as_bytes()).is_err(), "accepted {response:?}");
        }
        let huge_header = format!("HTTP/1.1 200 OK\r\nX: {}\r\n\r\n", "x".repeat(MAX_HEADERS));
        assert!(parse(huge_header.as_bytes()).is_err());
    }

    #[test]
    fn subresource_body_limits_cover_framing_and_decoding() {
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
            "HTTP/1.1 200 OK\r\n\r\nhello",
        ] {
            assert!(read_response_limited(Cursor::new(response), 4).is_err());
            assert_eq!(
                read_response_limited(Cursor::new(response), 5).unwrap().1,
                b"hello"
            );
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"expanded response").unwrap();
        let bytes = encoder.finish().unwrap();
        assert!(decode_content_limited(bytes, Some("gzip"), 4).is_err());
    }

    #[test]
    fn unsupported_urls_and_oversized_forms_fail_before_network() {
        assert!(
            fetch_release("http://127.0.0.1:1/")
                .unwrap_err()
                .contains("HTTPS")
        );
        assert!(fetch("file:///etc/passwd").unwrap_err().contains("scheme"));
        assert!(
            fetch("https://user:password@example.org")
                .unwrap_err()
                .contains("credentials")
        );
        let url = Url::parse("http://127.0.0.1:1/").unwrap();
        assert!(request_bytes(&url, Some(&"x".repeat(MAX_REQUEST))).is_err());
    }

    #[test]
    fn request_uses_host_port_query_and_no_fragment() {
        let url = Url::parse("http://[::1]:8080/a?q=one%20two#fragment").unwrap();
        let request = String::from_utf8(request_bytes(&url, Some("q=one+two")).unwrap()).unwrap();
        assert!(request.starts_with("POST /a?q=one%20two HTTP/1.1\r\nHost: [::1]:8080\r\n"));
        assert!(!request.contains("fragment"));
        assert!(request.ends_with("Content-Length: 9\r\n\r\nq=one+two"));
    }

    #[test]
    fn gzip_is_decoded_with_an_independent_output_limit() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"plain HTML document").unwrap();
        let encoded = encoder.finish().unwrap();
        assert_eq!(
            decode_content(encoded, Some("gzip")).unwrap(),
            b"plain HTML document"
        );
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&vec![b'x'; MAX_BODY + 1]).unwrap();
        assert!(decode_content(encoder.finish().unwrap(), Some("gzip")).is_err());

        // RFC 7932 Brotli decompression + independent output cap.
        let br_plain = vec![
            11, 9, 128, 112, 108, 97, 105, 110, 32, 72, 84, 77, 76, 32, 100, 111, 99, 117, 109,
            101, 110, 116, 3,
        ];
        assert_eq!(
            decode_content(br_plain, Some("br")).unwrap(),
            b"plain HTML document"
        );
        let br_bomb = vec![
            203, 255, 255, 63, 248, 37, 240, 226, 177, 64, 32, 247, 254, 15, 0, 0, 1, 120, 3,
        ];
        assert!(decode_content(br_bomb, Some("br")).is_err());

        // Both RFC 1950 zlib-wrapped and raw RFC 1951 deflate streams decode cleanly.
        let mut raw_def =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        raw_def.write_all(b"raw deflate payload").unwrap();
        assert_eq!(
            decode_content(raw_def.finish().unwrap(), Some("deflate")).unwrap(),
            b"raw deflate payload"
        );

        assert!(
            decode_content(vec![], Some("zstd"))
                .unwrap_err()
                .contains("Unsupported")
        );
    }

    #[test]
    fn local_server_redirects_post_to_get_and_preserves_query() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut received = Vec::new();
            for response in [
                "HTTP/1.1 303 See Other\r\nLocation: /result?q=rust\r\nContent-Length: 0\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nRust\r\n0\r\n\r\n",
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                if request.starts_with("POST") {
                    let mut body = [0; 6];
                    reader.read_exact(&mut body).unwrap();
                    assert_eq!(&body, b"q=rust");
                }
                received.push(request);
                socket.write_all(response.as_bytes()).unwrap();
            }
            received
        });
        let response = submit(&format!("http://{address}/start"), Some("q=rust")).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.url.path(), "/result");
        assert_eq!(response.url.query(), Some("q=rust"));
        assert_eq!(response.body, b"Rust");
        assert!(
            !response.resource_same_origin_chain,
            "top-level requests carry no resource-origin proof"
        );
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("POST /start HTTP/1.1"));
        assert!(requests[1].starts_with("GET /result?q=rust HTTP/1.1"));
    }

    #[test]
    fn socket_deadline_expires_even_before_io() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut socket = TimedSocket {
            socket,
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert_eq!(
            socket.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn redirect_loop_stops_at_the_documented_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..=MAX_REDIRECTS {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                socket
                    .write_all(
                        b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n",
                    )
                    .unwrap();
            }
        });
        let error = fetch(&format!("http://{address}/loop")).unwrap_err();
        assert!(error.contains("Redirect limit (8)"));
        server.join().unwrap();
    }

    #[test]
    fn temporary_redirect_preserves_post_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for status in [307, 200] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                assert!(first.starts_with("POST "));
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = [0; 6];
                reader.read_exact(&mut body).unwrap();
                assert_eq!(body, *b"q=rust");
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n"
                );
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        assert_eq!(
            submit(&format!("http://{address}/start"), Some("q=rust"))
                .unwrap()
                .status,
            200
        );
        server.join().unwrap();
    }

    #[test]
    fn cookies_respect_host_domain_path_and_secure_scope() {
        let mut jar = CookieJar::default();
        let origin = Url::parse("https://www.example.com/account/login").unwrap();
        jar.store(&origin, "host=one");
        jar.store(&origin, "site=two; Domain=.example.com; Path=/; Secure");
        jar.store(&origin, "deep=three; Path=/account/login");
        assert_eq!(jar.header(&origin), "deep=three; host=one; site=two");
        assert_eq!(
            jar.header(&Url::parse("https://www.example.com/account/profile").unwrap()),
            "host=one; site=two"
        );
        assert_eq!(
            jar.header(&Url::parse("https://www.example.com/accountant").unwrap()),
            "site=two"
        );
        assert_eq!(
            jar.header(&Url::parse("https://other.example.com/").unwrap()),
            "site=two"
        );
        assert_eq!(
            jar.header(&Url::parse("http://other.example.com/").unwrap()),
            ""
        );
        assert_eq!(
            jar.header(&Url::parse("https://notexample.com/").unwrap()),
            ""
        );
        assert_eq!(
            jar.header(&Url::parse("https://example.com.other.test/").unwrap()),
            ""
        );
    }

    #[test]
    fn cookies_reject_public_suffixes_unrelated_domains_and_unsafe_prefixes() {
        let mut jar = CookieJar::default();
        let origin = Url::parse("https://www.example.co.uk/").unwrap();
        for header in [
            "a=b; Domain=co.uk",
            "a=b; Domain=uk",
            "a=b; Domain=other.co.uk",
            "__Secure-a=b",
            "__Host-a=b; Secure",
            "__Host-a=b; Secure; Path=/; Domain=example.co.uk",
            "a=line\r\ninjection",
            "a=bad value",
            "a=b; Partitioned; Secure",
        ] {
            jar.store(&origin, header);
        }
        let github = Url::parse("https://one.github.io/").unwrap();
        jar.store(&github, "a=b; Domain=github.io");
        let plain = Url::parse("http://www.example.co.uk/").unwrap();
        jar.store(&plain, "a=b; Secure");
        assert!(jar.cookies.is_empty());
        jar.store(&origin, "__Host-good=value; Secure; Path=/");
        assert_eq!(jar.header(&origin), "__Host-good=value");
        jar.store(&origin, "protected=original; Secure; Path=/");
        jar.store(&plain, "protected=changed; Path=/");
        assert!(jar.header(&origin).contains("protected=original"));
    }

    #[test]
    fn cookies_expire_delete_replace_and_stay_bounded() {
        let mut jar = CookieJar::default();
        let origin = Url::parse("https://example.com/").unwrap();
        jar.store(&origin, "value=one; Path=/");
        jar.store(&origin, "value=two; Path=/");
        assert_eq!(jar.header(&origin), "value=two");
        jar.store(&origin, "value=deleted; Path=/; Max-Age=0");
        assert_eq!(jar.header(&origin), "");
        jar.store(
            &origin,
            "expired=value; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        );
        assert_eq!(jar.header(&origin), "");
        jar.store(
            &origin,
            "alive=value; Max-Age=60; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        );
        assert_eq!(jar.header(&origin), "alive=value");
        jar.store(&origin, "alive=deleted; Max-Age=-10");
        assert_eq!(jar.header(&origin), "");
        jar.store(
            &origin,
            &format!("oversized={}", "x".repeat(MAX_COOKIE_BYTES)),
        );
        assert!(jar.cookies.is_empty());
        for site in 0..6 {
            let origin = Url::parse(&format!("https://example{site}.com/")).unwrap();
            for cookie in 0..40 {
                jar.store(&origin, &format!("cookie{cookie}={}", "x".repeat(1000)));
            }
            assert!(jar.header(&origin).len() <= MAX_COOKIE_HEADER);
            assert!(
                jar.cookies
                    .iter()
                    .filter(|cookie| cookie.domain == origin.host_str().unwrap())
                    .count()
                    <= MAX_SITE_COOKIES
            );
        }
        assert_eq!(jar.cookies.len(), MAX_COOKIES);
    }

    #[test]
    fn session_carries_actual_set_cookie_through_redirect_and_next_navigation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let response = match index {
                    0 => {
                        assert!(!request.contains("Cookie:"));
                        "HTTP/1.1 302 Found\r\nLocation: /one\r\nSet-Cookie: first=one; Path=/\r\nContent-Length: 0\r\n\r\n"
                    }
                    1 => {
                        assert!(request.contains("Cookie: first=one\r\n"));
                        "HTTP/1.1 200 OK\r\nSet-Cookie: second=two; Path=/\r\nContent-Length: 0\r\n\r\n"
                    }
                    _ => {
                        assert!(request.contains("Cookie: first=one; second=two\r\n"));
                        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                    }
                };
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        let session = Session::new();
        assert_eq!(
            session
                .fetch(&format!("http://{address}/start"))
                .unwrap()
                .url
                .path(),
            "/one"
        );
        assert_eq!(
            session
                .clone()
                .fetch(&format!("http://{address}/two"))
                .unwrap()
                .status,
            200
        );
        assert!(Session::new().cookies.lock().unwrap().cookies.is_empty());
        server.join().unwrap();
    }

    fn resource_test_tls(trusted: bool) -> (Arc<rustls::ClientConfig>, Arc<rustls::ServerConfig>) {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let leaf =
            CertificateDer::from(include_bytes!("../../../tests/fixtures/net/server.der").to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            include_bytes!("../../../tests/fixtures/net/server-key.der").to_vec(),
        ));
        let server =
            rustls::ServerConfig::builder_with_provider(rustls_rustcrypto::provider().into())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![leaf], key)
                .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        if trusted {
            roots
                .add(CertificateDer::from(
                    include_bytes!("../../../tests/fixtures/net/ca.der").to_vec(),
                ))
                .unwrap();
        }
        (
            Arc::new(crate::tls_client_config(roots).unwrap()),
            Arc::new(server),
        )
    }

    fn resource_tls_server(
        listener: TcpListener,
        config: Arc<rustls::ServerConfig>,
        replies: Vec<String>,
    ) -> std::thread::JoinHandle<Vec<String>> {
        listener.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut requests = Vec::new();
            for reply in replies {
                let socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "Missing owned TLS request");
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let connection = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut stream = rustls::StreamOwned::new(connection, socket);
                let mut reader = BufReader::new(&mut stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => return requests, // Expected for rejected TLS handshakes.
                        Ok(_) => {
                            request.push_str(&line);
                            if line == "\r\n" {
                                break;
                            }
                        }
                    }
                }
                requests.push(request);
                stream.write_all(reply.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
            requests
        })
    }

    fn resource_limits(page: &str, kind: ResourceKind) -> RequestLimits {
        RequestLimits {
            https_only: false,
            origin: Some(Url::parse(page).unwrap()),
            resource_kind: kind,
            max_body: 512 * 1024,
            deadline: Instant::now() + Duration::from_secs(6),
        }
    }

    #[test]
    fn script_provenance_covers_every_redirect_not_only_the_final_origin() {
        for chain in [
            vec![false],
            vec![false, false],
            vec![true],
            vec![false, true],
            vec![true, false],
            vec![false, true, false],
        ] {
            let (client, tls) = resource_test_tls(true);
            let local = TcpListener::bind("127.0.0.1:0").unwrap();
            let foreign = TcpListener::bind("127.0.0.1:0").unwrap();
            let page = format!("https://localhost:{}/", local.local_addr().unwrap().port());
            let other = format!(
                "https://localhost:{}/",
                foreign.local_addr().unwrap().port()
            );
            let urls: Vec<_> = chain
                .iter()
                .enumerate()
                .map(|(i, foreign)| format!("{}hop{i}.js", if *foreign { &other } else { &page }))
                .collect();
            let (mut local_replies, mut foreign_replies) = (Vec::new(), Vec::new());
            for (index, foreign) in chain.iter().enumerate() {
                let reply = if let Some(next) = urls.get(index + 1) {
                    format!("HTTP/1.1 302 Found\r\nLocation: {next}\r\nContent-Length: 0\r\n\r\n")
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: 2\r\n\r\n0;".into()
                };
                if *foreign {
                    foreign_replies.push(reply);
                } else {
                    local_replies.push(reply);
                }
            }
            let a = resource_tls_server(local, tls.clone(), local_replies);
            let b = resource_tls_server(foreign, tls, foreign_replies);
            let response = submit_with_limits(
                &urls[0],
                None,
                client,
                &Mutex::new(CookieJar::default()),
                resource_limits(&page, ResourceKind::Script),
            )
            .unwrap();
            assert_eq!(response.url.as_str(), urls.last().unwrap());
            assert_eq!(response.body, b"0;");
            assert_eq!(
                response.resource_same_origin_chain,
                !chain.contains(&true),
                "chain={chain:?}"
            );
            assert_eq!(
                a.join().unwrap().len() + b.join().unwrap().len(),
                chain.len()
            );
        }
    }

    #[test]
    fn image_and_script_tls_origin_crossing_permanently_strips_request_and_response_cookies() {
        for kind in [ResourceKind::Image, ResourceKind::Script] {
            let (client, tls) = resource_test_tls(true);
            let origin = TcpListener::bind("127.0.0.1:0").unwrap();
            let other = TcpListener::bind("127.0.0.1:0").unwrap();
            let page = format!("https://localhost:{}/", origin.local_addr().unwrap().port());
            let cross = format!(
                "https://localhost:{}/cross",
                other.local_addr().unwrap().port()
            );
            let first = resource_tls_server(origin, tls.clone(), vec![
                format!("HTTP/1.1 302 Found\r\nLocation: {cross}\r\nSet-Cookie: before=accepted; Path=/; Secure\r\nContent-Length: 0\r\n\r\n"),
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nSet-Cookie: back=ignored; Path=/; Secure\r\nContent-Length: 3\r\n\r\nimg".into(),
                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".into(),
            ]);
            let second = resource_tls_server(
                other,
                tls,
                vec![format!(
                    "HTTP/1.1 302 Found\r\nLocation: {page}back\r\nSet-Cookie: cross=ignored; Path=/; Secure\r\nContent-Length: 0\r\n\r\n"
                )],
            );
            let cookies = Mutex::new(CookieJar::default());
            cookies
                .lock()
                .unwrap()
                .store(&Url::parse(&page).unwrap(), "seed=present; Path=/; Secure");
            let response = submit_with_limits(
                &format!("{page}start"),
                None,
                client.clone(),
                &cookies,
                resource_limits(&page, kind),
            )
            .unwrap();
            assert_eq!(response.body, b"img");
            assert_eq!(response.url.as_str(), format!("{page}back"));
            // A separate request keeps the shared first-party session. Suppression
            // belongs to the cross-origin chain, not a global destructive jar clear.
            submit_with_limits(
                &format!("{page}next"),
                None,
                client,
                &cookies,
                resource_limits(&page, kind),
            )
            .unwrap();
            let first = first.join().unwrap();
            let second = second.join().unwrap();
            assert_eq!(first.len(), 3);
            assert_eq!(second.len(), 1);
            assert!(first[0].contains("Cookie: seed=present\r\n"));
            assert!(!first[1].contains("Cookie:"));
            assert!(!second[0].contains("Cookie:"));
            assert!(first[2].contains("Cookie: seed=present; before=accepted\r\n"));
            let jar = &mut *cookies.lock().unwrap();
            assert_eq!(
                jar.header(&Url::parse(&page).unwrap()),
                "seed=present; before=accepted"
            );
        }
    }

    #[test]
    fn same_origin_image_and_stylesheet_tls_redirects_keep_session_cookies() {
        for kind in [
            ResourceKind::Image,
            ResourceKind::Stylesheet,
            ResourceKind::Script,
        ] {
            let (client, tls) = resource_test_tls(true);
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let page = format!(
                "https://localhost:{}/",
                listener.local_addr().unwrap().port()
            );
            let server = resource_tls_server(listener, tls, vec![
                "HTTP/1.1 302 Found\r\nLocation: /next\r\nSet-Cookie: next=kept; Path=/; Secure\r\nContent-Length: 0\r\n\r\n".into(),
                "HTTP/1.1 200 OK\r\nSet-Cookie: final=kept; Path=/; Secure\r\nContent-Length: 2\r\n\r\nok".into(),
            ]);
            let cookies = Mutex::new(CookieJar::default());
            cookies
                .lock()
                .unwrap()
                .store(&Url::parse(&page).unwrap(), "initial=kept; Path=/; Secure");
            let response =
                submit_with_limits(&page, None, client, &cookies, resource_limits(&page, kind))
                    .unwrap();
            assert_eq!(response.body, b"ok");
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].contains("Cookie: initial=kept\r\n"));
            assert!(requests[1].contains("Cookie: initial=kept; next=kept\r\n"));
            assert_eq!(
                cookies.lock().unwrap().header(&Url::parse(&page).unwrap()),
                "initial=kept; next=kept; final=kept"
            );
        }
    }

    #[test]
    fn direct_cross_origin_images_and_scripts_verify_tls_and_never_use_matching_domain_cookies() {
        for kind in [
            ResourceKind::Image,
            ResourceKind::Script,
            ResourceKind::Stylesheet,
        ] {
            for (host, trusted, succeeds) in [
                ("localhost", true, true),
                ("localhost", false, false),
                ("127.0.0.1", true, false),
            ] {
                let (client, tls) = resource_test_tls(trusted);
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let url = format!(
                    "https://{host}:{}/image",
                    listener.local_addr().unwrap().port()
                );
                let server = resource_tls_server(listener, tls, vec![
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nSet-Cookie: foreign=ignored; Path=/\r\nContent-Length: 3\r\n\r\nimg".into()
                ]);
                let cookies = Mutex::new(CookieJar::default());
                cookies
                    .lock()
                    .unwrap()
                    .store(&Url::parse(&url).unwrap(), "existing=not-sent; Path=/");
                let result = submit_with_limits(
                    &url,
                    None,
                    client,
                    &cookies,
                    resource_limits("https://localhost:1/", kind),
                );
                if succeeds {
                    assert_eq!(result.unwrap().body, b"img");
                    let requests = server.join().unwrap();
                    assert_eq!(requests.len(), 1);
                    assert!(!requests[0].contains("Cookie:"));
                } else {
                    let error = result.unwrap_err();
                    assert!(error.contains("certificate"), "{error}");
                    assert!(
                        error.contains(if trusted {
                            "not valid for name"
                        } else {
                            "UnknownIssuer"
                        }),
                        "{error}"
                    );
                    assert!(server.join().unwrap().is_empty());
                }
                assert_eq!(
                    cookies.lock().unwrap().header(&Url::parse(&url).unwrap()),
                    "existing=not-sent"
                );
            }
        }
    }

    #[test]
    fn automatic_resource_rejections_happen_before_connecting() {
        let (client, _) = resource_test_tls(true);
        for (scheme, kind) in [
            ("http", ResourceKind::Image),
            ("http", ResourceKind::Script),
            ("http", ResourceKind::Stylesheet),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!(
                "{scheme}://localhost:{}/blocked",
                listener.local_addr().unwrap().port()
            );
            let error = submit_with_limits(
                &url,
                None,
                client.clone(),
                &Mutex::new(CookieJar::default()),
                resource_limits("https://localhost:1/", kind),
            )
            .unwrap_err();
            assert!(
                error.contains(match kind {
                    ResourceKind::Image => "Images require HTTPS",
                    ResourceKind::Script => "Scripts require HTTPS",
                    ResourceKind::Stylesheet => "Stylesheets require HTTPS",
                }),
                "{error}"
            );
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "http://localhost:{}/expired",
            listener.local_addr().unwrap().port()
        );
        let mut limits = resource_limits(&url, ResourceKind::Image);
        limits.deadline = Instant::now() - Duration::from_secs(1);
        assert!(
            submit_with_limits(
                &url,
                None,
                client,
                &Mutex::new(CookieJar::default()),
                limits
            )
            .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn image_and_script_https_redirect_cannot_return_to_same_origin_http_exception() {
        for kind in [
            ResourceKind::Image,
            ResourceKind::Script,
            ResourceKind::Stylesheet,
        ] {
            let (client, tls) = resource_test_tls(true);
            let http = TcpListener::bind("127.0.0.1:0").unwrap();
            http.set_nonblocking(true).unwrap();
            let page = format!("http://localhost:{}/", http.local_addr().unwrap().port());
            let https = TcpListener::bind("127.0.0.1:0").unwrap();
            let image = format!(
                "https://localhost:{}/image",
                https.local_addr().unwrap().port()
            );
            let server = resource_tls_server(
                https,
                tls,
                vec![format!(
                    "HTTP/1.1 302 Found\r\nLocation: {page}downgrade\r\nContent-Length: 0\r\n\r\n"
                )],
            );
            let error = submit_with_limits(
                &image,
                None,
                client,
                &Mutex::new(CookieJar::default()),
                resource_limits(&page, kind),
            )
            .unwrap_err();
            assert!(error.contains("cannot downgrade from HTTPS"), "{error}");
            assert_eq!(server.join().unwrap().len(), 1);
            assert_eq!(http.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        }
    }

    #[test]
    fn stylesheet_tls_redirect_to_http_or_foreign_origin_enforces_https_and_cookie_isolation() {
        let (client, tls) = resource_test_tls(true);
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let page = format!("https://localhost:{}/", origin.local_addr().unwrap().port());
        let insecure = TcpListener::bind("127.0.0.1:0").unwrap();
        insecure.set_nonblocking(true).unwrap();
        let downgrade = format!(
            "http://localhost:{}/style.css",
            insecure.local_addr().unwrap().port()
        );
        let server = resource_tls_server(
            origin,
            tls.clone(),
            vec![format!(
                "HTTP/1.1 302 Found\r\nLocation: {downgrade}\r\nContent-Length: 0\r\n\r\n"
            )],
        );
        let error = submit_with_limits(
            &page,
            None,
            client.clone(),
            &Mutex::new(CookieJar::default()),
            resource_limits(&page, ResourceKind::Stylesheet),
        )
        .unwrap_err();
        assert!(error.contains("Stylesheets require HTTPS"), "{error}");
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(
            insecure.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );

        let origin2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let page2 = format!(
            "https://localhost:{}/",
            origin2.local_addr().unwrap().port()
        );
        let cdn = TcpListener::bind("127.0.0.1:0").unwrap();
        let cross = format!(
            "https://localhost:{}/style.css",
            cdn.local_addr().unwrap().port()
        );
        let first = resource_tls_server(
            origin2,
            tls.clone(),
            vec![format!(
                "HTTP/1.1 302 Found\r\nLocation: {cross}\r\nContent-Length: 0\r\n\r\n"
            )],
        );
        let second = resource_tls_server(
            cdn,
            tls,
            vec!["HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nSet-Cookie: cdn=ignored; Path=/; Secure\r\nContent-Length: 8\r\n\r\na{top:0}".into()],
        );
        let cookies = Mutex::new(CookieJar::default());
        cookies.lock().unwrap().store(
            &Url::parse(&cross).unwrap(),
            "cdn_existing=not-sent; Path=/",
        );
        let resp = submit_with_limits(
            &page2,
            None,
            client,
            &cookies,
            resource_limits(&page2, ResourceKind::Stylesheet),
        )
        .unwrap();
        assert_eq!(resp.body, b"a{top:0}");
        let cdn_reqs = second.join().unwrap();
        assert_eq!(cdn_reqs.len(), 1);
        assert!(!cdn_reqs[0].contains("Cookie:"));
        assert_eq!(first.join().unwrap().len(), 1);
        assert_eq!(
            cookies.lock().unwrap().header(&Url::parse(&cross).unwrap()),
            "cdn_existing=not-sent"
        );
    }

    #[test]
    fn rustcrypto_tls_accepts_trusted_certificate_and_rejects_untrusted_or_wrong_name() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let leaf =
            CertificateDer::from(include_bytes!("../../../tests/fixtures/net/server.der").to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            include_bytes!("../../../tests/fixtures/net/server-key.der").to_vec(),
        ));
        let config =
            rustls::ServerConfig::builder_with_provider(rustls_rustcrypto::provider().into())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![leaf], key)
                .unwrap();
        let config = Arc::new(config);

        for (host, trusted, succeeds, downgrade) in [
            ("localhost", true, true, false),
            ("localhost", false, false, false),
            ("127.0.0.1", true, false, false),
            ("localhost", true, false, true),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server_config = config.clone();
            let server = std::thread::spawn(move || {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let connection = rustls::ServerConnection::new(server_config).unwrap();
                let mut stream = rustls::StreamOwned::new(connection, socket);
                let mut reader = BufReader::new(&mut stream);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => return,
                        Ok(_) if line == "\r\n" => break,
                        Ok(_) => {}
                    }
                }
                let reply: &[u8] = if downgrade {
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\n\r\n"
                } else {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\ntrusted"
                };
                stream.write_all(reply).unwrap();
                stream.flush().unwrap();
            });
            let mut roots = rustls::RootCertStore::empty();
            if trusted {
                roots
                    .add(CertificateDer::from(
                        include_bytes!("../../../tests/fixtures/net/ca.der").to_vec(),
                    ))
                    .unwrap();
            }
            let client = Arc::new(crate::tls_client_config(roots).unwrap());
            let response = if downgrade {
                submit_with_config_policy(
                    &format!("https://{host}:{port}/"),
                    None,
                    client,
                    &Mutex::new(CookieJar::default()),
                    true,
                )
            } else {
                submit_with_config(
                    &format!("https://{host}:{port}/"),
                    None,
                    client,
                    &Mutex::new(CookieJar::default()),
                )
            };
            if downgrade {
                assert!(
                    response
                        .unwrap_err()
                        .contains("Release download requires HTTPS")
                );
            } else if succeeds {
                assert_eq!(response.unwrap().body, b"trusted");
            } else {
                let error = response.unwrap_err();
                assert!(
                    error.contains("certificate"),
                    "unexpected TLS failure: {error}"
                );
                if trusted {
                    assert!(
                        error.contains("not valid for name"),
                        "unexpected hostname failure: {error}"
                    );
                } else {
                    assert!(
                        error.contains("UnknownIssuer"),
                        "unexpected trust failure: {error}"
                    );
                }
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn http_transport_decodes_br_gzip_deflate_and_strips_script_encoding_headers() {
        let br_payload = vec![
            11, 9, 128, 112, 108, 97, 105, 110, 32, 72, 84, 77, 76, 32, 100, 111, 99, 117, 109,
            101, 110, 116, 3,
        ];
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&br_payload).unwrap();
        let br_then_gz = gz.finish().unwrap();

        let mut raw_def =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        raw_def.write_all(b"{\"status\":\"ok\"}").unwrap();
        let deflate_payload = raw_def.finish().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut seen_accept = Vec::new();
            for (encoding, ct, body) in [
                ("br", "text/html; charset=utf-8", br_payload),
                ("br, gzip", "text/html; charset=utf-8", br_then_gz),
                ("deflate", "application/json", deflate_payload),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&mut socket);
                let mut req_text = String::new();
                loop {
                    let mut line = String::new();
                    let n = reader.read_line(&mut line).unwrap();
                    if n == 0 || line == "\r\n" {
                        break;
                    }
                    req_text.push_str(&line);
                }
                seen_accept.push(req_text);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).unwrap();
                socket.write_all(&body).unwrap();
                socket.flush().unwrap();
            }
            seen_accept
        });

        let client = Session::new();
        let page_url = format!("http://127.0.0.1:{port}/page");
        let resp1 = client.fetch(&page_url).unwrap();
        assert_eq!(resp1.body, b"plain HTML document");

        let resp2 = client
            .fetch(&format!("http://127.0.0.1:{port}/chained"))
            .unwrap();
        assert_eq!(resp2.body, b"plain HTML document");

        let script_resp = client.fetch_script_same_origin(
            &page_url,
            mg_sparkle::page_session::ScriptFetchRequest {
                request_id: 7,
                method: "GET".into(),
                url: format!("http://127.0.0.1:{port}/api.json"),
                headers: Vec::new(),
                body_b64: None,
            },
        );
        assert!(script_resp.error.is_none(), "{script_resp:?}");
        let decoded = base64_decode_forgiving(&script_resp.body_b64).unwrap();
        assert_eq!(decoded, b"{\"status\":\"ok\"}");
        assert!(
            !script_resp.headers.iter().any(|(k, _)| {
                k.eq_ignore_ascii_case("content-encoding")
                    || k.eq_ignore_ascii_case("content-length")
            }),
            "decoded script responses must strip transport framing headers: {:?}",
            script_resp.headers
        );

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        for req in requests {
            assert!(
                req.to_ascii_lowercase()
                    .contains("accept-encoding: br, gzip, deflate"),
                "expected Accept-Encoding header in {req:?}"
            );
        }
    }
}
