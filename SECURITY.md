# Security Policy

## Supported Versions

Leyline is pre-1.0 and does not yet maintain long-term-support branches.
Security fixes are made against `main` and released in the next tagged
version; only the latest release is supported.

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Use GitHub's [private vulnerability reporting][advisories] for this
repository (Security tab → "Report a vulnerability"). If that is not
available to you, email
**13538858+blackmorth@users.noreply.github.com** with details.

Please include:

* A description of the vulnerability and its potential impact
* Steps to reproduce, or a proof-of-concept (a crafted RAW/catalog file, if
  relevant)
* The affected version/commit

You should receive an initial response within a few days. This is a
volunteer-maintained project, so response times may vary, but reports are
taken seriously and will be acknowledged even if a fix takes time.

## Scope

Leyline is a **Local First** application (see `docs/vision.md`): it has no
network stack, no accounts, and no cloud sync, so the realistic attack
surface is narrower than a typical desktop app but not zero:

* **Untrusted file parsing** — RAW decoding (`leyline-raw` via LibRaw), lens
  metadata (`leyline-lens` via Lensfun), and the SQLite catalog
  (`leyline-catalog`) all parse files that may come from an untrusted
  source (a RAW file downloaded from the internet, a shared catalog). Memory
  safety or logic bugs reachable from opening such a file are in scope.
* **Export/output path handling** — path traversal or symlink issues in
  import/export are in scope.
* **Third-party native dependencies** — LibRaw, Lensfun, and LittleCMS are
  linked in, not vendored-and-forked; a vulnerability in the upstream
  library itself should generally be reported to that project directly, but
  please also let us know so we can track and update the pinned/packaged
  version.

Out of scope: vulnerabilities that require the attacker to already have
arbitrary local code execution on the user's machine (Leyline stores no
secrets and has no elevated privileges to escalate to), and social
engineering.

## Disclosure

We ask for a reasonable period to investigate and release a fix before any
public disclosure. Credit will be given in the release notes unless you
prefer to remain anonymous.

[advisories]: https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing/privately-reporting-a-security-vulnerability
