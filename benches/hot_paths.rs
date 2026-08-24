//! Local-only performance benchmarks for gvsn's hottest pure-computation
//! paths, identified via call-graph fan-in/loop analysis rather than guessed:
//!
//! - [`gvsn::profile::ShellProfile::parse`] - re-parses the user's shell
//!   profile on every `gvsn setup`/`gvsn doctor` run.
//! - [`gvsn::version::GoVersion::parse`] - the single highest fan-in
//!   function in the whole call graph (20 callers): every version comparison
//!   and lookup goes through it.
//! - [`gvsn::gvsn_release::parse_semver`] - parses gvsn's own release tag on
//!   every `gvsn upgrade` and the background update check.
//! - [`gvsn::archive::download::verify_sha256`] - hashes the full archive
//!   after every download; the one benchmark here with a real I/O component.
//! - [`gvsn::version::parse_tool_versions_golang_line`] - added alongside
//!   `.go-version` in `toolchain::active_version`'s directory walk-up, so it
//!   now runs on every command in a project that uses an asdf-style
//!   `.tool-versions` file instead of gvsn's own pin file.
//!
//! Run locally with `cargo bench`. This target is intentionally never
//! invoked from `.github/workflows/*` - see `benches/README.md`.

use std::hint::black_box;
use std::io::Write;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gvsn::archive::download::verify_sha256;
use gvsn::gvsn_release::parse_semver;
use gvsn::profile::ShellProfile;
use gvsn::version::{parse_tool_versions_golang_line, GoVersion};

/// Builds a synthetic shell profile with `blocks` gvsn-managed blocks
/// interleaved with unrelated user lines, roughly matching what a
/// long-lived `.bashrc`/`.zshrc` looks like after years of accumulated
/// aliases and exports.
fn synthetic_profile(blocks: usize) -> String {
    let mut s = String::new();
    for i in 0..blocks {
        s.push_str(&format!("export MY_VAR_{i}=value_{i}\n"));
        s.push_str("alias ll='ls -la'\n\n");
    }
    s.push_str("# gvsn init\n");
    s.push_str("eval \"$(gvsn env --shell bash)\"\n\n");
    s.push_str("# gvsn wrapper\n");
    s.push_str("gvsn() { command gvsn \"$@\"; }\n\n");
    for i in 0..blocks {
        s.push_str(&format!(
            "# custom function {i}\nmy_func_{i}() {{ echo {i}; }}\n\n"
        ));
    }
    s
}

fn bench_profile_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("ShellProfile::parse");
    for blocks in [10usize, 100, 1000] {
        let content = synthetic_profile(blocks);
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(blocks),
            &content,
            |b, content| {
                b.iter(|| ShellProfile::parse(black_box(content)));
            },
        );
    }
    group.finish();
}

fn bench_go_version_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("GoVersion::parse");
    for input in ["go1.22.4", "1.22.4", "1.22"] {
        group.bench_with_input(BenchmarkId::from_parameter(input), input, |b, input| {
            b.iter(|| GoVersion::parse(black_box(input)));
        });
    }
    group.finish();
}

fn bench_parse_semver(c: &mut Criterion) {
    c.bench_function("parse_semver", |b| {
        b.iter(|| parse_semver(black_box("1.5.1")));
    });
}

fn bench_verify_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("verify_sha256");
    // 4 KiB (a typical checksum file) up to 64 MiB (in the ballpark of a real
    // Go release archive) so the curve shows whether the read-buffer size in
    // `verify_sha256` keeps throughput flat as files grow.
    for size_mb in [1u64, 16, 64] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        let chunk = vec![0xABu8; 1024 * 1024];
        for _ in 0..size_mb {
            f.write_all(&chunk).unwrap();
        }
        drop(f);

        // The expected digest deliberately does not match, so every
        // iteration still does the full read+hash and only skips the (cheap)
        // final string comparison - this benchmarks the hot loop, not the
        // error path.
        group.throughput(Throughput::Bytes(size_mb * 1024 * 1024));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{size_mb}MiB")),
            &path,
            |b, path| {
                b.iter(|| verify_sha256(black_box(path), black_box("0")));
            },
        );
    }
    group.finish();
}

/// Builds a synthetic `.tool-versions` file with `other_tools` unrelated
/// entries before the `golang` line, matching a real asdf-managed project
/// that pins several runtimes.
fn synthetic_tool_versions(other_tools: usize) -> String {
    let mut s = String::new();
    for i in 0..other_tools {
        s.push_str(&format!("tool-{i} {i}.0.0\n"));
    }
    s.push_str("golang 1.22.4\n");
    s
}

fn bench_parse_tool_versions_golang_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_tool_versions_golang_line");
    for other_tools in [0usize, 10, 50] {
        let content = synthetic_tool_versions(other_tools);
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(other_tools),
            &content,
            |b, content| {
                b.iter(|| parse_tool_versions_golang_line(black_box(content)));
            },
        );
    }
    group.finish();
}

criterion_group!(
    hot_paths,
    bench_profile_parse,
    bench_go_version_parse,
    bench_parse_semver,
    bench_verify_sha256,
    bench_parse_tool_versions_golang_line
);
criterion_main!(hot_paths);
