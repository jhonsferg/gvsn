//! Local-only performance benchmarks for gvm's hottest pure-computation
//! paths, identified via call-graph fan-in/loop analysis rather than guessed:
//!
//! - [`gvm_rs::profile::ShellProfile::parse`] - re-parses the user's shell
//!   profile on every `gvm setup`/`gvm doctor` run.
//! - [`gvm_rs::version::GoVersion::parse`] - the single highest fan-in
//!   function in the whole call graph (20 callers): every version comparison
//!   and lookup goes through it.
//! - [`gvm_rs::gvm_release::parse_semver`] - parses gvm's own release tag on
//!   every `gvm upgrade` and the background update check.
//! - [`gvm_rs::archive::download::verify_sha256`] - hashes the full archive
//!   after every download; the one benchmark here with a real I/O component.
//!
//! Run locally with `cargo bench`. This target is intentionally never
//! invoked from `.github/workflows/*` - see `benches/README.md`.

use std::hint::black_box;
use std::io::Write;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gvm_rs::archive::download::verify_sha256;
use gvm_rs::gvm_release::parse_semver;
use gvm_rs::profile::ShellProfile;
use gvm_rs::version::GoVersion;

/// Builds a synthetic shell profile with `blocks` gvm-managed blocks
/// interleaved with unrelated user lines, roughly matching what a
/// long-lived `.bashrc`/`.zshrc` looks like after years of accumulated
/// aliases and exports.
fn synthetic_profile(blocks: usize) -> String {
    let mut s = String::new();
    for i in 0..blocks {
        s.push_str(&format!("export MY_VAR_{i}=value_{i}\n"));
        s.push_str("alias ll='ls -la'\n\n");
    }
    s.push_str("# gvm init\n");
    s.push_str("eval \"$(gvm env --shell bash)\"\n\n");
    s.push_str("# gvm wrapper\n");
    s.push_str("gvm() { command gvm \"$@\"; }\n\n");
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

criterion_group!(
    hot_paths,
    bench_profile_parse,
    bench_go_version_parse,
    bench_parse_semver,
    bench_verify_sha256
);
criterion_main!(hot_paths);
