# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
| ------- | --------- |
| 1.x     | ✅        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report security issues privately via GitHub's built-in mechanism:

1. Go to the [Security tab](https://github.com/jhonsferg/gvsn/security/advisories/new) of this repository.
2. Click **"Report a vulnerability"**.
3. Fill in the details: affected versions, reproduction steps, and potential impact.

You will receive an acknowledgement within **72 hours** and a resolution timeline
within **7 days** for critical issues.

## Scope

- Arbitrary code execution via crafted `.go-version` files or default-packages entries
- Path traversal in archive extraction
- Binary substitution during `gvsn install` or `gvsn upgrade` (SHA-256 bypass)
- Credential or secret leakage in logs or error messages

## Out of Scope

- Issues in Go toolchains themselves (report to the [Go team](https://go.dev/security))
- Social engineering or phishing
- Vulnerabilities in systems that `gvsn` does not control (e.g. go.dev infrastructure)

## Antivirus / Windows Defender false positives

`gvsn` binaries are not code-signed yet, which makes them more likely to trigger antivirus and
SmartScreen warnings than signed software - this is a **false positive**, not an indication that a
release is compromised. A few things about `gvsn` specifically make this more likely:

- **Low file reputation**: every release produces a binary with a new hash, so cloud-based
  reputation systems (Windows Defender SmartScreen, and similarly in third-party AV) see it as
  "never seen before" on every version bump.
- **Self-replacing update**: `gvsn upgrade` downloads a new binary and replaces the currently
  running executable in place. This is the same category of behavior some heuristic engines
  associate with malware droppers/updaters, even though it's implemented via the same
  atomic-rename pattern used by mainstream tools like `rustup`, `scoop`, and `nvm-windows` (which
  hit this same false-positive class from time to time too).
- **`irm | iex` / `curl | sh` installers**: the one-line install pattern is standard across the
  ecosystem (Rust, Homebrew, nvm, ...) but is also exactly what "download and execute" heuristics
  watch for in PowerShell.

### If your antivirus or Windows Defender flags `gvsn`

1. **Verify the download is genuine** before doing anything else: compare its SHA-256 against the
   `checksums.txt` published on the [same release](https://github.com/jhonsferg/gvsn/releases).
   `install.ps1`/`install.sh` and `uninstall.ps1`/`uninstall.sh` already do this automatically, and
   releases with a `VIRUSTOTAL_API_KEY` configured also link a multi-engine VirusTotal scan for
   every archive directly in the release notes.
2. Add an exclusion for the `gvsn` binary/install directory in your AV product, or restore it from
   quarantine, once you've verified the checksum.
3. Please also report the false positive to your AV vendor (most have a submission form, e.g.
   [Microsoft's for Defender/SmartScreen](https://www.microsoft.com/en-us/wdsi/filesubmission)) -
   this helps them correct it for every other `gvsn` user, not just you.
4. If you'd rather not deal with any of this, [open an issue](https://github.com/jhonsferg/gvsn/issues)
   with your AV product's name and the exact detection name/ID it reported - it helps us track
   which engines flag `gvsn` and prioritize getting it code-signed and submitted for allow-listing.

We will never ask you to disable OS protections to run `gvsn` - see
[Malicious forks / clones](#malicious-forks--clones) below for how to tell a genuine release from a
compromised one.

### For maintainers: enabling code signing (pending - applied to OSSign)

Signing is the actual fix for the false positives above, not just a workaround - it removes the
"unknown publisher" warning entirely and lets reputation accumulate on the publisher identity
instead of resetting on every release's new file hash. Free options for OSS projects were surveyed
in August 2026:

- **[SignPath Foundation](https://signpath.org/)**: no longer viable - what used to be a
  straightforward free program now funnels through SignPath's commercial product, which only
  offers a paid trial.
- **[Certum](https://www.certum.eu/en/code-signing-certificates/)**: discontinued its free
  open-source code-signing program back in 2016; only paid certificates remain.
- **[OSSign](https://ossign.org/)**: still genuinely free for qualifying OSS projects (OSI
  license, public source, actively maintained - `gvsn` meets all of it), and integrates via a
  [GitHub Action](https://github.com/OSSign/actions) that dispatches a signing request and waits
  for the signed artifact. **Chosen path**, but as of 2026-08-22 OSSign has applications
  **suspended due to a backlog** - there's no live form to submit right now. Check
  [ossign.org](https://ossign.org/) periodically, or email `contact@ossign.org`, to find out when
  applications reopen.

Once approved, OSSign provisions per-project credentials (a username + token) and a dedicated
signing repository - the exact wiring into
[`release.yml`](.github/workflows/release.yml) depends on details only available at that point
(their action needs the job to poll or use a GitHub Environment wait-timer while the signing
request is manually approved on their end), so it hasn't been pre-wired the way the VirusTotal step
below was - doing so now would mean guessing at values, exactly the kind of thing that broke this
workflow file once already (see the fix in `fix(ci): route secrets through env before use in if:
conditionals`). Wire it in as a follow-up once OSSign approval and credentials arrive.

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
2. Report it to us via a [GitHub Security Advisory](https://github.com/jhonsferg/gvsn/security/advisories/new)
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

### Known cases

> ℹ️ The case below predates this project's rename from `gvm-rs` to `gvsn` (August 2026) and is
> preserved exactly as reported at the time - the clone impersonated the project under its former
> name. The project's original repository, `jhonsferg/gvm-rs`, has since been deleted, so the
> issue link below no longer resolves; the full technical record survives in this project's own
> incident reports, linked below.

- **[`Foremost-headsail607/gvm-rs`](https://github.com/Foremost-headsail607/gvm-rs)** - flagged by
  this repository's automated clone-watch (see issue #53, `jhonsferg/gvm-rs#53` - no longer
  reachable, see note above). It reuses this project's name and
  description, but does **not** distribute binaries through GitHub Releases like the real pipeline
  does - its README links directly to an archive committed inside `src/`
  (`gvm-rs-xiphisternal.zip`), bypassing this repository's build, checksum, and
  [VirusTotal-scan](#antivirus--windows-defender-false-positives) process entirely. This was
  reported through [GitHub's abuse form](https://github.com/contact/report-abuse) on 2026-07-20
  (Ticket ID `4587510`); the reply received the next day did not reference the report, the
  repository, or the evidence submitted - it addressed an unrelated Copilot-access restriction on
  the same ticket. The clone was still active and receiving pushes as of
  2026-08-22. Full technical writeup, including commit hashes, the recovered malware-loader
  chain, and its correlation with a real antivirus detection:
  [`docs/security/incidents/2026-06-malicious-clone-foremost-headsail607.md`](docs/security/incidents/2026-06-malicious-clone-foremost-headsail607.md).
  For a plain-language walkthrough of the whole attack (what the malware does, its techniques,
  IOCs, and how to protect yourself): [`docs/security/anatomy-of-a-malicious-clone.md`](docs/security/anatomy-of-a-malicious-clone.md).
- If you installed `gvsn` from anywhere other than the
  [official releases](https://github.com/jhonsferg/gvsn/releases) or the install commands in the
  [README](README.md#-installation), do not run it - verify what you downloaded, or reinstall from
  a genuine source.

### Disclaimer

This project, its releases, and its published checksums are only what comes out of
`jhonsferg/gvsn`'s own [`release.yml`](.github/workflows/release.yml) pipeline. We are not
responsible for, and have no control over, binaries distributed by clones, forks, or third parties
impersonating this project - including any that reuse this project's name, README, or commit
history. If in doubt, always verify the SHA-256 checksum of anything named `gvsn` against the
`checksums.txt` published on the matching [release](https://github.com/jhonsferg/gvsn/releases).
