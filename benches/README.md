# Benchmarks

Local-only performance benchmarks (`cargo bench`, [criterion](https://github.com/bheisler/criterion.rs)) for gvsn's pure-computation hot paths.

## Running

```sh
cargo bench
```

HTML reports land in `target/criterion/report/index.html`.

## Scope: local only, never CI

These benchmarks are **not** wired into any workflow under `.github/workflows/`, and must stay that way. `cargo bench` runs each benchmark for several seconds per function to get a statistically stable measurement - that's the right trade-off on a developer machine chasing a specific regression, but it's wasted GitHub Actions compute on every push when all `.github/workflows/ci.yml` needs is `cargo test`. If you add a benchmark, do not add a step that runs it in CI.

## What's covered

The functions here were picked from real call-graph signals (fan-in, loop depth) rather than guessed - see `benches/hot_paths.rs` for the reasoning behind each one.
