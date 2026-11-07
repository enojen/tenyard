//! procs: the CPU and memory of each process group in one box pod, on `/metrics`.
//!
//! It runs as a sidecar of the box in a pod that shares one process namespace, reads the pod's
//! processes every `--interval-ms` and counts them into the groups of `--groups`
//! (docs/reference.md, "Process metrics"). A broken groups file never stops the box: every process is then
//! `other`, and the error is on stderr.
//!
//!   procs [--listen 0.0.0.0:9256] [--interval-ms 250] [--groups /config/groups.conf]
//!         [--proc /proc] [--cgroup <path>]
//!
//! `--cgroup` reads only the processes under that cgroup v2 path (`/user.slice/...`), for a run
//! on a computer next to other processes; in a pod it is not needed.

mod account;
mod groups;
mod metrics;
mod procfs;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

use account::Account;
use groups::Rules;

const REQUEST_LIMIT: usize = 8192;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

struct Options {
    listen: String,
    interval: Duration,
    groups: PathBuf,
    proc_root: PathBuf,
    cgroup: Option<String>,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        listen: "0.0.0.0:9256".to_string(),
        interval: Duration::from_millis(250),
        groups: PathBuf::from("/config/groups.conf"),
        proc_root: PathBuf::from("/proc"),
        cgroup: None,
    };
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--listen" => o.listen = value,
            "--interval-ms" => {
                let ms: u64 = value
                    .parse()
                    .map_err(|_| format!("--interval-ms {value}: not a number"))?;
                o.interval = Duration::from_millis(ms.max(1));
            }
            "--groups" => o.groups = PathBuf::from(value),
            "--proc" => o.proc_root = PathBuf::from(value),
            "--cgroup" => o.cgroup = Some(value),
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    Ok(o)
}

fn rules(path: &PathBuf) -> Rules {
    let parsed = fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| Rules::parse(&t));
    parsed.unwrap_or_else(|why| {
        eprintln!(
            "procs: groups file {}: {why}; every process is other",
            path.display()
        );
        Rules::default()
    })
}

fn serve(listener: TcpListener, text: Arc<Mutex<String>>) {
    for stream in listener.incoming().flatten() {
        let body = text.lock().map(|t| t.clone()).unwrap_or_default();
        if let Err(why) = answer(stream, &body) {
            eprintln!("procs: request: {why}");
        }
    }
}

fn answer(mut stream: TcpStream, body: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < REQUEST_LIMIT {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
    }
    let response = if request.starts_with(b"GET /metrics ") || request.starts_with(b"GET /metrics?")
    {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
    };
    stream.write_all(response.as_bytes())
}

fn main() {
    let o = options().unwrap_or_else(|why| {
        eprintln!("procs: {why}");
        process::exit(2);
    });
    let rules = rules(&o.groups);
    let listener = TcpListener::bind(&o.listen).unwrap_or_else(|why| {
        eprintln!("procs: listen {}: {why}", o.listen);
        process::exit(1);
    });
    let text = Arc::new(Mutex::new(String::new()));
    let served = Arc::clone(&text);
    thread::spawn(move || serve(listener, served));

    let mut account = Account::default();
    let mut next = Instant::now();
    loop {
        account.step(procfs::snapshot(&o.proc_root, o.cgroup.as_deref()), &rules);
        if let Ok(mut t) = text.lock() {
            *t = metrics::render(&account);
        }
        next += o.interval;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now; // a slow read: start the next one at once, do not catch up
        }
    }
}
