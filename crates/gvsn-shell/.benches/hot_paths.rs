//! Local-only performance benchmark for gvsn-shell's hottest path:
//!
//! - [`gvsn_shell::profile::ShellProfile::parse`] - re-parses the user's shell
//!   profile on every `gvsn setup`/`gvsn doctor` run.
//!
//! Run locally with `cargo bench -p gvsn-shell`. This target is intentionally
//! never invoked from `.github/workflows/*` - see the "Benchmarks" section in
//! CONTRIBUTING.md.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use gvsn_shell::profile::ShellProfile;

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

criterion_group!(hot_paths, bench_profile_parse);
criterion_main!(hot_paths);
