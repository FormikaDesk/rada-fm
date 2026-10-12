//! A web server of our own for tests of anything that downloads: it listens on a free port
//! of this machine, answers the routes it is given, and remembers what it was asked. No test
//! ever needs the network.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// Promise more bytes than are sent, then hang up (a download cut short).
    pub truncate: bool,
}

#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub headers: Vec<(String, String)>,
}

pub struct Server {
    pub base: String,
    pub routes: Arc<Mutex<HashMap<String, Reply>>>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (r, s, st) = (routes.clone(), seen.clone(), stop.clone());
        std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let (r, s) = (r.clone(), s.clone());
                        std::thread::spawn(move || serve(conn, r, s));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Server {
            base,
            routes,
            seen,
            stop,
        }
    }

    pub fn route(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(
            path.to_string(),
            Reply {
                status,
                body: body.into(),
                truncate: false,
            },
        );
    }

    pub fn route_truncated(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(
            path.to_string(),
            Reply {
                status: 200,
                body: body.into(),
                truncate: true,
            },
        );
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub fn count(&self, path: &str) -> usize {
        self.requests().iter().filter(|s| s.path == path).count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(
    mut conn: TcpStream,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    let _ = conn.set_nonblocking(false);
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match conn.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    seen.lock().unwrap().push(Seen {
        path: path.clone(),
        headers,
    });
    let key = path.split('?').next().unwrap_or("").to_string();
    let reply = routes.lock().unwrap().get(&key).cloned();
    let reply = reply.unwrap_or(Reply {
        status: 404,
        body: b"not found".to_vec(),
        truncate: false,
    });
    let declared = if reply.truncate {
        reply.body.len() * 2 + 10
    } else {
        reply.body.len()
    };
    let _ = write!(
        conn,
        "HTTP/1.1 {} X\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n",
        reply.status
    );
    let _ = conn.write_all(&reply.body);
    let _ = conn.flush();
}
