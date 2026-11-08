use std::collections::HashMap;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

const CACHE_FOR: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct TenboxServer {
    authority: String,
    path: String,
    token: String,
    cache: Mutex<HashMap<(String, String), (String, Instant)>>,
}

impl TenboxServer {
    pub fn new(url: &str, token: &str) -> Result<TenboxServer, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| format!("--server {url}: not http://host:port/path"))?;
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        Ok(TenboxServer {
            authority: authority.to_string(),
            path: format!("/{path}"),
            token: token.trim().to_string(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn value(&self, placeholder: &str, host: &str) -> Result<String, String> {
        let key = (placeholder.to_string(), host.to_string());
        if let Some((value, at)) = self.cache.lock().await.get(&key)
            && at.elapsed() < CACHE_FOR
        {
            return Ok(value.clone());
        }
        let value = timeout(TIMEOUT, self.ask(placeholder, host))
            .await
            .map_err(|_| "the tenbox server did not answer in time".to_string())??;
        self.cache
            .lock()
            .await
            .insert(key, (value.clone(), Instant::now()));
        Ok(value)
    }

    async fn ask(&self, placeholder: &str, host: &str) -> Result<String, String> {
        let failed = |e: &dyn std::fmt::Display| format!("tenbox server: {e}");
        let stream = TcpStream::connect(&self.authority)
            .await
            .map_err(|e| failed(&e))?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| failed(&e))?;
        tokio::spawn(connection);
        let body = json!({ "placeholder": placeholder, "host": host }).to_string();
        let request = Request::post(&self.path)
            .header(HOST, &self.authority)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| failed(&e))?;
        let response = sender.send_request(request).await.map_err(|e| failed(&e))?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| failed(&e))?
            .to_bytes();
        if status != StatusCode::OK {
            return Err(format!("tenbox server answered {status} for {host}"));
        }
        let answer: Value = serde_json::from_slice(&bytes).map_err(|e| failed(&e))?;
        answer
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "tenbox server: no value in the answer".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct Fake {
        url: String,
        requests: Arc<std::sync::Mutex<Vec<String>>>,
        calls: Arc<AtomicUsize>,
    }

    async fn fake(status: &'static str, body: &'static str) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/run-credentials", listener.local_addr().unwrap());
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let (seen, count) = (Arc::clone(&requests), Arc::clone(&calls));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let mut buf = vec![0u8; 8192];
                let mut read = 0;
                loop {
                    let n = stream.read(&mut buf[read..]).await.unwrap();
                    read += n;
                    let text = String::from_utf8_lossy(&buf[..read]).to_string();
                    if n == 0 || text.ends_with('}') {
                        seen.lock().unwrap().push(text);
                        break;
                    }
                }
                let answer = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(answer.as_bytes()).await.unwrap();
            }
        });
        Fake {
            url,
            requests,
            calls,
        }
    }

    #[tokio::test]
    async fn the_value_comes_from_the_server_with_the_injector_token() {
        let fake = fake("200 OK", r#"{"value":"ghs_real"}"#).await;
        let server = TenboxServer::new(&fake.url, "inj-token\n").unwrap();
        let value = server.value("tbxph_abc", "api.github.com").await;
        assert_eq!(value, Ok("ghs_real".to_string()));
        let request = fake.requests.lock().unwrap()[0].clone();
        assert!(request.starts_with("POST /run-credentials HTTP/1.1\r\n"));
        assert!(request.contains("authorization: Bearer inj-token\r\n"));
        assert!(request.ends_with(r#"{"host":"api.github.com","placeholder":"tbxph_abc"}"#));
    }

    #[tokio::test]
    async fn a_value_is_asked_once_a_minute() {
        let fake = fake("200 OK", r#"{"value":"ghs_real"}"#).await;
        let server = TenboxServer::new(&fake.url, "t").unwrap();
        for _ in 0..3 {
            assert!(server.value("tbxph_abc", "github.com").await.is_ok());
        }
        assert!(server.value("tbxph_abc", "api.github.com").await.is_ok());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_unknown_placeholder_is_an_error() {
        let fake = fake("404 Not Found", r#"{"title":"no such run"}"#).await;
        let server = TenboxServer::new(&fake.url, "t").unwrap();
        let value = server.value("tbxph_abc", "github.com").await;
        assert!(value.unwrap_err().contains("404"));
    }

    #[tokio::test]
    async fn a_server_that_is_down_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/run-credentials", listener.local_addr().unwrap());
        drop(listener);
        let server = TenboxServer::new(&url, "t").unwrap();
        assert!(server.value("tbxph_abc", "github.com").await.is_err());
    }

    #[test]
    fn only_a_plain_http_url_is_taken() {
        assert!(TenboxServer::new("https://server/run-credentials", "t").is_err());
    }
}
