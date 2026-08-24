# Security Documentation

> ℹ️ This project was renamed from `gvm-rs` to `gvsn` in August 2026. The guides and reports below
> predate the rename and correctly refer to the project under its former name throughout - this
> preserves the historical record as it was originally documented and verified.

This section documents real security incidents affecting this project, written for a general
audience: users, maintainers of other projects, and security researchers. The goal is that anyone
can understand **what the attacker did, how the malware behaves, and how to recognize the same
pattern** - without needing to be a reverse engineer.

## Guides

- **[Anatomy of a Malicious Repository Clone](anatomy-of-a-malicious-clone.md)** - *start here.*
  The full story of how an attacker cloned `gvm-rs`, hid malware behind fake commits and a renamed
  LuaJIT interpreter, and tried to trick users into running an infostealer. Written in plain
  language with diagrams, the complete attack timeline, the malware's behavior step by step, the
  current takedown status, how the same threat reaches package registries (crates.io/npm/...), and a
  "how to protect yourself / your project" checklist. **Recommended first read.**

## Technical analyses

- **[Defensive Analysis: Lua-based Trojan](lua-trojan-defensive-analysis.md)** - the deep technical
  companion: delivery chain, obfuscation, the instrumented LuaJIT harness used for dynamic analysis,
  the full behavioral evidence, and the complete IOC tables.

## Incident reports

- [`incidents/2026-06-malicious-clone-foremost-headsail607.md`](incidents/2026-06-malicious-clone-foremost-headsail607.md)
  - the formal incident record: commit-history reconstruction (§A), static payload analysis (§B),
  and dynamic/behavioral analysis (§C), with methodology.

## Policy

- [`SECURITY.md`](../../SECURITY.md) - reporting process and the rules that define a malicious
  fork/clone (what the project treats as an incident and how it responds).
