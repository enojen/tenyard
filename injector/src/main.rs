mod certs;
mod credentials;
mod server;
mod tunnel;

use std::sync::Arc;
use std::{env, fs, process};

use tokio::net::TcpListener;

use certs::Leaves;
use server::TenboxServer;
use tunnel::Injector;

struct Options {
    listen: String,
    ca_cert: String,
    ca_key: String,
    hosts: Vec<String>,
    server: String,
    server_token: String,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        listen: "0.0.0.0:3129".to_string(),
        ca_cert: String::new(),
        ca_key: String::new(),
        hosts: Vec::new(),
        server: String::new(),
        server_token: String::new(),
    };
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--listen" => o.listen = value,
            "--ca-cert" => o.ca_cert = value,
            "--ca-key" => o.ca_key = value,
            "--host" => o.hosts.push(value),
            "--server" => o.server = value,
            "--server-token" => o.server_token = value,
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    let missing = [
        ("--ca-cert", o.ca_cert.is_empty()),
        ("--ca-key", o.ca_key.is_empty()),
        ("--host", o.hosts.is_empty()),
        ("--server", o.server.is_empty()),
        ("--server-token", o.server_token.is_empty()),
    ];
    match missing.iter().find(|(_, empty)| *empty) {
        Some((flag, _)) => Err(format!("{flag} is needed")),
        None => Ok(o),
    }
}

fn read(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn injector(o: &Options) -> Result<Injector, String> {
    Ok(Injector {
        hosts: o.hosts.clone(),
        leaves: Leaves::load(&read(&o.ca_cert)?, &read(&o.ca_key)?)?,
        server: TenboxServer::new(&o.server, &read(&o.server_token)?)?,
        upstream_tls: tunnel::upstream_tls(),
    })
}

#[tokio::main]
async fn main() {
    let o = options().unwrap_or_else(|why| {
        eprintln!("injector: {why}");
        process::exit(2);
    });
    let injector = Arc::new(injector(&o).unwrap_or_else(|why| {
        eprintln!("injector: {why}");
        process::exit(1);
    }));
    let listener = TcpListener::bind(&o.listen).await.unwrap_or_else(|why| {
        eprintln!("injector: listen {}: {why}", o.listen);
        process::exit(1);
    });
    eprintln!("injector: {} for {}", o.listen, o.hosts.join(" "));
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(why) => {
                eprintln!("injector: accept: {why}");
                continue;
            }
        };
        let injector = Arc::clone(&injector);
        tokio::spawn(async move {
            if let Err(why) = tunnel::serve(stream, injector).await {
                eprintln!("injector: {why}");
            }
        });
    }
}
