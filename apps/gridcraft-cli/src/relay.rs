//! `relay`: the control channel for GridCraft in the browser.
//!
//! A browser page can't listen on a port, so the web app (`?control=ws://127.0.0.1:7980`) dials
//! out to this relay over a WebSocket, and the relay serves the ordinary JSON-lines control
//! protocol on `127.0.0.1:<port>` (`docs/control-protocol.md`). `gridcraft-cli send`,
//! `gridcraft-cli mcp --connect` and scripts then drive the browser exactly as they drive the
//! desktop app's `--control` port.
//!
//! The browser has no file system, so the relay does the file work on its behalf: a `path` on
//! `app.open`, `file.open` and `file.reload` is read here and sent as bytes, `app.save {path}`
//! comes back as bytes written here, and `ui.screenshot {path}` writes the PNG here.
//! `--watch FILE` reloads the file in the browser (`file.reload`) whenever it changes on disk,
//! and when the page (re)connects.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

/// The longest request line a control client may send.
const MAX_LINE: usize = 64 * 1024 * 1024;
/// The largest message the browser may send (a base64 screenshot or workbook).
const MAX_MESSAGE: u64 = 256 * 1024 * 1024;
/// The largest file the relay reads for the browser.
const MAX_FILE: u64 = 128 * 1024 * 1024;
/// The largest WebSocket upgrade request.
const MAX_HANDSHAKE: usize = 16 * 1024;
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
const WATCH_EVERY: Duration = Duration::from_millis(300);
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub struct Options {
    pub port: u16,
    pub ws_port: u16,
    pub watch: Vec<String>,
    /// Extra page origins allowed to connect, besides localhost ones.
    pub origins: Vec<String>,
}

/// The connected browser page and the requests waiting for its replies.
#[derive(Default)]
struct Hub {
    browser: Mutex<Option<Browser>>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    next_id: AtomicU64,
    /// Bumped on every new browser connection, so watchers resend their files.
    generation: AtomicU64,
    ws_port: u16,
}

struct Browser {
    generation: u64,
    out: TcpStream,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn failure(e: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": e.to_string()})
}

impl Hub {
    /// Sends one request to the browser and waits for its reply (`{ok, result | error}`).
    fn call(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        lock(&self.pending).insert(id, tx);
        let text = json!({"id": id, "method": method, "params": params}).to_string();
        let sent = match lock(&self.browser).as_mut() {
            Some(b) => write_frame(&mut b.out, 0x1, text.as_bytes()).map_err(|e| e.to_string()),
            None => Err(format!("no browser is connected: open GridCraft for the web with ?control=ws://127.0.0.1:{} in the address", self.ws_port)),
        };
        let reply = match sent {
            Ok(()) => rx.recv_timeout(REPLY_TIMEOUT).unwrap_or_else(|_| failure("timed out waiting for the browser (is its tab in the background?)")),
            Err(e) => failure(e),
        };
        lock(&self.pending).remove(&id);
        reply
    }

    /// A browser left: everything it owed fails now instead of timing out.
    fn disconnect(&self, generation: u64) {
        let mut browser = lock(&self.browser);
        if browser.as_ref().is_some_and(|b| b.generation == generation) {
            *browser = None;
            drop(browser);
            for (_, tx) in lock(&self.pending).drain() {
                let _ = tx.send(failure("the browser disconnected"));
            }
            eprintln!("gridcraft-cli relay: browser disconnected");
        }
    }
}

pub fn run(o: Options) -> Result<(), String> {
    let hub = Arc::new(Hub { ws_port: o.ws_port, ..Hub::default() });
    let control = TcpListener::bind(("127.0.0.1", o.port)).map_err(|e| format!("can't listen on 127.0.0.1:{}: {e}", o.port))?;
    let ws = TcpListener::bind(("127.0.0.1", o.ws_port)).map_err(|e| format!("can't listen on 127.0.0.1:{}: {e}", o.ws_port))?;
    eprintln!("gridcraft-cli relay: control on 127.0.0.1:{}, browser WebSocket on ws://127.0.0.1:{}", o.port, o.ws_port);
    eprintln!("gridcraft-cli relay: open GridCraft for the web with ?control=ws://127.0.0.1:{}", o.ws_port);

    for path in o.watch {
        let hub = hub.clone();
        spawn("relay-watch", move || watch(&hub, &path))?;
    }
    {
        let hub = hub.clone();
        spawn("relay-control", move || {
            for stream in control.incoming().flatten() {
                let hub = hub.clone();
                let _ = spawn("relay-client", move || serve_client(&hub, stream));
            }
        })?;
    }
    let origins = Arc::new(o.origins);
    for stream in ws.incoming().flatten() {
        let hub = hub.clone();
        let origins = origins.clone();
        let _ = spawn("relay-browser", move || serve_browser(&hub, stream, &origins));
    }
    Ok(())
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new().name(name.into()).spawn(f).map(|_| ()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- control clients

fn serve_client(hub: &Hub, stream: TcpStream) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    loop {
        let line = match read_line(&mut reader) {
            Ok(Some(l)) => l,
            Ok(None) => break,
            Err(e) => {
                let _ = writeln!(out, "{}", failure(e));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", failure(format!("invalid JSON: {e}")));
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let mut reply = request(hub, method, params);
        if let Some(o) = reply.as_object_mut() {
            o.insert("id".into(), id);
        }
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

/// One line, at most `MAX_LINE` bytes; `None` at the end of the stream.
fn read_line(r: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    let n = r.take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE {
        return Err("request too large".into());
    }
    String::from_utf8(buf).map(Some).map_err(|_| "request is not UTF-8".into())
}

/// Runs one control request against the browser, doing its file work here.
fn request(hub: &Hub, method: &str, params: Value) -> Value {
    match route(method, &params) {
        Route::Open { command, path } => match file_params(&path, &params) {
            Ok(p) => hub.call(command, p),
            Err(e) => failure(e),
        },
        Route::Save { path } => save(hub, &path),
        Route::Screenshot { path } => {
            let reply = hub.call("ui.screenshot", json!({}));
            write_screenshot(reply, &path)
        }
        Route::Forward => hub.call(method, params),
    }
}

#[derive(Debug, PartialEq)]
enum Route {
    /// Read `path` here and send its bytes to `command`.
    Open {
        command: &'static str,
        path: String,
    },
    Save {
        path: String,
    },
    Screenshot {
        path: String,
    },
    Forward,
}

fn route(method: &str, p: &Value) -> Route {
    let path = |v: &Value| v.get("path").and_then(Value::as_str).filter(|_| v.get("base64").is_none()).map(str::to_string);
    let inner = p.get("params").unwrap_or(&Value::Null);
    let command = p.get("command").or(p.get("id")).and_then(Value::as_str);
    match (method, command) {
        ("app.open" | "file.open", _) => path(p).map_or(Route::Forward, |path| Route::Open { command: "file.open", path }),
        ("file.reload", _) => path(p).map_or(Route::Forward, |path| Route::Open { command: "file.reload", path }),
        ("engine.execute" | "command", Some("file.open")) => path(inner).map_or(Route::Forward, |path| Route::Open { command: "file.open", path }),
        ("engine.execute" | "command", Some("file.reload")) => {
            path(inner).map_or(Route::Forward, |path| Route::Open { command: "file.reload", path })
        }
        ("app.save", _) => p.get("path").and_then(Value::as_str).map_or(Route::Forward, |path| Route::Save { path: path.to_string() }),
        ("ui.screenshot", _) => p.get("path").and_then(Value::as_str).map_or(Route::Forward, |path| Route::Screenshot { path: path.to_string() }),
        _ => Route::Forward,
    }
}

/// `{name, base64}` for the file at `path`, plus the request's other options (password, force).
fn file_params(path: &str, request: &Value) -> Result<Value, String> {
    let options = request.get("params").filter(|_| request.get("command").is_some()).unwrap_or(request);
    let bytes = read_file(path)?;
    let mut p = json!({"name": file_name(path), "base64": gridcraft_engine::io::base64_encode(&bytes)});
    for key in ["password", "force"] {
        if let (Some(v), Some(o)) = (options.get(key), p.as_object_mut()) {
            o.insert(key.into(), v.clone());
        }
    }
    Ok(p)
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if len > MAX_FILE {
        return Err(format!("{path}: larger than {} MB", MAX_FILE / (1024 * 1024)));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn save(hub: &Hub, path: &str) -> Value {
    let format = std::path::Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_else(|| "xlsx".into());
    let reply = hub.call("engine.execute", json!({"command": "file.saveBytes", "params": {"format": format}}));
    let Some(b64) = reply.pointer("/result/base64").and_then(Value::as_str) else { return reply };
    let Some(bytes) = gridcraft_engine::io::base64_decode(b64) else { return failure("the browser sent invalid base64") };
    match std::fs::write(path, &bytes) {
        Ok(()) => json!({"ok": true, "result": {"path": path, "bytes": bytes.len()}}),
        Err(e) => failure(format!("{path}: {e}")),
    }
}

fn write_screenshot(reply: Value, path: &str) -> Value {
    let Some(b64) = reply.pointer("/result/base64").and_then(Value::as_str) else { return reply };
    let Some(png) = gridcraft_engine::io::base64_decode(b64) else { return failure("the browser sent invalid base64") };
    match std::fs::write(path, &png) {
        Ok(()) => json!({"ok": true, "result": {"path": path, "width": reply.pointer("/result/width"), "height": reply.pointer("/result/height")}}),
        Err(e) => failure(format!("{path}: {e}")),
    }
}

// ---------------------------------------------------------------- watched files

fn watch(hub: &Hub, path: &str) {
    let stamp = |p: &str| std::fs::metadata(p).ok().map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
    let mut sent: Option<((SystemTime, u64), u64)> = None;
    let mut seen = stamp(path);
    eprintln!("gridcraft-cli relay: watching {path}");
    loop {
        std::thread::sleep(WATCH_EVERY);
        let now = stamp(path);
        // Wait until the file holds still for one interval: a writer may be halfway through.
        let settled = now.is_some() && now == seen;
        seen = now;
        let generation = hub.generation.load(Ordering::Relaxed);
        let connected = lock(&hub.browser).is_some();
        let (Some(current), true, true) = (now, settled, connected) else { continue };
        if sent == Some((current, generation)) {
            continue;
        }
        sent = Some((current, generation));
        let reply = match file_params(path, &json!({})) {
            Ok(p) => hub.call("file.reload", p),
            Err(e) => failure(e),
        };
        match reply.get("ok").and_then(Value::as_bool) {
            Some(true) => eprintln!("gridcraft-cli relay: reloaded {path}"),
            _ => eprintln!("gridcraft-cli relay: couldn't reload {path}: {}", reply.get("error").and_then(Value::as_str).unwrap_or("no reply")),
        }
    }
}

// ---------------------------------------------------------------- the browser (WebSocket)

fn serve_browser(hub: &Hub, stream: TcpStream, origins: &[String]) {
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut out = stream;
    let request = match read_handshake(&mut reader) {
        Ok(r) => r,
        Err(e) => {
            let _ = write!(out, "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            eprintln!("gridcraft-cli relay: refused a connection: {e}");
            return;
        }
    };
    let origin = header(&request, "origin");
    if !origin_allowed(origin.as_deref(), origins) {
        let _ = write!(out, "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        eprintln!("gridcraft-cli relay: refused a page from {} (allow it with --origin)", origin.as_deref().unwrap_or("an unknown origin"));
        return;
    }
    let Some(key) = header(&request, "sec-websocket-key") else {
        let _ = write!(out, "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    let accept = gridcraft_engine::io::base64_encode(&sha1_digest(format!("{key}{WS_GUID}").as_bytes()));
    if write!(out, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").is_err()
    {
        return;
    }
    let Ok(writer) = out.try_clone() else { return };
    let generation = hub.generation.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(mut old) = lock(&hub.browser).replace(Browser { generation, out: writer }) {
        // The newest page wins (a reload, or a second tab).
        let _ = write_frame(&mut old.out, 0x8, &[]);
        let _ = old.out.shutdown(std::net::Shutdown::Both);
    }
    eprintln!("gridcraft-cli relay: browser connected from {}", origin.as_deref().unwrap_or("an unknown origin"));
    loop {
        match read_message(&mut reader, &mut out) {
            Ok(Some(text)) => deliver(hub, &text),
            Ok(None) => break,
            Err(e) => {
                eprintln!("gridcraft-cli relay: browser connection error: {e}");
                break;
            }
        }
    }
    hub.disconnect(generation);
}

/// A reply from the browser goes to the request that's waiting for it.
fn deliver(hub: &Hub, text: &str) {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else { return };
    let Some(id) = v.get("id").and_then(Value::as_u64) else { return };
    if let Some(o) = v.as_object_mut() {
        o.remove("id");
    }
    if let Some(tx) = lock(&hub.pending).remove(&id) {
        let _ = tx.send(v);
    }
}

fn read_handshake(r: &mut impl BufRead) -> Result<String, String> {
    let mut request = String::new();
    loop {
        let mut line = String::new();
        let n = r.take(MAX_HANDSHAKE as u64).read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed during the handshake".into());
        }
        request.push_str(&line);
        if request.len() > MAX_HANDSHAKE {
            return Err("handshake too large".into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    let upgrade = header(&request, "upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    if !request.starts_with("GET ") || !upgrade {
        return Err("not a WebSocket upgrade".into());
    }
    Ok(request)
}

fn header(request: &str, name: &str) -> Option<String> {
    request.lines().skip(1).find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    })
}

/// Pages served from this machine may connect, and those named with `--origin`. Any other page
/// open in the browser is refused, so it can't read the files the relay serves.
fn origin_allowed(origin: Option<&str>, extra: &[String]) -> bool {
    let Some(origin) = origin else { return true };
    if extra.iter().any(|o| o.trim_end_matches('/').eq_ignore_ascii_case(origin)) {
        return true;
    }
    let host = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")).unwrap_or("");
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| format!("{h}]")).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

fn sha1_digest(data: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    sha1::Sha1::digest(data).to_vec()
}

/// One server frame (never masked).
fn write_frame(w: &mut impl Write, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut head = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => head.push(n as u8),
        n if n <= u16::MAX as usize => {
            head.push(126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

/// The next complete text message, answering pings on the way; `None` when the page closes.
fn read_message(r: &mut impl Read, w: &mut impl Write) -> Result<Option<String>, String> {
    let mut message: Vec<u8> = Vec::new();
    let mut in_message = false;
    loop {
        let mut head = [0u8; 2];
        if r.read_exact(&mut head).is_err() {
            return Ok(None);
        }
        let fin = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0f;
        if head[1] & 0x80 == 0 {
            return Err("unmasked frame from the browser".into());
        }
        let len = match head[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                r.read_exact(&mut b).map_err(|e| e.to_string())?;
                u64::from(u16::from_be_bytes(b))
            }
            127 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b).map_err(|e| e.to_string())?;
                u64::from_be_bytes(b)
            }
            n => u64::from(n),
        };
        if (message.len() as u64).saturating_add(len) > MAX_MESSAGE {
            return Err("message too large".into());
        }
        let mut mask = [0u8; 4];
        r.read_exact(&mut mask).map_err(|e| e.to_string())?;
        let mut payload = Vec::new();
        r.take(len).read_to_end(&mut payload).map_err(|e| e.to_string())?;
        if payload.len() as u64 != len {
            return Ok(None);
        }
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask.get(i % 4).copied().unwrap_or(0);
        }
        match opcode {
            0x0 if in_message => message.extend_from_slice(&payload),
            0x1 | 0x2 if !in_message => {
                message = payload;
                in_message = true;
            }
            0x8 => {
                let _ = write_frame(w, 0x8, &[]);
                return Ok(None);
            }
            0x9 => {
                if payload.len() > 125 {
                    return Err("oversized ping".into());
                }
                write_frame(w, 0xA, &payload).map_err(|e| e.to_string())?;
                continue;
            }
            0xA => continue,
            _ => return Err(format!("unexpected frame (opcode {opcode})")),
        }
        if fin {
            return String::from_utf8(message).map(Some).map_err(|_| "message is not UTF-8".into());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A masked client frame.
    fn client_frame(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut f = vec![if fin { 0x80 } else { 0 } | opcode];
        match payload.len() {
            n if n < 126 => f.push(0x80 | n as u8),
            n => {
                f.push(0x80 | 126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        f
    }

    #[test]
    fn accept_key_matches_the_rfc_example() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        assert_eq!(gridcraft_engine::io::base64_encode(&sha1_digest(format!("{key}{WS_GUID}").as_bytes())), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn reads_masked_fragmented_messages_and_answers_pings() {
        let long = "x".repeat(300);
        let mut input = client_frame(0x9, true, b"hi");
        input.extend(client_frame(0x1, false, b"{\"id\":"));
        input.extend(client_frame(0x0, true, b"1}"));
        input.extend(client_frame(0x1, true, long.as_bytes()));
        input.extend(client_frame(0x8, true, b""));
        let mut r = std::io::Cursor::new(input);
        let mut w = Vec::new();
        assert_eq!(read_message(&mut r, &mut w).unwrap().as_deref(), Some("{\"id\":1}"));
        assert_eq!(w, [0x8A, 2, b'h', b'i'], "a ping is answered with a pong");
        assert_eq!(read_message(&mut r, &mut w).unwrap(), Some(long));
        assert_eq!(read_message(&mut r, &mut w).unwrap(), None);
    }

    #[test]
    fn hostile_frames_are_refused() {
        let unmasked = vec![0x81, 1, b'x'];
        assert!(read_message(&mut std::io::Cursor::new(unmasked), &mut Vec::new()).is_err());
        let mut huge = vec![0x81, 0x80 | 127];
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        huge.extend_from_slice(&[0; 4]);
        assert!(read_message(&mut std::io::Cursor::new(huge), &mut Vec::new()).is_err());
        let stray_continuation = client_frame(0x0, true, b"x");
        assert!(read_message(&mut std::io::Cursor::new(stray_continuation), &mut Vec::new()).is_err());
        let truncated = client_frame(0x1, true, b"hello")[..6].to_vec();
        assert_eq!(read_message(&mut std::io::Cursor::new(truncated), &mut Vec::new()).unwrap(), None);
    }

    #[test]
    fn frames_use_the_right_length_encoding() {
        for (n, head) in [(5usize, vec![0x81, 5]), (300, vec![0x81, 126, 1, 44]), (70_000, vec![0x81, 127, 0, 0, 0, 0, 0, 1, 17, 112])] {
            let mut w = Vec::new();
            write_frame(&mut w, 0x1, &vec![b'a'; n]).unwrap();
            assert_eq!(w[..head.len()], head[..]);
            assert_eq!(w.len(), head.len() + n);
        }
    }

    #[test]
    fn only_local_pages_connect_unless_allowed() {
        assert!(origin_allowed(Some("http://127.0.0.1:8771"), &[]));
        assert!(origin_allowed(Some("http://localhost:8771"), &[]));
        assert!(origin_allowed(Some("http://[::1]:8771"), &[]));
        assert!(origin_allowed(None, &[]), "a non-browser client sends no origin");
        assert!(!origin_allowed(Some("https://evil.example"), &[]));
        assert!(!origin_allowed(Some("http://localhost.evil.example"), &[]));
        assert!(!origin_allowed(Some("null"), &[]));
        assert!(origin_allowed(Some("https://sheets.example.com"), &["https://sheets.example.com/".into()]));
    }

    #[test]
    fn handshake_needs_a_websocket_upgrade() {
        let ok = "GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nSec-WebSocket-Key: abc\r\n\r\n";
        let r = read_handshake(&mut std::io::Cursor::new(ok)).unwrap();
        assert_eq!(header(&r, "sec-websocket-key").as_deref(), Some("abc"));
        assert!(read_handshake(&mut std::io::Cursor::new("GET / HTTP/1.1\r\nHost: x\r\n\r\n")).is_err());
        assert!(read_handshake(&mut std::io::Cursor::new("GET / HTTP/1.1\r\n")).is_err());
        let endless = format!("GET / HTTP/1.1\r\nX: {}\r\n", "a".repeat(MAX_HANDSHAKE));
        assert!(read_handshake(&mut std::io::Cursor::new(endless)).is_err());
    }

    #[test]
    fn file_work_routes_to_the_relay() {
        assert_eq!(route("app.open", &json!({"path": "/a/b.xlsx"})), Route::Open { command: "file.open", path: "/a/b.xlsx".into() });
        assert_eq!(route("file.reload", &json!({"path": "/a/b.xlsx"})), Route::Open { command: "file.reload", path: "/a/b.xlsx".into() });
        assert_eq!(
            route("engine.execute", &json!({"command": "file.reload", "params": {"path": "b.xlsx"}})),
            Route::Open { command: "file.reload", path: "b.xlsx".into() }
        );
        assert_eq!(route("file.open", &json!({"name": "b.xlsx", "base64": "AA=="})), Route::Forward);
        assert_eq!(route("app.save", &json!({"path": "out.xlsx"})), Route::Save { path: "out.xlsx".into() });
        assert_eq!(route("ui.screenshot", &json!({"path": "s.png"})), Route::Screenshot { path: "s.png".into() });
        assert_eq!(route("ui.screenshot", &json!({})), Route::Forward);
        assert_eq!(route("engine.execute", &json!({"command": "cell.set"})), Route::Forward);
    }

    #[test]
    fn file_params_carry_options_and_read_the_file() {
        let dir = std::env::temp_dir().join(format!("gridcraft-relay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.xlsx");
        std::fs::write(&path, b"abc").unwrap();
        let path = path.to_string_lossy().to_string();
        let p = file_params(&path, &json!({"command": "file.reload", "params": {"path": path, "force": true}})).unwrap();
        assert_eq!(p, json!({"name": "m.xlsx", "base64": "YWJj", "force": true}));
        assert!(file_params(&format!("{path}.missing"), &json!({})).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn calls_fail_cleanly_without_a_browser() {
        let hub = Hub::default();
        let r = request(&hub, "engine.execute", json!({"command": "cell.set"}));
        assert_eq!(r["ok"], false);
        assert!(r["error"].as_str().unwrap().contains("no browser is connected"));
        assert!(lock(&hub.pending).is_empty());
    }

    #[test]
    fn replies_reach_the_waiting_request() {
        let hub = Hub::default();
        let (tx, rx) = channel();
        lock(&hub.pending).insert(7, tx);
        deliver(&hub, "{\"id\":7,\"ok\":true,\"result\":1}");
        assert_eq!(rx.recv().unwrap(), json!({"ok": true, "result": 1}));
        deliver(&hub, "{\"id\":99,\"ok\":true}");
        deliver(&hub, "not json");
    }
}
