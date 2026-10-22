//! Встроенный обход DPI для Customcord.
//!
//! Поднимает локальный SOCKS5-прокси на 127.0.0.1 и переключает на него
//! WebView2 через `--proxy-server`. Когда обход включён, прокси разбивает
//! первый TLS ClientHello на несколько TCP-сегментов (тот же приём, что
//! используют GoodbyeDPI/Zapret): DPI не видит целый SNI и не блокирует
//! соединение. Дополнительно поддерживается внешний SOCKS5/HTTP-прокси —
//! он нужен там, где Discord блокируют по IP-адресу, а не по SNI.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const CONFIG_DIR: &str = "Customcord";
const CONFIG_FILE: &str = "bypass.json";

/// Таймаут установки соединения с конечным сервером.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
/// Сколько живёт результат DNS-запроса в кэше.
const DNS_TTL: Duration = Duration::from_secs(300);
/// Если DoH недоступен, не пытаемся снова некоторое время.
const DOH_BACKOFF: Duration = Duration::from_secs(180);
/// Пауза между сегментами ClientHello. Даёт DPI «забыть» первую часть.
const FRAGMENT_DELAY_MS: u64 = 7;

pub struct Bypass {
    /// Порт локального SOCKS5-сервера.
    pub port: u16,
    enabled: AtomicBool,
    doh: AtomicBool,
    upstream: Mutex<Option<String>>,
    dns_cache: Mutex<HashMap<String, DnsEntry>>,
    doh_disabled_until: Mutex<Option<Instant>>,
}

struct DnsEntry {
    ips: Vec<IpAddr>,
    expires: Instant,
}

#[derive(Clone)]
struct Settings {
    enabled: bool,
    doh: bool,
    upstream: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            doh: true,
            upstream: None,
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        base.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    fn load() -> Self {
        let mut cfg = Self::default();

        if let Ok(text) = std::fs::read_to_string(Self::path()) {
            cfg.enabled = json_bool(&text, "enabled").unwrap_or(cfg.enabled);
            cfg.doh = json_bool(&text, "doh").unwrap_or(cfg.doh);
            cfg.upstream = json_string(&text, "upstream").filter(|s| !s.trim().is_empty());
        }

        // Ручные переключатели для диагностики/автоматизации.
        if let Ok(value) = std::env::var("CUSTOMCORD_BYPASS") {
            let value = value.trim().to_ascii_lowercase();
            cfg.enabled = !matches!(value.as_str(), "0" | "off" | "false" | "no");
        }
        if let Ok(value) = std::env::var("CUSTOMCORD_DOH") {
            let value = value.trim().to_ascii_lowercase();
            cfg.doh = !matches!(value.as_str(), "0" | "off" | "false" | "no");
        }
        if let Ok(value) = std::env::var("CUSTOMCORD_UPSTREAM_PROXY") {
            let value = value.trim().to_string();
            if !value.is_empty() {
                cfg.upstream = Some(value);
            }
        }

        let args: Vec<String> = std::env::args().collect();
        if let Some(position) = args
            .iter()
            .position(|arg| arg == "--proxy" || arg == "--upstream-proxy")
        {
            if let Some(value) = args.get(position + 1).map(|value| value.trim()) {
                if !value.is_empty() {
                    cfg.upstream = Some(value.to_string());
                }
            }
        }
        if let Some(value) = args.iter().find_map(|arg| arg.strip_prefix("--proxy=")) {
            let value = value.trim();
            if !value.is_empty() {
                cfg.upstream = Some(value.to_string());
            }
        }
        if args.iter().any(|arg| arg == "--no-bypass") {
            cfg.enabled = false;
        }

        cfg
    }

    fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let upstream = match &self.upstream {
            Some(value) => format!("\"{}\"", json_escape(value)),
            None => "null".to_string(),
        };
        let body = format!(
            "{{\n  \"enabled\": {},\n  \"doh\": {},\n  \"upstream\": {}\n}}\n",
            self.enabled, self.doh, upstream
        );
        let _ = std::fs::write(path, body);
    }
}

/// Запускает локальный SOCKS5-сервер. Возвращает `None`, если порт
/// занять не удалось — тогда клиент просто работает напрямую.
pub fn start() -> Option<Arc<Bypass>> {
    let settings = Settings::load();
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();

    let state = Arc::new(Bypass {
        port,
        enabled: AtomicBool::new(settings.enabled),
        doh: AtomicBool::new(settings.doh),
        upstream: Mutex::new(settings.upstream.clone()),
        dns_cache: Mutex::new(HashMap::new()),
        doh_disabled_until: Mutex::new(None),
    });

    let accept_state = Arc::clone(&state);
    thread::Builder::new()
        .name("customcord-bypass-accept".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(client) => {
                        let state = Arc::clone(&accept_state);
                        let _ = thread::Builder::new()
                            .name("customcord-bypass-conn".to_string())
                            .spawn(move || handle_client(client, state));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        })
        .ok()?;

    Some(state)
}

impl Bypass {
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn doh_enabled(&self) -> bool {
        self.doh.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.persist();
    }

    pub fn set_doh(&self, enabled: bool) {
        self.doh.store(enabled, Ordering::Relaxed);
        *lock(&self.doh_disabled_until) = None;
        self.persist();
    }

    pub fn set_upstream(&self, upstream: Option<String>) {
        let upstream = upstream
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        *lock(&self.upstream) = upstream;
        self.persist();
    }

    pub fn upstream(&self) -> Option<String> {
        lock(&self.upstream).clone()
    }

    /// Аргумент Chromium, направляющий весь трафик WebView2 в наш прокси.
    pub fn chromium_arg(&self) -> String {
        format!(" --proxy-server=socks5://127.0.0.1:{}", self.port)
    }

    /// Начальное состояние для UI в настройках Discord.
    pub fn settings_json(&self) -> String {
        let upstream = match self.upstream() {
            Some(value) => format!("\"{}\"", json_escape(&value)),
            None => "null".to_string(),
        };
        format!(
            "{{\"enabled\":{},\"doh\":{},\"upstream\":{}}}",
            self.enabled(),
            self.doh_enabled(),
            upstream
        )
    }

    fn persist(&self) {
        Settings {
            enabled: self.enabled(),
            doh: self.doh_enabled(),
            upstream: self.upstream(),
        }
        .save();
    }

    fn resolve_doh(&self, host: &str) -> Vec<IpAddr> {
        let key = host.trim_end_matches('.').to_ascii_lowercase();

        if let Some(entry) = lock(&self.dns_cache).get(&key) {
            if entry.expires > Instant::now() {
                return entry.ips.clone();
            }
        }

        if let Some(until) = *lock(&self.doh_disabled_until) {
            if until > Instant::now() {
                return Vec::new();
            }
        }

        let ips = doh_lookup(&key).unwrap_or_default();
        if ips.is_empty() {
            *lock(&self.doh_disabled_until) = Some(Instant::now() + DOH_BACKOFF);
            return Vec::new();
        }

        *lock(&self.doh_disabled_until) = None;
        lock(&self.dns_cache).insert(
            key,
            DnsEntry {
                ips: ips.clone(),
                expires: Instant::now() + DNS_TTL,
            },
        );

        static DOH_LOGGED: AtomicBool = AtomicBool::new(false);
        if !DOH_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log_debug(&format!(
                "[BYPASS] DoH is working ({} address(es) for {})",
                ips.len(),
                host
            ));
        }

        ips
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// SOCKS5 (входящие подключения Chromium)
// ---------------------------------------------------------------------------

struct Target {
    host: String,
    port: u16,
}

fn handle_client(mut client: TcpStream, state: Arc<Bypass>) {
    let _ = client.set_nodelay(true);
    let _ = client.set_read_timeout(Some(Duration::from_secs(20)));

    let target = match socks_handshake(&mut client) {
        Ok(target) => target,
        Err(_) => return,
    };

    let upstream = state.upstream();
    let remote = match upstream.as_deref() {
        Some(value) => match parse_proxy(value) {
            Some(proxy) => connect_via_upstream(&proxy, &target.host, target.port),
            None => {
                crate::log_debug("[BYPASS] invalid upstream proxy URL");
                sock_reply_or_ignore(&mut client, 0x04);
                return;
            }
        },
        None => connect_direct(&state, &target.host, target.port),
    };

    let remote = match remote {
        Ok(stream) => stream,
        Err(err) => {
            crate::log_debug(&format!(
                "[BYPASS] connect {}:{} failed: {}",
                target.host, target.port, err
            ));
            sock_reply_or_ignore(&mut client, 0x05);
            return;
        }
    };

    if socks_reply(&mut client, 0x00).is_err() {
        return;
    }

    let _ = client.set_read_timeout(None);
    let _ = remote.set_read_timeout(None);
    let _ = client.set_nodelay(true);
    let _ = remote.set_nodelay(true);

    static CONNECTION_COUNT: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);
    let index = CONNECTION_COUNT.fetch_add(1, Ordering::Relaxed);
    if index < 30 {
        let mode = if upstream.is_some() {
            "upstream"
        } else if state.enabled() {
            "fragment"
        } else {
            "direct"
        };
        crate::log_debug(&format!(
            "[BYPASS] tunnel {}:{} ({})",
            target.host, target.port, mode
        ));
    }

    // Внешний прокси уже обходит DPI — фрагментация не нужна.
    let fragment = state.enabled() && upstream.is_none();
    relay(client, remote, fragment);
}

fn socks_handshake(client: &mut TcpStream) -> io::Result<Target> {
    let mut greeting = [0u8; 2];
    client.read_exact(&mut greeting)?;
    if greeting[0] != 0x05 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not socks5"));
    }

    let mut methods = vec![0u8; greeting[1] as usize];
    client.read_exact(&mut methods)?;
    client.write_all(&[0x05, 0x00])?; // без авторизации

    let mut request = [0u8; 4];
    client.read_exact(&mut request)?;
    if request[0] != 0x05 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad socks version"));
    }
    if request[1] != 0x01 {
        sock_reply_or_ignore(client, 0x07); // поддерживаем только CONNECT
        return Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported command"));
    }

    let host = match request[3] {
        0x01 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets)?;
            Ipv4Addr::from(octets).to_string()
        }
        0x04 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets)?;
            Ipv6Addr::from(octets).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len)?;
            let mut name = vec![0u8; len[0] as usize];
            client.read_exact(&mut name)?;
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => {
            sock_reply_or_ignore(client, 0x08);
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad address type"));
        }
    };

    let mut port = [0u8; 2];
    client.read_exact(&mut port)?;

    Ok(Target {
        host,
        port: u16::from_be_bytes(port),
    })
}

fn socks_reply(client: &mut TcpStream, code: u8) -> io::Result<()> {
    // VER REP RSV ATYP BND.ADDR BND.PORT
    client.write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
}

fn sock_reply_or_ignore(client: &mut TcpStream, code: u8) {
    let _ = socks_reply(client, code);
}

// ---------------------------------------------------------------------------
// Подключение к конечному серверу
// ---------------------------------------------------------------------------

fn connect_direct(state: &Bypass, host: &str, port: u16) -> io::Result<TcpStream> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return connect_socket(SocketAddr::new(ip, port));
    }

    let mut last_error: Option<io::Error> = None;
    let mut tried: Vec<IpAddr> = Vec::new();

    if state.enabled() && state.doh_enabled() {
        for ip in state.resolve_doh(host) {
            if tried.contains(&ip) {
                continue;
            }
            match connect_socket(SocketAddr::new(ip, port)) {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    last_error = Some(err);
                    tried.push(ip);
                }
            }
        }
    }

    // Системный DNS — запасной путь (и основной, если DoH отключён).
    for ip in system_resolve(host, port) {
        if tried.contains(&ip) || ip.is_unspecified() {
            continue;
        }
        match connect_socket(SocketAddr::new(ip, port)) {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_error = Some(err);
                tried.push(ip);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses resolved")
    }))
}

fn connect_socket(addr: SocketAddr) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn system_resolve(host: &str, port: u16) -> Vec<IpAddr> {
    match (host, port).to_socket_addrs() {
        Ok(addrs) => addrs.map(|addr| addr.ip()).collect(),
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Внешний прокси (SOCKS5 / HTTP CONNECT)
// ---------------------------------------------------------------------------

enum ProxyScheme {
    Socks5,
    Http,
}

struct Proxy {
    scheme: ProxyScheme,
    host: String,
    port: u16,
    user: Option<String>,
    password: Option<String>,
}

fn parse_proxy(input: &str) -> Option<Proxy> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let (scheme, rest) = match input.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("socks5".to_string(), input),
    };

    let scheme = match scheme.as_str() {
        "socks" | "socks5" | "socks5h" => ProxyScheme::Socks5,
        "http" | "https" => ProxyScheme::Http,
        _ => return None,
    };

    let (credentials, host_port) = match rest.rsplit_once('@') {
        Some((credentials, host_port)) => (Some(credentials), host_port),
        None => (None, rest),
    };

    let host_port = host_port.trim_end_matches('/');
    let default_port = match scheme {
        ProxyScheme::Socks5 => 1080,
        ProxyScheme::Http => 8080,
    };

    let (host, port) = if let Some(stripped) = host_port.strip_prefix('[') {
        // [IPv6]:port
        let (host, rest) = stripped.split_once(']')?;
        let port = rest
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_port);
        (host.to_string(), port)
    } else if let Some((host, port)) = host_port.rsplit_once(':') {
        match port.parse::<u16>() {
            Ok(port) => (host.to_string(), port),
            Err(_) => (host_port.to_string(), default_port),
        }
    } else {
        (host_port.to_string(), default_port)
    };

    if host.is_empty() {
        return None;
    }

    let (user, password) = match credentials.and_then(|value| value.split_once(':')) {
        Some((user, password)) => (
            Some(percent_decode(user)),
            Some(percent_decode(password)),
        ),
        None => (credentials.map(percent_decode), None),
    };

    Some(Proxy {
        scheme,
        host,
        port,
        user,
        password,
    })
}

fn connect_via_upstream(proxy: &Proxy, host: &str, port: u16) -> io::Result<TcpStream> {
    let mut stream = connect_proxy_socket(proxy)?;
    match proxy.scheme {
        ProxyScheme::Socks5 => socks5_upstream(&mut stream, proxy, host, port)?,
        ProxyScheme::Http => http_upstream(&mut stream, proxy, host, port)?,
    }
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(None);
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn connect_proxy_socket(proxy: &Proxy) -> io::Result<TcpStream> {
    let addrs: Vec<SocketAddr> = if let Ok(ip) = proxy.host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, proxy.port)]
    } else {
        (proxy.host.as_str(), proxy.port)
            .to_socket_addrs()
            .map(|iter| iter.collect())
            .unwrap_or_default()
    };

    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
                return Ok(stream);
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::AddrNotAvailable, "proxy address not resolved")
    }))
}

fn socks5_upstream(
    stream: &mut TcpStream,
    proxy: &Proxy,
    host: &str,
    port: u16,
) -> io::Result<()> {
    let use_auth = proxy.user.is_some();
    if use_auth {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02])?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00])?;
    }

    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice)?;
    if choice[0] != 0x05 || choice[1] == 0xFF {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "socks5 auth method rejected",
        ));
    }

    if choice[1] == 0x02 {
        let user = proxy.user.clone().unwrap_or_default();
        let password = proxy.password.clone().unwrap_or_default();
        if user.len() > 255 || password.len() > 255 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "proxy credentials too long",
            ));
        }
        let mut packet = Vec::with_capacity(3 + user.len() + password.len());
        packet.push(0x01);
        packet.push(user.len() as u8);
        packet.extend_from_slice(user.as_bytes());
        packet.push(password.len() as u8);
        packet.extend_from_slice(password.as_bytes());
        stream.write_all(&packet)?;

        let mut auth = [0u8; 2];
        stream.read_exact(&mut auth)?;
        if auth[1] != 0x00 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "socks5 auth failed",
            ));
        }
    } else if choice[1] != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected socks5 auth method",
        ));
    }

    let mut request = Vec::with_capacity(7 + host.len());
    request.extend_from_slice(&[0x05, 0x01, 0x00]);

    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(0x01);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(0x04);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            if host.len() > 255 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "host name too long",
                ));
            }
            request.push(0x03);
            request.push(host.len() as u8);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request)?;

    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    if head[1] != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("socks5 connect failed: {}", head[1]),
        ));
    }

    let skip = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            len[0] as usize
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad socks5 reply",
            ));
        }
    };
    let mut discard = vec![0u8; skip + 2];
    stream.read_exact(&mut discard)?;

    Ok(())
}

fn http_upstream(stream: &mut TcpStream, proxy: &Proxy, host: &str, port: u16) -> io::Result<()> {
    let auth = match (&proxy.user, &proxy.password) {
        (Some(user), password) => {
            let token = base64_encode(format!("{}:{}", user, password.as_deref().unwrap_or("")).as_bytes());
            format!("Proxy-Authorization: Basic {}\r\n", token)
        }
        (None, _) => String::new(),
    };

    let request = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: Keep-Alive\r\n{}User-Agent: Customcord\r\n\r\n",
        auth
    );
    stream.write_all(request.as_bytes())?;

    let mut response = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while response.len() < 8192 {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                response.push(byte[0]);
                if response.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }

    let head = String::from_utf8_lossy(&response);
    let status = head.lines().next().unwrap_or_default();
    let code = status.split_whitespace().nth(1).unwrap_or_default();
    if code == "200" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("proxy CONNECT failed: {}", status),
        ))
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Передача данных и фрагментация TLS ClientHello
// ---------------------------------------------------------------------------

fn relay(mut client: TcpStream, mut remote: TcpStream, fragment: bool) {
    if forward_first_flight(&mut client, &mut remote, fragment).is_err() {
        return;
    }

    let client_reader = match client.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let remote_writer = match remote.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };

    let upload = thread::spawn(move || {
        let mut reader = client_reader;
        let mut writer = remote_writer;
        let _ = io::copy(&mut reader, &mut writer);
        let _ = writer.shutdown(Shutdown::Write);
    });

    let _ = io::copy(&mut remote, &mut client);
    let _ = client.shutdown(Shutdown::Write);
    let _ = upload.join();
}

fn forward_first_flight(
    client: &mut TcpStream,
    remote: &mut TcpStream,
    fragment: bool,
) -> io::Result<()> {
    // ClientHello отправляется сразу после ответа SOCKS5, поэтому ждём
    // недолго: иначе замедлим соединения без начальных данных.
    client.set_read_timeout(Some(Duration::from_millis(250)))?;

    let mut buffer = [0u8; 16 * 1024];
    let count = match client.read(&mut buffer) {
        Ok(0) => return Ok(()),
        Ok(count) => count,
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(())
        }
        Err(err) => return Err(err),
    };

    client.set_read_timeout(None)?;

    if fragment && count >= 5 && buffer[0] == 0x16 {
        if let Some(position) = tls_split_position(&buffer[..count]) {
            let position = position.clamp(1, count - 1);

            remote.write_all(&buffer[..position])?;
            remote.flush()?;
            thread::sleep(Duration::from_millis(FRAGMENT_DELAY_MS));
            remote.write_all(&buffer[position..count])?;
            remote.flush()?;
            return Ok(());
        }
    }

    remote.write_all(&buffer[..count])?;
    remote.flush()
}

/// Ищет позицию внутри SNI-хоста. Разрез посреди имени ломает
/// сигнатуру `discord.com` для DPI, но сервер собирает поток обратно.
fn tls_split_position(buffer: &[u8]) -> Option<usize> {
    if buffer.len() < 5 || buffer[0] != 0x16 {
        return None;
    }

    let record_len = u16::from_be_bytes([buffer[3], buffer[4]]) as usize;
    let end = (5 + record_len).min(buffer.len());

    // TLS record header (5) + Handshake header (4) + client version (2) + random (32)
    if end < 5 + 4 + 2 + 32 {
        return None;
    }
    if buffer[5] != 0x01 {
        return None; // не ClientHello
    }

    let mut offset = 5 + 4 + 2 + 32;

    // session_id
    if offset >= end {
        return None;
    }
    offset += 1 + buffer[offset] as usize;

    // cipher_suites
    if offset + 2 > end {
        return None;
    }
    let cipher_len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
    offset += 2 + cipher_len;

    // compression_methods
    if offset >= end {
        return None;
    }
    offset += 1 + buffer[offset] as usize;

    // extensions
    if offset + 2 > end {
        return None;
    }
    let extensions_len = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) as usize;
    offset += 2;
    let extensions_end = (offset + extensions_len).min(end);

    while offset + 4 <= extensions_end {
        let extension_type = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]);
        let extension_len = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;
        let body = offset + 4;
        if body + extension_len > extensions_end {
            break;
        }

        if extension_type == 0x0000 && extension_len >= 5 {
            // server_name: list_len(2), name_type(1), name_len(2), name
            let mut name_offset = body + 2;
            let list_end = (name_offset + u16::from_be_bytes([buffer[body], buffer[body + 1]]) as usize)
                .min(body + extension_len);

            while name_offset + 3 <= list_end {
                let name_type = buffer[name_offset];
                let name_len =
                    u16::from_be_bytes([buffer[name_offset + 1], buffer[name_offset + 2]]) as usize;
                name_offset += 3;
                if name_offset + name_len > list_end {
                    break;
                }
                if name_type == 0 && name_len > 1 {
                    // Разрезаем ближе к середине имени, чтобы ни в одном
                    // сегменте не было полного домена.
                    return Some(name_offset + name_len / 2);
                }
                name_offset += name_len;
            }
        }

        offset = body + extension_len;
    }

    // SNI не нашли — режем посередине записи.
    if end > 1 {
        Some(end / 2)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// DNS over HTTPS
// ---------------------------------------------------------------------------

fn doh_lookup(host: &str) -> Option<Vec<IpAddr>> {
    if host.is_empty() || host.parse::<IpAddr>().is_ok() {
        return None;
    }

    let encoded = percent_encode_host(host);
    let endpoints: [(&str, String); 3] = [
        (
            "cloudflare-dns.com",
            format!("/dns-query?name={}&type=A", encoded),
        ),
        ("dns.google", format!("/resolve?name={}&type=A", encoded)),
        ("1.1.1.1", format!("/dns-query?name={}&type=A", encoded)),
    ];

    // Опрашиваем параллельно: если один резолвер заблокирован, берём ответ
    // от другого и не ждём двойной таймаут.
    let (sender, receiver) = std::sync::mpsc::channel();
    for (endpoint_host, path) in endpoints {
        let sender = sender.clone();
        thread::spawn(move || {
            if let Some(body) = winhttp_get(endpoint_host, &path) {
                let ips = parse_doh_json(&body);
                if !ips.is_empty() {
                    let _ = sender.send(ips);
                }
            }
        });
    }
    drop(sender);

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match receiver.recv_timeout(remaining) {
            Ok(ips) if !ips.is_empty() => return Some(ips),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

fn parse_doh_json(body: &str) -> Vec<IpAddr> {
    if let Some(start) = body.find("\"Status\"") {
        let tail = &body[start + 8..];
        if let Some(colon) = tail.find(':') {
            let value = tail[colon + 1..].trim_start();
            if !value.starts_with('0') {
                return Vec::new();
            }
        }
    }

    let mut ips = Vec::new();
    let mut cursor = 0;

    while let Some(position) = body[cursor..].find("\"data\":\"") {
        let absolute = cursor + position;
        let data_start = absolute + 8;
        let rest = &body[data_start..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let value = &rest[..end];

        // Перед "data" в том же объекте должно стоять "type":1 (A-запись).
        let object_start = body[..absolute].rfind('{').unwrap_or(0);
        let object = &body[object_start..absolute];
        let is_a = object
            .rfind("\"type\"")
            .and_then(|index| object[index + 6..].split(':').nth(1))
            .map(|value| value.trim_start().starts_with('1'))
            .unwrap_or(false);

        if is_a {
            if let Ok(ip) = value.parse::<Ipv4Addr>() {
                ips.push(IpAddr::V4(ip));
            }
        }

        cursor = data_start + end + 1;
    }

    ips
}

fn percent_encode_host(host: &str) -> String {
    let mut out = String::with_capacity(host.len());
    for byte in host.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn winhttp_get(host: &str, path: &str) -> Option<String> {
    use std::ffi::c_void;
    use std::ptr;

    type Handle = *mut c_void;

    const WINHTTP_ACCESS_TYPE_NO_PROXY: u32 = 1;
    const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
    const WINHTTP_FLAG_REFRESH: u32 = 0x0000_0100;

    #[link(name = "winhttp")]
    unsafe extern "system" {
        fn WinHttpOpen(
            agent: *const u16,
            access_type: u32,
            proxy: *const u16,
            proxy_bypass: *const u16,
            flags: u32,
        ) -> Handle;
        fn WinHttpConnect(
            session: Handle,
            server: *const u16,
            port: u16,
            reserved: u32,
        ) -> Handle;
        fn WinHttpOpenRequest(
            connect: Handle,
            verb: *const u16,
            object_name: *const u16,
            version: *const u16,
            referrer: *const u16,
            accept_types: *const *const u16,
            flags: u32,
        ) -> Handle;
        fn WinHttpSendRequest(
            request: Handle,
            headers: *const u16,
            headers_length: u32,
            optional: *mut c_void,
            optional_length: u32,
            total_length: u32,
            context: usize,
        ) -> i32;
        fn WinHttpReceiveResponse(request: Handle, reserved: *mut c_void) -> i32;
        fn WinHttpReadData(
            request: Handle,
            buffer: *mut c_void,
            bytes_to_read: u32,
            bytes_read: *mut u32,
        ) -> i32;
        fn WinHttpSetTimeouts(
            handle: Handle,
            resolve: i32,
            connect: i32,
            send: i32,
            receive: i32,
        ) -> i32;
        fn WinHttpCloseHandle(handle: Handle) -> i32;
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let agent = wide("Customcord/0.3");
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 2000, 2000, 2000, 3000);

        let server = wide(host);
        let connect = WinHttpConnect(session, server.as_ptr(), 443, 0);
        let mut result = None;

        if !connect.is_null() {
            let verb = wide("GET");
            let object = wide(path);
            let headers = wide("Accept: application/dns-json\r\n");

            let request = WinHttpOpenRequest(
                connect,
                verb.as_ptr(),
                object.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE | WINHTTP_FLAG_REFRESH,
            );

            if !request.is_null() {
                let sent = WinHttpSendRequest(
                    request,
                    headers.as_ptr(),
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    0,
                    0,
                );

                if sent != 0 && WinHttpReceiveResponse(request, ptr::null_mut()) != 0 {
                    let mut body = Vec::with_capacity(4096);
                    let mut chunk = [0u8; 8192];

                    loop {
                        let mut read = 0u32;
                        let ok = WinHttpReadData(
                            request,
                            chunk.as_mut_ptr().cast(),
                            chunk.len() as u32,
                            &mut read,
                        );
                        if ok == 0 || read == 0 {
                            break;
                        }
                        body.extend_from_slice(&chunk[..read as usize]);
                        if body.len() > 128 * 1024 {
                            break;
                        }
                    }

                    if !body.is_empty() {
                        result = Some(String::from_utf8_lossy(&body).into_owned());
                    }
                }

                WinHttpCloseHandle(request);
            }

            WinHttpCloseHandle(connect);
        }

        WinHttpCloseHandle(session);
        result
    }
}

#[cfg(not(target_os = "windows"))]
fn winhttp_get(_host: &str, _path: &str) -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// Простой JSON для файла настроек
// ---------------------------------------------------------------------------

fn json_bool(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\"", key);
    let start = text.find(&needle)? + needle.len();
    let tail = &text[start..];
    let colon = tail.find(':')?;
    let value = tail[colon + 1..].trim_start();
    Some(value.starts_with("true"))
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let start = text.find(&needle)? + needle.len();
    let tail = &text[start..];
    let colon = tail.find(':')?;
    let value = tail[colon + 1..].trim_start();
    if !value.starts_with('"') {
        return None;
    }

    let mut out = String::new();
    let mut chars = value[1..].chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return None,
            },
            other => out.push(other),
        }
    }
    None
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный TLS 1.2 ClientHello с одним SNI.
    fn client_hello(host: &str) -> (Vec<u8>, usize) {
        let host_bytes = host.as_bytes();

        let mut sni = Vec::new();
        sni.extend_from_slice(&((3 + host_bytes.len()) as u16).to_be_bytes());
        sni.push(0);
        sni.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
        sni.extend_from_slice(host_bytes);

        let mut extensions = Vec::new();
        extensions.extend_from_slice(&0u16.to_be_bytes());
        extensions.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&sni);

        let mut handshake = Vec::new();
        handshake.push(0x01);
        handshake.extend_from_slice(&[0, 0, 0]); // длина заполним ниже
        handshake.extend_from_slice(&[0x03, 0x03]);
        handshake.extend_from_slice(&[0u8; 32]);
        handshake.push(0); // session id
        handshake.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // cipher suites
        handshake.extend_from_slice(&[0x01, 0x00]); // compression
        handshake.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        handshake.extend_from_slice(&extensions);
        let handshake_len = (handshake.len() - 4) as u32;
        handshake[1] = (handshake_len >> 16) as u8;
        handshake[2] = (handshake_len >> 8) as u8;
        handshake[3] = handshake_len as u8;

        let mut record = Vec::new();
        record.push(0x16);
        record.extend_from_slice(&[0x03, 0x01]);
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);

        let name_offset = record
            .windows(host_bytes.len())
            .position(|window| window == host_bytes)
            .expect("sni offset");
        (record, name_offset)
    }

    #[test]
    fn splits_inside_sni_hostname() {
        let (hello, name_offset) = client_hello("discord.com");
        let split = tls_split_position(&hello).expect("split position");
        assert_eq!(split, name_offset + "discord.com".len() / 2);
        assert!(split > 5 && split < hello.len());
    }

    #[test]
    fn parses_doh_json_answers() {
        let body = r#"{"Status":0,"Answer":[{"name":"discord.com.","type":1,"TTL":17,"data":"162.159.137.232"},{"name":"discord.com.","type":1,"TTL":17,"data":"162.159.138.232"},{"name":"alias.","type":5,"data":"discord.com."}]}"#;
        let ips = parse_doh_json(body);
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0].to_string(), "162.159.137.232");
    }

    #[test]
    fn parses_upstream_proxy_urls() {
        let proxy = parse_proxy("socks5://user:pass@127.0.0.1:1080").unwrap();
        assert!(matches!(proxy.scheme, ProxyScheme::Socks5));
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 1080);
        assert_eq!(proxy.user.as_deref(), Some("user"));
        assert_eq!(proxy.password.as_deref(), Some("pass"));

        let proxy = parse_proxy("http://proxy.example.com").unwrap();
        assert!(matches!(proxy.scheme, ProxyScheme::Http));
        assert_eq!(proxy.port, 8080);

        assert!(parse_proxy("ftp://example.com").is_none());
    }

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn reads_settings_json() {
        let text = r#"{"enabled": false, "doh": true, "upstream": "socks5://127.0.0.1:9050"}"#;
        assert_eq!(json_bool(text, "enabled"), Some(false));
        assert_eq!(json_bool(text, "doh"), Some(true));
        assert_eq!(
            json_string(text, "upstream").as_deref(),
            Some("socks5://127.0.0.1:9050")
        );
    }
}
