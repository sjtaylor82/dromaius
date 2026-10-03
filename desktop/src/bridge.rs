//! TCP connection to the Android bridge (through `adb forward`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::BackendEvent;
use crate::protocol::{BRIDGE_PORT, FromBridge, ToBridge};

#[derive(Clone, Default)]
pub struct Bridge {
    stream: Arc<Mutex<Option<TcpStream>>>,
    next_req: Arc<Mutex<u64>>,
}

impl Bridge {
    /// Connects in the background, reconnecting whenever the link drops.
    pub fn spawn(&self, events: Sender<BackendEvent>) {
        let shared = self.stream.clone();
        std::thread::spawn(move || {
            loop {
                if let Ok(stream) = TcpStream::connect(("127.0.0.1", BRIDGE_PORT)) {
                    let _ = stream.set_nodelay(true);
                    if let Ok(writer) = stream.try_clone() {
                        *shared.lock().unwrap() = Some(writer);
                        if !read_loop(stream, &events) {
                            return;
                        }
                        *shared.lock().unwrap() = None;
                        if events.send(BackendEvent::BridgeDisconnected).is_err() {
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(700));
            }
        });
    }

    /// Sends a command; returns its request id, or None when disconnected.
    pub fn send(&self, cmd: ToBridge, aid: &dyn Fn(u64) -> u64) -> Option<u64> {
        let req = {
            let mut n = self.next_req.lock().unwrap();
            *n += 1;
            *n
        };
        let mut line = cmd.to_json(req, aid).to_string();
        line.push('\n');
        let mut guard = self.stream.lock().unwrap();
        let stream = guard.as_mut()?;
        if stream.write_all(line.as_bytes()).is_err() {
            *guard = None;
            return None;
        }
        Some(req)
    }
}

/// Forwards messages until the connection drops. Returns false if the app is shutting down.
fn read_loop(stream: TcpStream, events: &Sender<BackendEvent>) -> bool {
    // adb accepts the forward even when nothing listens on the device, then
    // closes it: we only count as connected once the bridge says hello.
    for line in BufReader::with_capacity(256 * 1024, stream).lines() {
        let Ok(line) = line else { return true };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<FromBridge>(&line) {
            Ok(msg) => {
                if events.send(BackendEvent::Bridge(msg)).is_err() {
                    return false;
                }
            }
            Err(e) => eprintln!("bad message from bridge: {e}"),
        }
    }
    true
}
