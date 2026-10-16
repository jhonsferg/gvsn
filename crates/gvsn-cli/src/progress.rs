//! Download progress indicator on stderr (indicatif).
//!
//! It is only drawn when stderr is a terminal and `NO_COLOR` is not set.
//! Otherwise it is a no-op, so piped output and CI logs stay clean.
//!
//! [`ByteBar`] implements `gvsn_net::download::ProgressSink`.

use std::borrow::Cow;
use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;
use gvsn_net::download::{self, ProgressSink};
use gvsn_net::http::HttpClient;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Returns `true` when the progress bar should be drawn: stderr is a
/// terminal and the user has not opted out of decorated output with
/// `NO_COLOR`.
pub fn enabled() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Downloads `url` to `dest` while drawing a [`ByteBar`] labelled `label`.
///
/// The bar is cleared once the download ends, successfully or not, so the
/// next line of output starts on a clean line.
///
/// # Errors
///
/// Same as [`download::fetch`].
pub fn fetch_with_bar(client: &HttpClient, url: &str, dest: &Path, label: &str) -> Result<()> {
    let bar = ByteBar::new(enabled(), format!("  {label}"));
    let result = download::fetch_with_progress(client, url, dest, &bar);
    bar.finish();
    result
}

/// Byte bar for a download. If `enabled` is false it draws nothing.
#[derive(Debug)]
pub struct ByteBar {
    bar: ProgressBar,
}

impl ByteBar {
    /// Creates the bar with `message` if `enabled`; otherwise it is a no-op.
    pub fn new(enabled: bool, message: impl Into<Cow<'static, str>>) -> Self {
        if !enabled {
            return Self {
                bar: ProgressBar::with_draw_target(None, ProgressDrawTarget::hidden()),
            };
        }
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        let style = ProgressStyle::with_template(
            "{msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ETA {eta}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar());
        bar.set_style(style.progress_chars("=> "));
        bar.set_message(message);
        Self { bar }
    }

    /// Clears the bar from the screen.
    pub fn finish(self) {
        self.bar.finish_and_clear();
    }
}

impl ProgressSink for ByteBar {
    fn on_start(&self, total: u64) {
        // A total of 0 means "unknown": the bar only shows bytes.
        self.bar.set_length(total);
        self.bar.set_position(0);
    }

    fn on_bytes(&self, n: u64) {
        self.bar.inc(n);
    }
}

#[cfg(test)]
mod tests {
    use super::ByteBar;
    use gvsn_net::download::ProgressSink;

    #[test]
    fn disabled_bar_draws_nothing_and_accepts_updates() {
        let bar = ByteBar::new(false, "  go1.22.4");
        assert!(bar.bar.is_hidden());
        bar.on_start(1024);
        bar.on_bytes(512);
        assert_eq!(bar.bar.position(), 512);
        bar.finish();
    }

    #[test]
    fn on_start_resets_position_for_a_retry() {
        let bar = ByteBar::new(false, "  go1.22.4");
        bar.on_start(1024);
        bar.on_bytes(700);
        bar.on_start(1024);
        assert_eq!(bar.bar.position(), 0);
        assert_eq!(bar.bar.length(), Some(1024));
        bar.finish();
    }
}
