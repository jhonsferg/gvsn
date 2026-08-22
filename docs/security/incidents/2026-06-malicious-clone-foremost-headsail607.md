# Incident: malware distribution via a cloned repository

- **Status:** Active (confirmed as of 2026-08-22)
- **Malicious repository:** [`Foremost-headsail607/gvm-rs`](https://github.com/Foremost-headsail607/gvm-rs)
- **Detected via:** this repository's automated clone-watch pipeline
  ([issue #53](https://github.com/jhonsferg/gvm-rs/issues/53))
- **Reported to GitHub:** yes, via the [abuse report form](https://github.com/contact/report-abuse);
  the response received did not address the report
- **Related policy:** [SECURITY.md § Malicious forks / clones](../../../SECURITY.md#malicious-forks--clones)

This document exists to give anyone evaluating this incident - GitHub Trust & Safety, security
researchers, or an affected user - the concrete technical evidence behind the summary in
`SECURITY.md`, without requiring them to dig through the malicious repository themselves. Findings
come from two sources: the malicious repository's public commit history via the GitHub API
(read-only metadata and text diffs), and a static analysis of the payload archive it currently
links, performed inside a network-isolated Docker container (REMnux). **The payload was never
executed**, in isolation or otherwise - see [Methodology](#methodology) for exactly how it was
handled.

## Summary

`Foremost-headsail607/gvm-rs` began as a copy of this project's genuine history, then diverged to
add a malware-loading chain to the source tree, later stripped that chain from the visible
history (very likely a rebase/force-push, since removal commits exist with no matching addition
commits), and now distributes a packaged payload directly from source control instead of through
GitHub Releases. As of today, its README actively links to that payload.

## Timeline

| Date (UTC) | Event |
|---|---|
| up to 2026-06-14T02:15:55Z | Commit history is byte-for-byte identical to `jhonsferg/gvm-rs` (last shared commit: `5c43d25445`) |
| 2026-06-16T03:52:03Z | Repository `Foremost-headsail607/gvm-rs` created |
| 2026-06-16 - 2026-06-17 | Several commits reapplying the same `fix(download): enable steady tick on chunk bars for real-time updates` message four times in a row - consistent with a rebase/history rewrite |
| 2026-06-17T14:12:25Z - 14:12:55Z | Four files removed within 30 seconds of each other (see [Technical findings](#technical-findings)); no corresponding "add" commits exist in the visible history for any of them |
| 2026-07-21T08:43:06Z | This repository's clone-watch flags the account (issue #53) |
| (undated, prior to today) | First report filed with GitHub via the abuse form; response received did not address the report |
| 2026-08-22T03:03:47Z | Latest commit: `README.md` updated to point the download button directly at a packaged payload (see below) |

## Technical findings

### A. Commit history reconstruction

#### 1. A malware-loading chain existed in the source tree, then was removed from history

Four files were deleted from the repository within a 30-second window
(commits `cb1dc59da0`, `207e24e026`, `aa7ac2f188`, `77e76f9e46`, all at 2026-06-17T14:12:xxZ).
None of them have a matching "add" commit anywhere in the repository's visible history, which is
only possible if the branch was rewritten (`git rebase` + force-push, or a squash) at some point -
the "add" commits exist as objects but were pruned from the ref history GitHub now serves.

| Path | Role |
|---|---|
| `assets/images/Launcher-masterling.bat` | Launcher script. Deleted content (recovered from the removal commit's diff): `start minify.exe jsm.txt` |
| `src/remote/minify_3.2.exe` | Executable disguised as a "minifier" tool, placed in `src/remote/` - not a plausible location for a build tool |
| `src/commands/jsm-v1.3.txt` | Payload disguised with a `.txt` extension, passed as the argument to the `.exe` above |
| `install/lua51_2.6.dll` | The Lua 5.1 interpreter runtime, placed in `install/` |

Read together, the chain is: a batch file starts an executable, which loads the Lua 5.1 runtime to
interpret a script disguised as a text file. This is a standard Lua-script-loader pattern for
delivering an obfuscated payload while keeping the initially-visible files small and
innocuous-looking.

**This matches a real detection**: on a separate machine, ESET quarantined a file downloaded from
`raw.githubusercontent.com` identified as **`Lua/Agent.CQ troyano`** - a Lua-based trojan family.
The mechanism recovered from this repository's own commit history (a Lua runtime DLL executing a
disguised script) is consistent with that detection's naming and behavior. Section B below confirms
the same chain is present in the currently-distributed archive.

#### 2. The repository does not use GitHub Releases

`GET /repos/Foremost-headsail607/gvm-rs/releases` returns an empty list. Every legitimate `gvm-rs`
binary is built and published exclusively by this repository's own
[`release.yml`](../../../.github/workflows/release.yml) pipeline, with checksums and (when
configured) a VirusTotal scan attached to the GitHub Release. A clone with zero releases has no
such provenance for anything it distributes.

#### 3. Active, current distribution via a direct source-tree link

The malicious repository's most recent commit (`623bc73310`, 2026-08-22T03:03:47Z) changed its
`README.md` download button:

```diff
-[Button: Click here to visit the release page](https://github.com/Foremost-headsail607/gvm-rs/releases)
+[Button: Click here to visit the release page](https://github.com/Foremost-headsail607/gvm-rs/raw/refs/heads/main/src/gvm-rs-xiphisternal.zip)
```

The link no longer even points at the (empty) Releases page - it points directly at an archive
(`gvm-rs-xiphisternal.zip`) committed inside `src/` and served via `raw.githubusercontent.com`,
bypassing GitHub's release/asset infrastructure entirely. This commit landed hours before this
document was written, confirming the repository is actively maintained and distributing a payload
right now, not a dormant leftover from June.

### B. Static analysis of the currently-distributed payload archive

Performed 2026-08-22, inside a REMnux Docker container with `--network none` (no network access
during any analysis or extraction step - see [Methodology](#methodology)).

**Archive**: `gvm-rs-xiphisternal.zip`, fetched from the URL currently linked in the malicious
repository's README (`.../raw/refs/heads/main/src/gvm-rs-xiphisternal.zip`).
476,614 bytes. `sha256: 396cd3da28dfd0ef477c87568329c845f7de2c6c4815c80d4e921b3c49500cda`.

It extracts to the exact same four-file set identified in the June commit history (§A.1), just
without the version-number suffixes:

| File | Size | SHA-256 |
|---|---|---|
| `Launcher.bat` | 24 B | `bcf741a9c9411344965f8e451e364b8923ff1f00e375f61aa5419ee07618856c` |
| `minify.exe` | 31,920 B | `7ad4b911d05a12f91ab27ba3baa351a56653ca099dda7ad87ee2b94f8cd018c9` |
| `lua51.dll` | 601,461 B | `04d3c82782927330d56827ff551697666dbab4b3abf5b86bde492efdd142bc58` |
| `jsm.txt` | 327,648 B | `8f4b2dfd27ef330c184377cd9d01aef278f82d04f2598dafe1f373119043e56c` |

**Findings:**

1. **`Launcher.bat` is a one-liner**: `start minify.exe jsm.txt` - identical to the content
   recovered earlier from the June removal commit's diff.
2. **`minify.exe` is a genuine, unmodified LuaJIT 2.1 interpreter, renamed.** Its imports are only
   standard `KERNEL32.dll`/`msvcrt.dll` runtime functions plus the full `lua51.dll` API
   (`luaL_newstate`, `luaL_loadfile`, `lua_pcall`, ...) - no networking, process-injection, or
   persistence APIs are imported by this binary itself. Embedded strings confirm it: `LuaJIT
   2.1.1774896198 -- Copyright (C) 2005-2026 Mike Pall. https://luajit.org/`. Its PE timestamp
   (`2026-04-01 04:24:07`) matches `lua51.dll`'s exactly, indicating both were taken from the same
   official LuaJIT build and simply renamed - not custom-compiled. This is a
   [LOLBIN-style technique](https://lolbas-project.github.io/): abusing a legitimate, trusted
   scripting engine under an innocuous name to run malicious logic that never appears in the `.exe`
   itself, evading detections keyed on the executable rather than the script it interprets.
3. **`jsm.txt` is the actual payload**: 327,648 bytes of Lua *source* (not compiled bytecode - it
   starts with plaintext `return(function(...)...`), wrapped in heavy control-flow-flattening /
   virtualization-style obfuscation (single/double-letter identifiers, arithmetic-obfuscated
   integer constants like `321916+((-253077-38487)+-30351)` in place of literals, and a large
   dispatch loop keyed on those constants). No obfuscator name/watermark (e.g. Prometheus, Luraph,
   MoonSec) was found in the plaintext. A best-effort search for literal IOCs (`http(s)://` URLs,
   IPv4 literals, Discord webhook paths) across the deobfuscated-looking plaintext found **none** -
   the obfuscation reconstructs any such strings at runtime rather than storing them literally, so
   static string extraction alone cannot recover the actual C2/exfiltration endpoints. Fully
   deobfuscating the custom VM to recover them was judged out of scope for this defensive writeup;
   it would require dedicated Lua-VM reverse engineering (symbolic execution or a Lua sandbox with
   full API instrumentation), not just static inspection.

**What this confirms**: the loader chain guessed from the June commit history is exactly what is
being distributed today, byte-for-byte the same technique (disguised LuaJIT interpreter + obfuscated
Lua payload), just repackaged into a single zip instead of four loose files. Combined with the
`Lua/Agent.CQ` antivirus detection referenced in §A.1, this is consistent with an actively
maintained Lua-based trojan, not a one-off or abandoned artifact.

## What this means for `gvm-rs` users

- The genuine `gvm-rs` has never had, and will never have, its download instructions point at a
  file committed directly into `src/` or any other source path. Downloads only ever come from a
  GitHub Release under `jhonsferg/gvm-rs`, or the `install.sh` / `install.ps1` scripts referenced
  in this repository's [README](../../../README.md#-installation).
- If you or anyone you know installed `gvm-rs` (or "gvm") from `Foremost-headsail607/gvm-rs`, from
  a `raw.githubusercontent.com` link, or from any source other than the ones above: do not run it.
  Treat it as compromised, and see your antivirus vendor's quarantine/removal instructions.

## Methodology

Two separate methods were used, matching the two subsections above:

- **§A** (commit history) comes entirely from GitHub's REST API (`/commits`, `/commits/{sha}`,
  `/readme`, `/releases`) - commit metadata and text diffs only. No file was downloaded for this
  part.
- **§B** (payload archive) required downloading the archive to inspect it. This was done as
  follows, to keep the payload contained and never executed:
  1. The archive was fetched into a Docker named volume by a short-lived, network-enabled
     container (`remnux/remnux-distro`) - download only, no extraction or analysis in that step.
  2. Every extraction and analysis step afterward ran in a **separate** container started with
     `--network none` (no network interface at all, not even a loopback to other containers), so
     nothing in that stage could reach out even if it tried.
  3. Only static-analysis tooling was used: `file`, `sha256sum`, `strings`, `xxd`, `objdump -p`
     (PE import tables and timestamps), `grep`. **The payload was never run** - `minify.exe` is a
     Windows PE binary, and REMnux is Linux with no Wine installed, so it could not have executed
     even by accident.
- This document will be updated if the repository, its payload, or GitHub's response to it, changes
  materially.
