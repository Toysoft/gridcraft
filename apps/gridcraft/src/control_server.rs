//! Localhost JSON-lines control server. Each connection may send many requests; each line
//! `{"id":…, "method":…, "params":…}` gets one reply line `{"id":…, "ok":…, …}`.
//!
//! The first line of a connection must authenticate with the app's token (#777; see
//! `gridcraft_mcp::control_auth`). A connection that doesn't, or later sends a line that isn't
//! JSON, is answered once and closed.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use gridcraft_mcp::control_auth;
use gridcraft_ui_egui::ControlRequest;
use serde_json::{Value as Json, json};

const MAX_LINE: usize = 64 * 1024 * 1024;
/// How long a new connection may take to send its token.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// The secret of this run: the one the user chose (`--control-token`, `GRIDCRAFT_CONTROL_TOKEN`),
/// else 256 random bits from the operating system, written to a file only the user can read.
fn make_token(port: u16, chosen: Option<String>) -> std::io::Result<String> {
    if let Some(t) = chosen.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        if t.len() < control_auth::MIN_TOKEN_LEN {
            return Err(std::io::Error::other(format!("the control token must have at least {} characters", control_auth::MIN_TOKEN_LEN)));
        }
        eprintln!("gridcraft: control channel on 127.0.0.1:{port} requires the token you set (clients: GRIDCRAFT_CONTROL_TOKEN)");
        return Ok(t);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(format!("no random numbers for the control token: {e}")))?;
    let token = control_auth::hex(&bytes);
    let path = control_auth::write_token_file(port, &token)?;
    eprintln!("gridcraft: control channel on 127.0.0.1:{port}; its token is in {} (gridcraft-cli reads it)", path.display());
    Ok(token)
}

pub fn start(port: u16, chosen_token: Option<String>, ctx: egui::Context) -> std::io::Result<Receiver<ControlRequest>> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let token: Arc<str> = make_token(port, chosen_token)?.into();
    let (tx, rx) = channel();
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = Arc::clone(&token);
            let _ = std::thread::Builder::new().name("control-conn".into()).spawn(move || serve(stream, &token, tx, ctx));
        }
    })?;
    log::info!("control channel listening on 127.0.0.1:{port}");
    Ok(rx)
}

/// Read the connection's first line (at most [`control_auth::MAX_AUTH_LINE`] bytes, within
/// [`AUTH_TIMEOUT`]) and check the token. `true` when the peer may go on.
fn authenticate(reader: &mut BufReader<TcpStream>, out: &mut TcpStream, token: &str) -> bool {
    let _ = out.set_read_timeout(Some(AUTH_TIMEOUT));
    let mut first = String::new();
    let read = reader.by_ref().take(control_auth::MAX_AUTH_LINE as u64).read_line(&mut first);
    let verdict = match read {
        Ok(n) if n > 0 && first.ends_with('\n') => control_auth::check_first_line(&first, token),
        _ => Err(json!({"ok": false, "error": "authentication required"})),
    };
    let allowed = verdict.is_ok();
    let reply = verdict.unwrap_or_else(|e| e);
    let _ = writeln!(out, "{reply}");
    let _ = out.set_read_timeout(None);
    allowed
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    if !authenticate(&mut reader, &mut out, token) {
        return;
    }
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            let _ = writeln!(out, "{}", json!({"ok": false, "error": "request too large"}));
            continue;
        }
        let msg: Json = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": format!("invalid JSON: {e}")}));
                break; // Not a client of ours; don't keep reading.
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Json::Null);
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
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
    #![allow(clippy::unwrap_used)]
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// A server on a loopback port with a stand-in UI that echoes each method.
    fn spawn() -> std::net::SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        std::thread::spawn(move || {
            for req in rx {
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, TOKEN, tx, egui::Context::default()));
            }
        });
        addr
    }

    fn lines(addr: std::net::SocketAddr, send: &str) -> Vec<Json> {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(send.as_bytes()).unwrap();
        // Replies up to the server closing the connection.
        BufReader::new(c).lines().map_while(Result::ok).map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    #[test]
    fn the_token_opens_the_connection() {
        let addr = spawn();
        let auth = control_auth::auth_line(1, TOKEN);
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(format!("{auth}\n{{\"id\":2,\"method\":\"ping\"}}\n").as_bytes()).unwrap();
        let mut r = BufReader::new(c);
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Json>(&line).unwrap()["ok"], json!(true));
        line.clear();
        r.read_line(&mut line).unwrap();
        let v: Json = serde_json::from_str(&line).unwrap();
        assert_eq!((v["id"].clone(), v["result"].clone()), (json!(2), json!("ping")));
    }

    #[test]
    fn anything_but_the_token_first_closes_the_connection() {
        let addr = spawn();
        let wrong = control_auth::auth_line(1, "ffffffffffffffffffffffffffffffff");
        for first in [wrong.as_str(), "{\"id\":1,\"method\":\"app.save\",\"params\":{\"path\":\"/tmp/x\"}}", "POST / HTTP/1.1", ""] {
            // The second request is never served: one refusal, then end of stream.
            let got = lines(addr, &format!("{first}\n{{\"id\":2,\"method\":\"ping\"}}\n"));
            assert_eq!(got.len(), 1, "{first}: {got:?}");
            assert_eq!(got[0]["ok"], json!(false), "{first}");
        }
    }

    #[test]
    fn an_unparsable_line_after_auth_closes_the_connection() {
        let addr = spawn();
        let auth = control_auth::auth_line(1, TOKEN);
        let got = lines(addr, &format!("{auth}\nnot json\n{{\"id\":3,\"method\":\"ping\"}}\n"));
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[1]["ok"], json!(false));
    }

    #[test]
    fn a_chosen_token_must_be_long_enough() {
        assert!(make_token(1, Some("short".into())).is_err());
        assert_eq!(make_token(1, Some(format!(" {TOKEN} "))).unwrap(), TOKEN);
    }
}
