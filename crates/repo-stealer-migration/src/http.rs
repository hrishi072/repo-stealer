use crate::{check, Access};
use repo_stealer_git::Options;
use reqwest::{
    blocking::Client,
    header::{HeaderValue, AUTHORIZATION},
};
use secrecy::ExposeSecret;
use serde::Serialize;
use std::{fs::File, io::Read, path::Path, time::Duration};
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
}
pub struct Api<'a> {
    client: Client,
    access: Option<&'a Access>,
    options: &'a Options,
}
impl<'a> Api<'a> {
    pub fn new(
        access: Option<&'a Access>,
        ca: Option<&Path>,
        options: &'a Options,
    ) -> anyhow::Result<Self> {
        let mut builder = Client::builder()
            .https_only(true)
            .no_proxy()
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
            access,
            options,
        })
    }
    // Intentionally no arbitrary method, mutation, redirect or response-link API.
    pub fn get(&self, url: Url) -> Result<Response, ApiIssue> {
        if check(self.options).is_err() {
            return Err(ApiIssue::new("cancelled", None));
        }
        let mut request = self.client.get(url).header("Accept", "application/json");
        if let Some(access) = self.access {
            let text = Zeroizing::new(format!("Bearer {}", access.token.expose_secret()));
            let mut value =
                HeaderValue::from_str(&text).map_err(|_| ApiIssue::new("invalid_token", None))?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        let mut response = request.send().map_err(|e| {
            ApiIssue::new(
                if e.is_timeout() {
                    "timeout"
                } else {
                    "https_request_failed"
                },
                None,
            )
        })?;
        let status = response.status().as_u16();
        if status != 200 {
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
            issue.retry_after_seconds = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(|n| n.min(86400));
            return Err(issue);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        let mut buffer = [0; 8192];
        loop {
            if check(self.options).is_err() {
                return Err(ApiIssue::new("cancelled", None));
            }
            let count = response
                .read(&mut buffer)
                .map_err(|_| ApiIssue::new("api_body_unavailable", Some(status)))?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > 2 * 1024 * 1024 {
                return Err(ApiIssue::new("api_body_exceeds_limit", Some(status)));
            }
        }
        // Never deserialize or display provider error bodies/headers. Successful
        // JSON is also bounded and only normalized allowlisted fields survive.
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| ApiIssue::new("invalid_json", Some(status)))?;
        let next = response
            .headers()
            .get("x-next-page")
            .is_some_and(|v| !v.as_bytes().is_empty() && v != "0")
            || response
                .headers()
                .get("link")
                .is_some_and(|v| v.to_str().is_ok_and(|s| s.contains("rel=\"next\"")));
        Ok(Response { value, next })
    }
}
