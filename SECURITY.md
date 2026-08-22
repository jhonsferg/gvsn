# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
| ------- | --------- |
| 1.x     | ✅        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report security issues privately via GitHub's built-in mechanism:

1. Go to the [Security tab](https://github.com/jhonsferg/gvm-rs/security/advisories/new) of this repository.
2. Click **"Report a vulnerability"**.
3. Fill in the details: affected versions, reproduction steps, and potential impact.

You will receive an acknowledgement within **72 hours** and a resolution timeline
within **7 days** for critical issues.

## Scope

- Arbitrary code execution via crafted `.go-version` files or default-packages entries
- Path traversal in archive extraction
- Binary substitution during `gvm install` or `gvm upgrade` (SHA-256 bypass)
- Credential or secret leakage in logs or error messages

## Out of Scope

- Issues in Go toolchains themselves (report to the [Go team](https://go.dev/security))
- Social engineering or phishing
- Vulnerabilities in systems that `gvm` does not control (e.g. go.dev infrastructure)

## Antivirus / Windows Defender false positives

`gvm` binaries are not code-signed yet, which makes them more likely to trigger antivirus and
SmartScreen warnings than signed software - this is a **false positive**, not an indication that a
release is compromised. A few things about `gvm` specifically make this more likely:

- **Low file reputation**: every release produces a binary with a new hash, so cloud-based
  reputation systems (Windows Defender SmartScreen, and similarly in third-party AV) see it as
  "never seen before" on every version bump.
- **Self-replacing update**: `gvm upgrade` downloads a new binary and replaces the currently
  running executable in place. This is the same category of behavior some heuristic engines
  associate with malware droppers/updaters, even though it's implemented via the same
  atomic-rename pattern used by mainstream tools like `rustup`, `scoop`, and `nvm-windows` (which
  hit this same false-positive class from time to time too).
- **`irm | iex` / `curl | sh` installers**: the one-line install pattern is standard across the
  ecosystem (Rust, Homebrew, nvm, ...) but is also exactly what "download and execute" heuristics
  watch for in PowerShell.

### If your antivirus or Windows Defender flags `gvm`

1. **Verify the download is genuine** before doing anything else: compare its SHA-256 against the
   `checksums.txt` published on the [same release](https://github.com/jhonsferg/gvm-rs/releases).
   `install.ps1`/`install.sh` and `uninstall.ps1`/`uninstall.sh` already do this automatically, and
   releases with a `VIRUSTOTAL_API_KEY` configured also link a multi-engine VirusTotal scan for
   every archive directly in the release notes.
2. Add an exclusion for the `gvm` binary/install directory in your AV product, or restore it from
   quarantine, once you've verified the checksum.
3. Please also report the false positive to your AV vendor (most have a submission form, e.g.
   [Microsoft's for Defender/SmartScreen](https://www.microsoft.com/en-us/wdsi/filesubmission)) -
   this helps them correct it for every other `gvm` user, not just you.
4. If you'd rather not deal with any of this, [open an issue](https://github.com/jhonsferg/gvm-rs/issues)
   with your AV product's name and the exact detection name/ID it reported - it helps us track
   which engines flag `gvm` and prioritize getting it code-signed and submitted for allow-listing.

We will never ask you to disable OS protections to run `gvm` - see
[Malicious forks / clones](#malicious-forks--clones) below for how to tell a genuine release from a
compromised one.

### For maintainers: enabling code signing (not done yet)

Signing is the actual fix for the false positives above, not just a workaround - it removes the
"unknown publisher" warning entirely and lets reputation accumulate on the publisher identity
instead of resetting on every release's new file hash. `gvm-rs` qualifies for
[SignPath Foundation](https://signpath.org/)'s free code signing for open-source projects (OSI
license, public repo, actively maintained, already shipping releases - all already true here), but
enrolling requires a few one-time manual steps outside this repository that only the project owner
can do:

1. Apply at [signpath.io/solutions/open-source-community](https://signpath.io/solutions/open-source-community)
   (review and approval happen over email - not instant).
2. Once approved, install the [SignPath GitHub App](https://github.com/apps/signpath) on this
   repository, and in the SignPath dashboard configure a "Trusted Build System" for `GitHub.com`
   with a project + signing policy for `gvm-rs`.
3. From that dashboard, add these to the repository (`gh secret set` / `gh variable set`, or
   Settings → Secrets and variables → Actions):
   - `SIGNPATH_API_TOKEN` (**secret**) - a submitter-scoped API token.
   - `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_SIGNING_POLICY_SLUG`
     (repository **variables** - not secret, just identifiers).

[`release.yml`](.github/workflows/release.yml) already has the signing step wired in behind these
four values - it's a complete no-op today (Windows archives ship unsigned, same as every release so
far) and activates automatically the moment they're all set, no further code changes needed.

Separately: unsigned/low-reputation files can also be reported straight to Microsoft via the
[WDSI submission portal](https://www.microsoft.com/en-us/wdsi/filesubmission), but that portal has
no public API - it's a manual, sign-in-required, per-file web form, so it isn't (and can't be)
wired into the release pipeline. Worth doing by hand after a release if false-positive reports come
in, but signing is the fix that actually scales.

## Malicious forks / clones

This project is MIT-licensed: forking, modifying, and redistributing it is welcome and expected.
What is **not** covered by that license, and will be treated as a security incident against this
project and its users, is any copy of this repository that:

- Presents itself as this project (same name, README, or commit history) without disclosing that
  it is a modified third-party copy.
- Removes or disables the CI/security workflows present in this repository
  (`ci.yml`, `release.yml`, `security.yml`).
- Distributes executables, archives, or scripts that are not produced by this repository's own
  release pipeline - especially files added directly into source control (`.zip`, `.exe`, `.dll`)
  instead of built by [`release.yml`](.github/workflows/release.yml).
- Instructs users to bypass OS security warnings (SmartScreen, Gatekeeper, antivirus) to run a
  downloaded file.
- Obscures the true nature of a change behind a misleading or reused commit message.

**If you discover such a clone:**

1. Do **not** download or run anything from it.
2. Report it to us via a [GitHub Security Advisory](https://github.com/jhonsferg/gvm-rs/security/advisories/new)
   on this repository, or by opening contact through the maintainer's GitHub profile
   ([@jhonsferg](https://github.com/jhonsferg)), including the repository URL and any
   indicators you noticed.
3. We will independently verify the finding (git history divergence, static analysis, and
   third-party AV/threat-intel confirmation where applicable) and, once confirmed, file an abuse
   report with GitHub requesting takedown of the malicious repository and suspension of the
   distributing account.
4. Confirmed cases will be publicly disclosed (with technical evidence) once the takedown is
   resolved, to help the broader community recognize the pattern.

We do not control third-party accounts or repositories and cannot guarantee removal timelines,
but every credible report will be investigated and escalated to GitHub Trust & Safety.
