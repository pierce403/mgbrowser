//! Bounded HTTP/1.1 transport for the research browser.
//!
//! TLS always uses our explicitly selected RustCrypto provider and public roots.
//! Cookies stay in memory for one browser session. There is no authentication,
//! proxy, or HTTP/2 support.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

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
const USER_AGENT: &str =
    "mgbrowser/0.1 (experimental Rust research browser; +https://mgbrowser.org)";

#[derive(Debug)]
pub struct Response {
    pub url: Url,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Shared, memory-only browser session. Clones share the same bounded cookie jar.
#[derive(Clone, Default)]
pub struct Session {
    cookies: Arc<Mutex<CookieJar>>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fetch(&self, url: &str) -> Result<Response, String> {
        self.submit(url, None)
    }

    pub fn submit(&self, url: &str, body: Option<&str>) -> Result<Response, String> {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config =
            crate::tls_client_config(roots).map_err(|e| format!("TLS configuration: {e}"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        submit_with_config(url, body, Arc::new(config), &self.cookies)
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

fn submit_with_config(
    url: &str,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    cookies: &Mutex<CookieJar>,
) -> Result<Response, String> {
    let mut url = Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let mut body = body;
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    for redirects in 0..=MAX_REDIRECTS {
        validate_url(&url)?;
        let cookie_header = cookies
            .lock()
            .map_err(|_| "Cookie session lock failed")?
            .header(&url);
        let (head, bytes) = request(&url, body, config.clone(), deadline, &cookie_header)?;
        {
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
        let bytes = decode_content(bytes, head.header("content-encoding"))?;
        return Ok(Response {
            url,
            status: head.status,
            content_type,
            body: bytes,
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
    expires: Option<SystemTime>,
}

impl CookieJar {
    fn prune(&mut self) {
        let now = SystemTime::now();
        self.cookies
            .retain(|cookie| cookie.expires.is_none_or(|expires| expires > now));
    }

    fn store(&mut self, url: &Url, header: &str) {
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
                "max-age" => {
                    max_age = value.trim().parse::<i64>().ok();
                }
                "expires" => {
                    cookie.expires = httpdate::parse_http_date(value.trim()).ok();
                }
                // Partition keys and script access are outside this transport.
                // HttpOnly data is never exposed through a scripting API.
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
        self.prune();
        let Some(host) = url.host_str() else {
            return String::new();
        };
        let mut matches = self
            .cookies
            .iter()
            .filter(|cookie| {
                (!cookie.secure || url.scheme() == "https")
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

fn request(
    url: &Url,
    body: Option<&str>,
    config: Arc<rustls::ClientConfig>,
    deadline: Instant,
    cookie_header: &str,
) -> Result<(Head, Vec<u8>), String> {
    let request = request_bytes_with_cookies(url, body, cookie_header)?;
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
    read_response(BufReader::new(stream))
}

#[cfg(test)]
fn request_bytes(url: &Url, body: Option<&str>) -> Result<Vec<u8>, String> {
    request_bytes_with_cookies(url, body, "")
}

fn request_bytes_with_cookies(
    url: &Url,
    body: Option<&str>,
    cookie_header: &str,
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
        "{method} {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: text/html,application/xhtml+xml,image/png;q=0.8,*/*;q=0.5\r\nAccept-Language: en-US,en;q=0.8\r\nAccept-Encoding: identity\r\nConnection: close\r\n"
    );
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
    Ok(Head { status, headers })
}

fn read_response(mut reader: impl BufRead) -> Result<(Head, Vec<u8>), String> {
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
    if matches!(head.status, 204 | 304) {
        return Ok((head, Vec::new()));
    }
    let bytes = if let Some(encoding) = head.header("transfer-encoding") {
        if head.header("content-length").is_some() {
            return Err("Ambiguous HTTP body length".into());
        }
        if !encoding.eq_ignore_ascii_case("chunked") {
            return Err(format!("Unsupported transfer encoding: {encoding}"));
        }
        read_chunked(&mut reader)?
    } else if let Some(length) = head.header("content-length") {
        if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Invalid Content-Length".into());
        }
        let length = length
            .parse::<usize>()
            .map_err(|_| "Invalid Content-Length")?;
        if length > MAX_BODY {
            return Err("Response body exceeds 8 MiB limit".into());
        }
        let mut bytes = vec![0; length];
        reader
            .read_exact(&mut bytes)
            .map_err(|e| format!("Incomplete response body: {e}"))?;
        bytes
    } else {
        read_limited(&mut reader)?
    };
    Ok((head, bytes))
}

fn read_chunked(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
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
        if size > MAX_BODY.saturating_sub(bytes.len()) {
            return Err("Response body exceeds 8 MiB limit".into());
        }
        let previous = bytes.len();
        bytes.resize(previous + size, 0);
        reader
            .read_exact(&mut bytes[previous..])
            .map_err(|e| format!("Incomplete chunk: {e}"))?;
        let mut ending = [0; 2];
        reader
            .read_exact(&mut ending)
            .map_err(|e| format!("Incomplete chunk ending: {e}"))?;
        if ending != *b"\r\n" {
            return Err("Invalid chunk ending".into());
        }
    }
}

fn read_limited(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_BODY + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Response body read failed: {e}"))?;
    if bytes.len() > MAX_BODY {
        return Err("Response body exceeds 8 MiB limit".into());
    }
    Ok(bytes)
}

fn decode_content(bytes: Vec<u8>, encoding: Option<&str>) -> Result<Vec<u8>, String> {
    match encoding
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "identity" => Ok(bytes),
        "gzip" => read_limited(&mut flate2::read::MultiGzDecoder::new(bytes.as_slice())),
        "deflate" => read_limited(&mut flate2::read::ZlibDecoder::new(bytes.as_slice())),
        other => Err(format!("Unsupported content encoding: {other}")),
    }
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
    fn unsupported_urls_and_oversized_forms_fail_before_network() {
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
        assert!(
            decode_content(vec![], Some("br"))
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

    #[test]
    fn rustcrypto_tls_accepts_trusted_certificate_and_rejects_untrusted_or_wrong_name() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let leaf =
            CertificateDer::from(include_bytes!("../tests/fixtures/net/server.der").to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            include_bytes!("../tests/fixtures/net/server-key.der").to_vec(),
        ));
        let config =
            rustls::ServerConfig::builder_with_provider(rustls_rustcrypto::provider().into())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![leaf], key)
                .unwrap();
        let config = Arc::new(config);

        for (host, trusted, succeeds) in [
            ("localhost", true, true),
            ("localhost", false, false),
            ("127.0.0.1", true, false),
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
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\ntrusted")
                    .unwrap();
                stream.flush().unwrap();
            });
            let mut roots = rustls::RootCertStore::empty();
            if trusted {
                roots
                    .add(CertificateDer::from(
                        include_bytes!("../tests/fixtures/net/ca.der").to_vec(),
                    ))
                    .unwrap();
            }
            let client = Arc::new(crate::tls_client_config(roots).unwrap());
            let response = submit_with_config(
                &format!("https://{host}:{port}/"),
                None,
                client,
                &Mutex::new(CookieJar::default()),
            );
            if succeeds {
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
}
