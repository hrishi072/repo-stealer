use crate::{check, Access};
use repo_stealer_git::Options;
use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Client,
};
use secrecy::ExposeSecret;
use serde::Serialize;
use std::{fs::File, future::Future, io::Read, path::Path, time::Duration};
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiIssue {
    pub code: String,
    pub status: Option<u16>,
    pub retry_after_seconds: Option<u64>,
}
impl ApiIssue {
    fn new(code: &str, status: Option<u16>) -> Self {
        Self {
            code: code.into(),
            status,
            retry_after_seconds: None,
        }
    }
}
pub struct Response {
    pub value: serde_json::Value,
    pub next: bool,
    pub classic_write_scopes: bool,
}
pub struct Api<'a> {
    client: Client,
    runtime: tokio::runtime::Runtime,
    access: Option<&'a Access>,
    options: &'a Options,
}
impl<'a> Api<'a> {
    pub fn new(
        access: Option<&'a Access>,
        ca: Option<&Path>,
        options: &'a Options,
    ) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot initialize cancellable API runtime"))?;
        let mut builder = Client::builder()
            .https_only(true)
            .no_proxy()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("RepoStealer/", env!("CARGO_PKG_VERSION")))
            .timeout(options.timeout.min(Duration::from_secs(15)))
            .connect_timeout(Duration::from_secs(10));
        if let Some(path) = ca {
            anyhow::ensure!(
                std::fs::symlink_metadata(path)?.is_file(),
                "CA bundle must be a regular PEM file"
            );
            let mut bytes = Vec::new();
            File::open(path)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 4 * 1024 * 1024, "CA bundle exceeds 4 MiB");
            let certs = reqwest::Certificate::from_pem_bundle(&bytes)
                .map_err(|_| anyhow::anyhow!("Invalid PEM CA bundle"))?;
            anyhow::ensure!(!certs.is_empty(), "CA bundle has no certificates");
            builder = builder.tls_certs_only(certs);
        }
        let client = builder
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot initialize verified HTTPS API client"))?;
        Ok(Self {
            client,
            runtime,
            access,
            options,
        })
    }
    /// Keep a single pinned operation alive between cancellation polls. Dropping
    /// it aborts local waiting; a mutation may already have reached the server.
    /// There is never a retry or a shortened valid-transfer deadline.
    fn wait<T>(
        &self,
        future: impl Future<Output = Result<T, reqwest::Error>>,
        failure: &str,
        status: Option<u16>,
    ) -> Result<T, ApiIssue> {
        self.runtime.block_on(async {
            let mut future = std::pin::pin!(future);
            loop {
                if check(self.options).is_err() {
                    return Err(ApiIssue::new("cancelled", status));
                }
                match tokio::time::timeout(Duration::from_millis(25), future.as_mut()).await {
                    Ok(result) => {
                        if check(self.options).is_err() {
                            return Err(ApiIssue::new("cancelled", status));
                        }
                        return result.map_err(|e| {
                            ApiIssue::new(if e.is_timeout() { "timeout" } else { failure }, status)
                        });
                    }
                    Err(_) => continue,
                }
            }
        })
    }
    fn read_body(
        &self,
        response: &mut reqwest::Response,
        bytes: &mut Vec<u8>,
        limit: usize,
    ) -> Result<(), ApiIssue> {
        let status = Some(response.status().as_u16());
        while let Some(chunk) = self.wait(response.chunk(), "api_body_unavailable", status)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(ApiIssue::new("api_body_exceeds_limit", status));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(())
    }
    // Existing plans use GET only. The separate one-shot creation service owns
    // crate-private writes; there is no renderer HTTP API or response-link follow.
    pub fn get(&self, url: Url) -> Result<Response, ApiIssue> {
        self.request(reqwest::Method::GET, url, None, 200)
    }
    pub(crate) fn write(
        &self,
        method: reqwest::Method,
        url: Url,
        body: serde_json::Value,
        status: u16,
    ) -> Result<Response, ApiIssue> {
        let body = if method == reqwest::Method::DELETE && body.is_null() {
            None
        } else {
            Some(body)
        };
        self.request(method, url, body, status)
    }
    fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        body: Option<serde_json::Value>,
        expected: u16,
    ) -> Result<Response, ApiIssue> {
        if check(self.options).is_err() {
            return Err(ApiIssue::new("cancelled", None));
        }
        let rulesets = url.path().ends_with("/rulesets");
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json");
        if let Some(access) = self.access {
            let text = Zeroizing::new(format!("Bearer {}", access.token.expose_secret()));
            let mut value =
                HeaderValue::from_str(&text).map_err(|_| ApiIssue::new("invalid_token", None))?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        if let Some(body) = body {
            request = request.header("Content-Type", "application/json").body(
                serde_json::to_vec(&body).map_err(|_| ApiIssue::new("invalid_request", None))?,
            );
        }
        let mut response =
            self.wait(async { request.send().await }, "https_request_failed", None)?;
        let status = response.status().as_u16();
        if status != expected {
            let limited = status == 429
                || (status == 403
                    && (response
                        .headers()
                        .get("x-ratelimit-remaining")
                        .is_some_and(|h| h == "0")
                        || response.headers().contains_key("retry-after")));
            let code = if limited {
                "rate_limited"
            } else {
                match status {
                    301..=399 => "redirect_refused",
                    401 => "authentication_rejected",
                    403 => "access_denied",
                    404 => "not_found_or_inaccessible",
                    _ => "api_request_failed",
                }
            };
            let mut issue = ApiIssue::new(code, Some(status));
            // Normalize only GitHub's explicit subscription-feature absence.
            // Never expose/store error bodies; all other 403s remain unknown.
            if rulesets && status == 403 && !limited {
                let mut body = zeroize::Zeroizing::new(Vec::new());
                let read = self.read_body(&mut response, &mut body, 2048);
                if let Err(issue) = &read {
                    if issue.code == "cancelled" {
                        return Err(issue.clone());
                    }
                }
                if read.is_ok() {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                        if value.get("message").and_then(serde_json::Value::as_str)==Some("Upgrade to GitHub Pro or make this repository public to enable this feature.") && value.get("documentation_url").and_then(serde_json::Value::as_str)==Some("https://docs.github.com/rest/repos/rules#get-all-repository-rulesets") {
                            issue.code="private_rulesets_feature_unavailable".into();
                        }
                    }
                }
            }
            issue.retry_after_seconds = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|n| n.min(86400));
            return Err(issue);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        self.read_body(&mut response, &mut bytes, 2 * 1024 * 1024)?;
        // Error bodies are never displayed or persisted; the sole feature-absence
        // classification above is bounded and exact. Successful JSON is also
        // bounded and only normalized allowlisted fields survive.
        let classic_write_scopes = response
            .headers()
            .get("x-oauth-scopes")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| {
                let scopes: Vec<_> = h.split(',').map(str::trim).collect();
                scopes.contains(&"repo") && scopes.contains(&"workflow")
            });
        let value = if bytes.is_empty() && expected == 204 {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|_| ApiIssue::new("invalid_json", Some(status)))?
        };
        let next = response
            .headers()
            .get("x-next-page")
            .is_some_and(|v| !v.as_bytes().is_empty() && v != "0")
            || response
                .headers()
                .get("link")
                .is_some_and(|v| v.to_str().is_ok_and(|s| s.contains("rel=\"next\"")));
        Ok(Response {
            value,
            next,
            classic_write_scopes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        time::Instant,
    };

    // Loopback HTTP isolates cancellation of a real socket. Production Api::new
    // retains mandatory HTTPS, certificate verification, origin checks and no retries.
    fn local_api(options: &Options) -> Api<'_> {
        Api {
            client: Client::builder()
                .no_proxy()
                .retry(reqwest::retry::never())
                .timeout(options.timeout.min(Duration::from_secs(5)))
                .build()
                .unwrap(),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            access: None,
            options,
        }
    }

    #[test]
    fn cancellation_interrupts_header_body_and_uncertain_mutation_waits() {
        for mode in ["headers", "body", "ruleset_body", "mutation"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = Url::parse(&format!(
                "http://{}/{}",
                listener.local_addr().unwrap(),
                if mode == "ruleset_body" {
                    "rulesets"
                } else {
                    "fixture"
                }
            ))
            .unwrap();
            let (ready_tx, ready_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = vec![];
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    assert_eq!(socket.read(&mut byte).unwrap(), 1);
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                assert!(request.starts_with(if mode == "mutation" {
                    b"POST "
                } else {
                    b"GET "
                }));
                if mode == "body" || mode == "ruleset_body" {
                    socket
                        .write_all(if mode == "ruleset_body" {
                            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 2\r\n\r\n{"
                        } else {
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{"
                        })
                        .unwrap();
                    socket.flush().unwrap();
                }
                ready_tx.send(()).unwrap();
                stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                listener.set_nonblocking(true).unwrap();
                assert!(
                    listener.accept().is_err(),
                    "Request was retried on another connection"
                );
            });
            let options = Options::default();
            let flag = options.cancelled.clone();
            let cancel = std::thread::spawn(move || {
                ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                // Let the client consume the headers in the body-stall case.
                std::thread::sleep(Duration::from_millis(50));
                let start = Instant::now();
                flag.store(true, Ordering::Relaxed);
                start
            });
            let api = local_api(&options);
            let result = if mode == "mutation" {
                api.write(
                    reqwest::Method::POST,
                    url,
                    serde_json::json!({"fixture":true}),
                    201,
                )
            } else {
                api.get(url)
            };
            assert_eq!(result.err().unwrap().code, "cancelled");
            assert!(
                cancel.join().unwrap().elapsed() < Duration::from_millis(500),
                "{mode} cancellation exceeded 500 ms"
            );
            drop(api);
            stop_tx.send(()).unwrap();
            server.join().unwrap();
        }
    }

    #[test]
    fn cancellation_polls_do_not_restart_the_request_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/fixture",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let (stop_tx, stop_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 8192];
            assert!(socket.read(&mut request).unwrap() > 0);
            stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let options = Options {
            timeout: Duration::from_millis(150),
            ..Options::default()
        };
        let api = local_api(&options);
        let start = Instant::now();
        let issue = api.get(url).err().unwrap();
        assert_eq!(issue.code, "timeout");
        assert!(start.elapsed() >= Duration::from_millis(100));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!options.cancelled.load(Ordering::Relaxed));
        drop(api);
        stop_tx.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn polling_preserves_slow_successful_waits_and_precancelled_flags() {
        let options = Options::default();
        let api = local_api(&options);
        let start = Instant::now();
        let value = api
            .wait(
                async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    Ok::<_, reqwest::Error>(42)
                },
                "transport",
                None,
            )
            .unwrap();
        assert_eq!(value, 42);
        assert!(start.elapsed() >= Duration::from_millis(150));
        assert!(!options.cancelled.load(Ordering::Relaxed));
        let flag = Arc::new(AtomicBool::new(true));
        let options = Options {
            cancelled: flag,
            ..options
        };
        let api = local_api(&options);
        assert_eq!(
            api.wait(
                std::future::pending::<Result<(), reqwest::Error>>(),
                "transport",
                None
            )
            .unwrap_err()
            .code,
            "cancelled"
        );
    }
}
