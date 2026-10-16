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
//! Progress goes to a [`ProgressSink`]; this module draws nothing. The
//! terminal progress bar lives in the CLI.
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

use crate::http::HttpClient;

/// Size of the buffer used to read the response body. Large enough to keep
/// per-syscall overhead low and let the single stream use as much of the
/// link's throughput as possible.
const READ_BUF_SIZE: usize = 1024 * 1024;

/// Receiver of a download's progress. Implement it to show a progress bar;
/// the library draws nothing on its own.
///
/// `on_start` is called at the start of each attempt (also after a retry) and
/// resets the counter. If the download is resumed, `on_bytes` first receives
/// the bytes that were already in the `.part` file.
pub trait ProgressSink {
    /// Expected total size in bytes. `0` if the server does not report it.
    fn on_start(&self, total: u64);

    /// New bytes written to the temporary destination.
    fn on_bytes(&self, n: u64);
}

/// Null receiver: discards progress. It is the default for [`fetch`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn on_start(&self, _total: u64) {}

    fn on_bytes(&self, _n: u64) {}
}

/// Downloads `url` to `dest` over a single HTTP stream, without reporting
/// progress.
///
/// Equivalent to [`fetch_with_progress`] with [`NoProgress`].
///
/// # Errors
///
/// Returns an error if the server is unreachable, all retries are
/// exhausted, or `dest` cannot be written to.
pub fn fetch(client: &HttpClient, url: &str, dest: &Path) -> Result<()> {
    fetch_with_progress(client, url, dest, &NoProgress)
}

/// Downloads `url` to `dest` over a single HTTP stream, reporting progress
/// to `progress`.
///
/// Reads the retry limit from the `HttpClient`.
///
/// # Errors
///
/// Same as [`fetch`].
pub fn fetch_with_progress(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    progress: &dyn ProgressSink,
) -> Result<()> {
    let retries = client.retries();
    fetch_single(client, url, dest, retries, progress)
}

/// Single-connection download with retry and transparent resume.
fn fetch_single(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    retries: u8,
    progress: &dyn ProgressSink,
) -> Result<()> {
    let part = part_path(dest);
    let mut attempt = 0u8;

    loop {
        let existing = part.metadata().map(|m| m.len()).unwrap_or(0);

        match try_fetch(client, url, &part, existing, progress) {
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
/// Reports the total size from the GET response's own headers - no separate
/// HEAD request, so there is only ever one redirect round-trip per attempt
/// instead of two.
fn try_fetch(
    client: &HttpClient,
    url: &str,
    part: &Path,
    offset: u64,
    progress: &dyn ProgressSink,
) -> Result<()> {
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
    // total for the progress report.
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

    progress.on_start(total_length);
    if resuming {
        progress.on_bytes(offset);
    }

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
    loop {
        let n = reader.read(&mut buf).context("Download interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("Write failed")?;
        progress.on_bytes(n as u64);
    }
    Ok(())
}

/// Returns the path used for the in-progress download: `"{dest}.part"`.
fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.to_string_lossy()))
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
        let progress = RecordingProgress::default();
        fetch_with_progress(&client, &url, &dest, &progress).unwrap();

        server.join().unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), full_content);
        assert!(!part_path(&dest).exists());
        // The total covers the whole file, and the bytes already on disk are
        // reported before the new ones, so the bar ends at 100%.
        assert_eq!(progress.total.get(), full_content.len() as u64);
        assert_eq!(progress.bytes.get(), full_content.len() as u64);
    }

    /// Sink that remembers what it was told, for asserting on progress.
    #[derive(Default)]
    struct RecordingProgress {
        total: std::cell::Cell<u64>,
        bytes: std::cell::Cell<u64>,
    }

    impl ProgressSink for RecordingProgress {
        fn on_start(&self, total: u64) {
            self.total.set(total);
            self.bytes.set(0);
        }

        fn on_bytes(&self, n: u64) {
            self.bytes.set(self.bytes.get() + n);
        }
    }
}
