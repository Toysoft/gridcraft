//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.
//!
//! The first request on a connection must be `{"method": "auth", "params": {"token": …}}` with
//! the token from `gridcraft_mcp::auth` (published to `auth::token_path(port)`); anything else,
//! including a line that isn't JSON (an HTTP request from a web page), gets an error reply and
//! closes the connection.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gridcraft_mcp::auth;
use gridcraft_ui_egui::ControlRequest;
use serde_json::{Value as Json, json};

const MAX_LINE: usize = 64 * 1024 * 1024;

pub fn start(port: u16, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let token = match auth::env_token() {
        Some(t) => t,
        None => auth::new_token()?,
    };
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    // The port actually bound (`--control 0` picks a free one).
    let port = listener.local_addr()?.port();
    let token_file = auth::publish_token(port, &token)?;
    let (tx, rx) = channel();
    let token = Arc::new(token);
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = Arc::clone(&token);
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &token, tx, ctx));
        }
    })?;
    log::info!("control channel listening on 127.0.0.1:{port} (token in {})", token_file.display());
    Ok(rx)
}

/// What to do with a request line that arrives before the connection has authenticated: the
/// reply, and whether the connection is now authenticated (when not, it is closed).
fn authenticate(line: &str, token: &str) -> (Json, bool) {
    let Ok(msg) = serde_json::from_str::<Json>(line) else {
        return (json!({"ok": false, "error": "authenticate first: {\"method\":\"auth\",\"params\":{\"token\":…}}"}), false);
    };
    let id = msg.get("id").cloned().unwrap_or(Json::Null);
    let is_auth = msg.get("method").and_then(Json::as_str) == Some("auth");
    let given = msg.get("params").and_then(|p| p.get("token")).and_then(Json::as_str);
    match given {
        Some(t) if is_auth && auth::tokens_match(t, token) => (json!({"id": id, "ok": true, "result": {"authenticated": true}}), true),
        _ if is_auth => (json!({"id": id, "ok": false, "error": "wrong token"}), false),
        _ => (json!({"id": id, "ok": false, "error": "authenticate first: {\"method\":\"auth\",\"params\":{\"token\":…}}"}), false),
    }
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let reader = BufReader::new(read);
    let mut authenticated = false;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            let _ = writeln!(out, "{}", json!({"ok": false, "error": "request too large"}));
            if !authenticated {
                break;
            }
            continue;
        }
        if !authenticated {
            let (reply, ok) = authenticate(&line, token);
            if writeln!(out, "{reply}").is_err() || !ok {
                break;
            }
            authenticated = true;
            continue;
        }
        let msg: Json = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": format!("invalid JSON: {e}")}));
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Json::Null);
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
        // A repeated `auth` is answered here, never dispatched to the UI.
        if method == "auth" {
            if writeln!(out, "{}", json!({"id": id, "ok": true, "result": {"authenticated": true}})).is_err() {
                break;
            }
            continue;
        }
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let (req, rx) = ControlRequest::new(method, params);
        if tx.send(req).is_err() {
            break;
        }
        // Wake the UI thread: egui only runs frames when something happens.
        ctx.request_repaint();
        let mut reply = rx
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| json!({"ok": false, "error": "timed out waiting for the UI (is the window minimised?)"}));
        if let Some(o) = reply.as_object_mut() {
            o.insert("id".into(), id);
        }
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef";

    #[test]
    fn only_the_right_token_authenticates() {
        let (reply, ok) = authenticate(&json!({"id": 1, "method": "auth", "params": {"token": TOKEN}}).to_string(), TOKEN);
        assert!(ok);
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["id"], 1);
        for line in [
            json!({"id": 1, "method": "auth", "params": {"token": "wrong"}}).to_string(),
            json!({"id": 1, "method": "auth", "params": {}}).to_string(),
            json!({"id": 1, "method": "auth"}).to_string(),
            json!({"id": 1, "method": "app.save", "params": {"path": "/tmp/x.xlsx", "token": TOKEN}}).to_string(),
        ] {
            let (reply, ok) = authenticate(&line, TOKEN);
            assert!(!ok, "{line}");
            assert_eq!(reply["ok"], false, "{line}");
        }
    }

    #[test]
    fn an_http_request_is_refused() {
        // The first line a browser sends for `fetch("http://127.0.0.1:PORT", {method: "POST", body})`.
        let (reply, ok) = authenticate("POST / HTTP/1.1", TOKEN);
        assert!(!ok);
        assert_eq!(reply["ok"], false);
    }

    /// Over a real socket: an unauthenticated request is refused and the connection closed; after
    /// `auth`, requests reach the UI channel.
    #[test]
    fn requests_reach_the_ui_only_after_auth() {
        use std::io::Read as _;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                serve(stream.unwrap(), TOKEN, tx.clone(), egui::Context::default());
            }
        });

        // No auth: refused, closed, nothing dispatched.
        let mut s = TcpStream::connect(addr).unwrap();
        writeln!(s, "{}", json!({"id": 1, "method": "app.quit"})).unwrap();
        let mut rest = String::new();
        s.read_to_string(&mut rest).unwrap();
        assert!(rest.contains("authenticate first"), "{rest}");
        assert!(rx.try_recv().is_err());

        // Auth, then a request the UI answers.
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        writeln!(w, "{}", json!({"id": 1, "method": "auth", "params": {"token": TOKEN}})).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("\"authenticated\":true"), "{line}");
        writeln!(w, "{}", json!({"id": 2, "method": "ui.inspect"})).unwrap();
        let req = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(req.method, "ui.inspect");
        let _ = req.reply.send(json!({"ok": true, "result": {}}));
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("\"id\":2"), "{line}");
        drop((w, r));
        server.join().unwrap();
    }
}
