//! Shared HTTP client, verbose-logging helpers, and download options.
//!
//! All outbound HTTP requests in `gvm` go through [`HttpClient`].
//! Using a single client ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.
//!
//! When the `--verbose` / `-v` flag is passed, [`log_request`] and
//! [`log_response`] print HTTP negotiation details to stderr so the user
//! can diagnose connectivity, redirects, and server behaviour.

use std::time::Duration;

use anyhow::Result;
use colored::Colorize;

/// HTTP client configuration shared across all requests.
#[derive(Debug, Clone)]
pub struct HttpClient {
    agent: ureq::Agent,
    verbose: bool,
    retries: u8,
}

impl HttpClient {
    /// Creates a new `HttpClient` with the given verbosity and retry settings.
    pub fn new(verbose: bool, retries: u8) -> Result<Self> {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            // Bounds the whole request (connect + write + read), so a
            // connection that stalls after the handshake (flaky Wi-Fi, a CDN
            // that stops sending bytes) fails and hits the retry/back-off
            // logic instead of hanging forever. Kept generous (5 min) because
            // downloads share this client: `archive::download::fetch` resumes
            // from the last written byte via `Range` on retry, so a merely
            // slow (but still progressing) transfer just continues across a
            // couple of attempts instead of failing outright.
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(format!("gvm/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Ok(Self {
            agent,
            verbose,
            retries,
        })
    }

    /// Returns the underlying `ureq` agent.
    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    /// Returns `true` when verbose mode is active.
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Returns the configured retry limit.
    pub fn retries(&self) -> u8 {
        self.retries
    }
}

/// Logs an outgoing HTTP request line to stderr when verbose mode is active.
pub fn log_request(client: &HttpClient, method: &str, url: &str) {
    if client.is_verbose() {
        eprintln!("  {} > {} {}", "[v]".dimmed(), method.bold(), url);
    }
}

/// Logs an incoming HTTP response status and all headers to stderr when
/// verbose mode is active.
///
/// `headers` is the `http::HeaderMap` returned by `ureq::Response::headers()`.
pub fn log_response(
    client: &HttpClient,
    status: u16,
    reason: &str,
    headers: &ureq::http::HeaderMap,
) {
    if client.is_verbose() {
        eprintln!(
            "  {} < {} {}",
            "[v]".dimmed(),
            status.to_string().bold(),
            reason
        );
        for (name, value) in headers {
            eprintln!(
                "  {} < {}: {}",
                "[v]".dimmed(),
                name,
                value.to_str().unwrap_or("<binary>")
            );
        }
        eprintln!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_client_stores_verbose_and_retries() {
        let client = HttpClient::new(true, 5).unwrap();
        assert!(client.is_verbose());
        assert_eq!(client.retries(), 5);

        let client = HttpClient::new(false, 0).unwrap();
        assert!(!client.is_verbose());
        assert_eq!(client.retries(), 0);
    }

    #[test]
    fn agent_is_accessible() {
        let client = HttpClient::new(false, 3).unwrap();
        // Just confirm the accessor returns a usable agent reference; no
        // network call is made.
        let _agent: &ureq::Agent = client.agent();
    }

    #[test]
    fn log_request_and_response_are_silent_when_not_verbose() {
        // These should not panic and should simply do nothing when verbose
        // mode is off - there is no way to assert on stderr output directly,
        // but exercising the non-verbose branch still gives us coverage of
        // the early-return path.
        let client = HttpClient::new(false, 3).unwrap();
        log_request(&client, "GET", "https://example.invalid/");

        let headers = ureq::http::HeaderMap::new();
        log_response(&client, 200, "OK", &headers);
    }

    #[test]
    fn log_request_and_response_do_not_panic_when_verbose() {
        let client = HttpClient::new(true, 3).unwrap();
        log_request(&client, "GET", "https://example.invalid/");

        let mut headers = ureq::http::HeaderMap::new();
        headers.insert(
            ureq::http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        log_response(&client, 404, "Not Found", &headers);
    }
}
