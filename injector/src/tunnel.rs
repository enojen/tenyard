use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{Either, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1::SendRequest;
use hyper::header::{AUTHORIZATION, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::certs::Leaves;
use crate::credentials;
use crate::server::TenboxServer;

const HEAD_LIMIT: usize = 8192;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

type Body = Either<Incoming, Full<Bytes>>;
type Upstream = Mutex<Option<SendRequest<Incoming>>>;

pub struct Injector {
    pub hosts: Vec<String>,
    pub leaves: Leaves,
    pub server: TenboxServer,
    pub upstream_tls: TlsConnector,
}

pub fn upstream_tls() -> TlsConnector {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    TlsConnector::from(Arc::new(config))
}

pub fn connect_host(line: &str, hosts: &[String]) -> Result<String, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let target = match words.as_slice() {
        ["CONNECT", target, version] if version.starts_with("HTTP/1.") => *target,
        _ => return Err(format!("not a CONNECT: {:?}", line.trim())),
    };
    let host = target
        .strip_suffix(":443")
        .ok_or_else(|| format!("CONNECT {target}: only port 443"))?;
    if !hosts.iter().any(|h| h == host) {
        return Err(format!("CONNECT {host}: not a credential host"));
    }
    Ok(host.to_string())
}

pub async fn serve(stream: TcpStream, injector: Arc<Injector>) -> Result<(), String> {
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let host = match timeout(IO_TIMEOUT, read_head(&mut reader)).await {
        Ok(Ok(None)) => return Ok(()),
        Ok(Ok(Some(line))) => connect_host(&line, &injector.hosts),
        Ok(Err(why)) => Err(why),
        Err(_) => Err("no CONNECT in time".to_string()),
    };
    let host = match host {
        Ok(host) => host,
        Err(why) => {
            let _ = reader
                .get_mut()
                .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err(why);
        }
    };
    let config = injector.leaves.config(&host, OffsetDateTime::now_utc())?;
    reader
        .get_mut()
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await
        .map_err(|e| e.to_string())?;
    let tls = timeout(IO_TIMEOUT, TlsAcceptor::from(config).accept(reader))
        .await
        .map_err(|_| format!("{host}: no TLS handshake in time"))?
        .map_err(|e| format!("{host}: TLS from the box: {e}"))?;
    let upstream: Arc<Upstream> = Arc::new(Mutex::new(None));
    let service = service_fn(move |request| {
        let (injector, host, upstream) =
            (Arc::clone(&injector), host.clone(), Arc::clone(&upstream));
        async move { Ok::<_, Infallible>(forward(request, &injector, &host, &upstream).await) }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(tls), service)
        .await
        .map_err(|e| e.to_string())
}

async fn read_head(reader: &mut BufReader<TcpStream>) -> Result<Option<String>, String> {
    let mut first = String::new();
    let mut read = 0;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 && read == 0 {
            return Ok(None);
        }
        read += n;
        if n == 0 || read > HEAD_LIMIT {
            return Err("CONNECT head too long or cut".to_string());
        }
        if first.is_empty() {
            first = line;
        } else if line == "\r\n" || line == "\n" {
            return Ok(Some(first));
        }
    }
}

async fn forward(
    request: Request<Incoming>,
    injector: &Injector,
    host: &str,
    upstream: &Upstream,
) -> Response<Body> {
    match send(request, injector, host, upstream).await {
        Ok(response) => response.map(Either::Left),
        Err(why) => {
            eprintln!("injector: {host}: {why}");
            let mut response = Response::new(Either::Right(Full::from(format!(
                "tenyard injector: {why}\n"
            ))));
            *response.status_mut() = StatusCode::BAD_GATEWAY;
            response
        }
    }
}

async fn send(
    mut request: Request<Incoming>,
    injector: &Injector,
    host: &str,
    upstream: &Upstream,
) -> Result<Response<Incoming>, String> {
    let found = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(credentials::find);
    if let Some(found) = found {
        let real = injector.server.value(&found.placeholder, host).await?;
        let mut filled = HeaderValue::from_str(&credentials::fill(&found, &real))
            .map_err(|_| "the real value is not a header value".to_string())?;
        filled.set_sensitive(true);
        request.headers_mut().insert(AUTHORIZATION, filled);
    }
    let mut slot = upstream.lock().await;
    if slot.as_ref().is_none_or(|sender| sender.is_closed()) {
        *slot = Some(open(&injector.upstream_tls, host).await?);
    }
    let sender = slot.as_mut().ok_or("no upstream connection")?;
    sender.ready().await.map_err(|e| format!("upstream: {e}"))?;
    sender
        .send_request(request)
        .await
        .map_err(|e| format!("upstream: {e}"))
}

async fn open(tls: &TlsConnector, host: &str) -> Result<SendRequest<Incoming>, String> {
    let tcp = timeout(IO_TIMEOUT, TcpStream::connect((host, 443)))
        .await
        .map_err(|_| format!("connect {host}: timed out"))?
        .map_err(|e| format!("connect {host}: {e}"))?;
    tcp.set_nodelay(true)
        .map_err(|e| format!("connect {host}: {e}"))?;
    let name = ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let stream = timeout(IO_TIMEOUT, tls.connect(name, tcp))
        .await
        .map_err(|_| format!("TLS to {host}: timed out"))?
        .map_err(|e| format!("TLS to {host}: {e}"))?;
    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| format!("upstream {host}: {e}"))?;
    let host = host.to_string();
    tokio::spawn(async move {
        if let Err(why) = connection.await {
            eprintln!("injector: upstream {host}: {why}");
        }
    });
    Ok(sender)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        vec!["github.com".to_string(), "api.github.com".to_string()]
    }

    #[test]
    fn a_connect_to_a_listed_host_on_443_is_taken() {
        let host = connect_host("CONNECT api.github.com:443 HTTP/1.1\r\n", &hosts());
        assert_eq!(host, Ok("api.github.com".to_string()));
    }

    #[test]
    fn anything_else_is_refused() {
        for line in [
            "CONNECT evil.example:443 HTTP/1.1\r\n",
            "CONNECT github.com:22 HTTP/1.1\r\n",
            "CONNECT github.com HTTP/1.1\r\n",
            "GET http://github.com/ HTTP/1.1\r\n",
            "CONNECT GitHub.com:443 HTTP/1.1 extra\r\n",
            "",
        ] {
            assert!(connect_host(line, &hosts()).is_err(), "{line:?}");
        }
    }
}
