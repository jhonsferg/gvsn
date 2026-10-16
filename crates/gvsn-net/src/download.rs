//! Single-stream download engine with resume and retry support.
//!
//! [`fetch`] is the single entry point for all downloads in `gvsn`. Every
//! download uses one HTTP connection end to end - no chunking, no
//! multi-bar coordination, nothing to race against. A large read buffer
//! keeps per-syscall overhead low so the stream can use as much of the
//! link's actual throughput as the OS scheduler allows.
//!
//! There is no separate HEAD probe: go.dev's download links 302-redirect to
//! a CDN host, and a HEAD request pays that redirect round-trip just to
//! learn the file size. The GET response carries the same size headers, so
//! [`try_fetch`] reads them off the real transfer instead of a throwaway
//! request.
//!
//! Resume is transparent: if a `.part` file already exists from a previous
//! attempt, the download picks up from the last written byte using a
//! `Range: bytes=N-` request. If the server ignores the Range header (no
//! `Accept-Ranges` support) the transfer restarts from scratch.
//!
//! On any network error the download retries up to [`HttpClient::retries`]
//! times using exponential back-off (1 s, 2 s, 4 s, …).
//!
//! This module does not verify integrity: the caller checks the SHA-256 of
//! the completed file (see `gvsn-core`) before extracting it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use crate::http::HttpClient;

/// Size of the buffer used to read the response body. Large enough to keep
/// per-syscall overhead low and let the single stream use as much of the
/// link's throughput as possible.
const READ_BUF_SIZE: usize = 1024 * 1024;

/// Downloads `url` to `dest` over a single HTTP stream.
///
/// Reads the retry limit from the `HttpClient`.
///
/// # Errors
///
/// Returns an error if the server is unreachable, all retries are
/// exhausted, or `dest` cannot be written to.
pub fn fetch(client: &HttpClient, url: &str, dest: &Path) -> Result<()> {
    let retries = client.retries();
    fetch_single(client, url, dest, retries)
}

/// Single-connection download with retry and transparent resume.
fn fetch_single(client: &HttpClient, url: &str, dest: &Path, retries: u8) -> Result<()> {
    let part = part_path(dest);
    let mut attempt = 0u8;

    loop {
        let existing = part.metadata().map(|m| m.len()).unwrap_or(0);

        match try_fetch(client, url, &part, existing) {
            Ok(()) => {
                std::fs::rename(&part, dest)
                    .with_context(|| format!("Cannot finalise {}", dest.display()))?;
                return Ok(());
            }
            Err(e) => {
                if attempt >= retries {
                    let _ = std::fs::remove_file(&part);
                    return Err(e).context(format!("Download failed after {retries} retries"));
                }
                attempt += 1;
                eprintln!(
                    "  {} Network error, retrying ({}/{retries})...",
                    "!".yellow(),
                    attempt
                );
                thread::sleep(Duration::from_secs(backoff(attempt)));
            }
        }
    }
}

/// Performs a single GET request and streams the body to `part` with a
/// large read buffer ([`READ_BUF_SIZE`]) to keep per-syscall overhead low.
///
/// Sizes the progress bar from the GET response's own headers - no separate
/// HEAD request, so there is only ever one redirect round-trip per attempt
/// instead of two.
fn try_fetch(client: &HttpClient, url: &str, part: &Path, offset: u64) -> Result<()> {
    crate::http::log_request(client, "GET", url);

    let mut req = client
        .agent()
        .get(url)
        // Prevent transparent gzip so the raw binary body is never decoded.
        .header("Accept-Encoding", "identity");
    if offset > 0 {
        req = req.header("Range", &format!("bytes={offset}-"));
    }

    let mut response = req
        .call()
        .with_context(|| format!("Failed to connect to {url}"))?;

    crate::http::log_response(
        client,
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    // 206 means the server honoured our Range header and we can append.
    // Any other status (200 for a server that ignores Range) means restart.
    let resuming = offset > 0 && response.status().as_u16() == 206;

    // go.dev's CDN always reports the full file size via
    // x-identity-content-length, even on a 206 response, so it doesn't need
    // adjusting for the current offset. Plain content-length is only the
    // size of the remaining bytes on a 206, so add offset back to get the
    // total for the progress bar.
    let headers = response.headers();
    let total_length = headers
        .get("x-identity-content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            let len = headers
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if resuming {
                offset + len
            } else {
                len
            }
        });

    let pb = progress_bar(total_length, if resuming { offset } else { 0 });
    pb.enable_steady_tick(Duration::from_millis(120));

    let mut file: File = if resuming {
        OpenOptions::new()
            .append(true)
            .open(part)
            .with_context(|| format!("Cannot open {} for appending", part.display()))?
    } else {
        File::create(part).with_context(|| format!("Cannot create {}", part.display()))?
    };

    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; READ_BUF_SIZE];
    let result = (|| -> Result<()> {
        loop {
            let n = reader.read(&mut buf).context("Download interrupted")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).context("Write failed")?;
            pb.inc(n as u64);
        }
        Ok(())
    })();

    pb.finish_and_clear();
    result
}

/// Returns the path used for the in-progress download: `"{dest}.part"`.
fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.to_string_lossy()))
}

/// Builds the progress bar for the download.
///
/// When `content_length > 0` returns a determinate bar pre-positioned at
/// `existing` bytes (to reflect a resumed transfer). Otherwise returns a
/// spinner.
fn progress_bar(content_length: u64, existing: u64) -> ProgressBar {
    if content_length > 0 {
        let bar = ProgressBar::new(content_length);
        bar.set_position(existing);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "  [{bar:40.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}  eta {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
        );
        bar
    } else {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.cyan}  {bytes}  {bytes_per_sec}")
                .unwrap(),
        );
        bar
    }
}

/// Exponential back-off in seconds for retry attempt `n` (1-based): 1, 2, 4, 8 …
fn backoff(n: u8) -> u64 {
    2u64.pow(n as u32 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn part_path_appends_suffix() {
        let dest = Path::new("/tmp/go1.22.4.linux-amd64.tar.gz");
        assert_eq!(
            part_path(dest),
            PathBuf::from("/tmp/go1.22.4.linux-amd64.tar.gz.part")
        );
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        assert_eq!(backoff(1), 1);
        assert_eq!(backoff(2), 2);
        assert_eq!(backoff(3), 4);
        assert_eq!(backoff(4), 8);
    }

    #[test]
    fn progress_bar_is_determinate_when_length_known() {
        let bar = progress_bar(1000, 250);
        assert_eq!(bar.length(), Some(1000));
        assert_eq!(bar.position(), 250);
    }

    #[test]
    fn progress_bar_is_spinner_when_length_unknown() {
        let bar = progress_bar(0, 0);
        assert_eq!(bar.length(), None);
    }

    #[test]
    fn part_path_handles_paths_without_extension() {
        let dest = Path::new("go-archive");
        assert_eq!(part_path(dest), PathBuf::from("go-archive.part"));
    }

    /// End-to-end regression test for resume: a `.part` file with some bytes
    /// already on disk must produce a `Range` request for the remaining
    /// bytes, and the response must be appended (not overwritten) so the
    /// final file matches the full original content. Previously only the
    /// pure `backoff()` helper was tested - this exercises the actual
    /// resume path against a real (minimal) HTTP server.
    #[test]
    fn fetch_resumes_from_existing_partial_file_via_range_request() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;

        let full_content = b"Hello, resumable download world!".to_vec();
        let already_written = 5usize; // "Hello"
        let remaining = full_content[already_written..].to_vec();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let remaining_for_server = remaining.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());

            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();

            let mut saw_range_header = false;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if line.to_ascii_lowercase().starts_with("range:") && line.contains("bytes=5-") {
                    saw_range_header = true;
                }
            }
            assert!(
                saw_range_header,
                "expected a 'Range: bytes=5-' header on the resumed request"
            );

            let mut stream = stream;
            let response = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                remaining_for_server.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&remaining_for_server).unwrap();
            stream.flush().unwrap();
        });

        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        std::fs::write(part_path(&dest), &full_content[..already_written]).unwrap();

        let client = HttpClient::new(false, 0).unwrap();
        let url = format!("http://{addr}/file.bin");
        fetch(&client, &url, &dest).unwrap();

        server.join().unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), full_content);
        assert!(!part_path(&dest).exists());
    }
}
