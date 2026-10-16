# Contributing to gvsn

gvsn is an actively maintained project and we intend to keep it that way - continuously
receiving bug fixes, improvements, and new features. Every contribution matters, whether
it is a typo fix, a new idea, a bug report, or a full feature implementation.
**Everyone is welcome to participate and collaborate.** There are no gatekeepers here.

---

## Ways to contribute

- **Report a bug** - open an issue using the bug report template.
- **Suggest a feature** - open an issue using the feature request template. All ideas are
  read and considered.
- **Improve documentation** - fix a typo, clarify an example, translate.
- **Write code** - fix a bug, implement a feature, improve performance, add tests.
- **Review pull requests** - feedback from any community member is valued.

---

## Before you start coding

- Check [open issues](https://github.com/jhonsferg/gvsn/issues) and
  [pull requests](https://github.com/jhonsferg/gvsn/pulls) to avoid duplicate work.
- For large changes, open an issue first to discuss the approach. This saves everyone
  time and ensures the work aligns with the project direction.

---

## Development setup

```sh
git clone https://github.com/jhonsferg/gvsn.git
cd gvsn
cargo build
cargo test
```

**Requirements:** Rust stable (see `rust-toolchain.toml`). No other system dependencies.

### Workspace layout

gvsn is a Cargo workspace split into crates under `crates/`. Dependencies only
point downwards in this list:

| Crate           | Path                    | Responsibility                                                         |
| --------------- | ----------------------- | ---------------------------------------------------------------------- |
| `gvsn`          | `crates/gvsn-cli`       | The binary: argument parsing, dispatch, commands and terminal output.  |
| `gvsn-core`     | `crates/gvsn-core`      | Domain: Go versions, toolchain store, release index, extraction, locks. |
| `gvsn-shell`    | `crates/gvsn-shell`     | Shell integration scripts and profile management. No internal deps.    |
| `gvsn-net`      | `crates/gvsn-net`       | HTTP client and resumable downloads. Knows nothing about Go.           |
| `gvsn-platform` | `crates/gvsn-platform`  | OS details: the `current` link and the Windows user PATH registry.     |

The binary crate keeps the package name `gvsn` so `cargo install gvsn` keeps
working; every crate is published to crates.io with the same version.

---

## Commit style - Conventional Commits

This project uses [Conventional Commits](https://www.conventionalcommits.org/) to drive
automatic versioning. Use the correct prefix so the CI can compute the next version:

| Prefix                          | Semver effect | Example                                 |
| ------------------------------- | ------------- | --------------------------------------- |
| `feat:`                         | minor bump    | `feat: add gvsn pin command`             |
| `fix:`                          | patch bump    | `fix: correct PATH on zsh logout`       |
| `perf:` / `refactor:`           | patch bump    | `perf: cache remote index`              |
| `docs:` / `chore:` / `ci:`      | no release    | `docs: update fish setup example`       |
| `feat!:` or `BREAKING CHANGE:`  | major bump    | `feat!: rename --force to --reinstall`  |

---

## Code standards

```sh
cargo fmt --all                              # format
cargo clippy --workspace --all-targets -- -D warnings   # lint (zero warnings required)
cargo test --workspace                                  # all tests must pass
```

CI enforces all three. A pull request that breaks any of them will not be merged.

---

## Adding a new command

1. Create `crates/gvsn-cli/src/commands/<name>.rs` with a `pub fn run(config: &Config, ...) -> Result<()>`.
2. Add `pub mod <name>;` to `crates/gvsn-cli/src/commands/mod.rs`.
3. Add a variant to `Command` in `crates/gvsn-cli/src/cli.rs`.
4. In `crates/gvsn-cli/src/dispatch.rs`: add a match arm for the new variant in `command_to_name_and_args`,
   implement the `Command` trait for it (a `<Name>Cmd` struct with `execute`), and register
   it with `registry.register(Box::new(<Name>Cmd))` in `build_registry`.
5. Write unit tests in the same file under `#[cfg(test)]`.

Logic that is not about the terminal (version handling, store layout, network,
shell scripts) belongs in the library crate that owns it, not in the command.

---

## Shell integration changes

Any change to a shell hook or wrapper must be tested against all four shells:
**Bash, Zsh, Fish, and PowerShell.** See `crates/gvsn-shell/src/shell/` for implementations.
The test helpers in `crates/gvsn-shell/src/shell/mod.rs` cover `inject_profile` logic and can be
extended for new behaviour.

---

## Benchmarks

Local-only performance benchmarks ([criterion](https://github.com/bheisler/criterion.rs))
cover the pure-computation hot paths. They live in `.benches/` inside the crate
that owns the code:

```sh
cargo bench -p gvsn-core    # GoVersion::parse, parse_semver, verify_sha256, .tool-versions
cargo bench -p gvsn-shell   # ShellProfile::parse
```

HTML reports land in `target/criterion/report/index.html`.

These benchmarks are **not** wired into any workflow under `.github/workflows/`,
and must stay that way. `cargo bench` runs each benchmark for several seconds per
function to get a statistically stable measurement - that's the right trade-off on
a developer machine chasing a specific regression, but it's wasted GitHub Actions
compute on every push when all CI needs is `cargo test`. If you add a benchmark,
do not add a step that runs it in CI. The functions covered were picked from real
call-graph signals (fan-in, loop depth) rather than guessed - see the header of
each `hot_paths.rs` for the reasoning behind each one.

---

## Pull request checklist

- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings
- [ ] `cargo test --workspace` passes (all tests green)
- [ ] New behaviour has unit tests
- [ ] Commit messages follow Conventional Commits

---

## License

By contributing you agree that your contributions will be licensed under the
[MIT License](LICENSE). This keeps the project free and open for everyone.

---

## Malicious use and unofficial copies

This project's openness (MIT license, public source, no gatekeeping on contributions) is meant to
benefit developers, not to provide cover for abuse. Forking and modifying `gvsn` is always fine.
What is not acceptable, and will be treated as a security incident rather than a licensing dispute,
is republishing this project (or a modified copy of it) in a way designed to impersonate it while
distributing malware, credential stealers, or other harmful payloads to people who believe they are
using the original tool.

See [SECURITY.md](SECURITY.md#malicious-forks--clones) for what we consider a malicious clone, how
to report one, and what action we take (independent verification, followed by an abuse report to
GitHub requesting takedown and account suspension, with public disclosure once resolved).
