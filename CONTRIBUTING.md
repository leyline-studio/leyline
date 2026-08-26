# Contributing to Leyline

Thank you for looking. This file is the English entry point GitHub expects at
the repository root; the full guide lives in
[`docs/contributing.md`](docs/contributing.md) and is written in French, like
the rest of `docs/`. Everything below is a summary of it, plus the two or three
things that surprise newcomers.

## The one rule that shapes everything else

> **No code before architecture. No architecture before vision.**

A change that alters a decision — how the pipeline is ordered, what a stage
does to pixels, where data lives, what the application refuses to be — starts
with an **Architecture Decision Record** in [`docs/adr/`](docs/adr/), not with
a patch. This is not ceremony: the ADRs are the reason the project can promise
that a photo edited today renders identically in ten years, and a change that
skips them cannot be audited later.

Until the first public release, ADRs are freely editable: one that no longer
matches the implementation is corrected in place. After it, they freeze and are
replaced by new ADRs, never edited.

## Before you open a pull request

```sh
make check     # rustfmt + clippy (zero warnings) + the whole test suite
```

`make check` is the gate. Everything else — `make`, on its own, lists the
targets — is a convenience wrapper around a plain `cargo` command.

Three expectations that are stricter here than in most projects:

* **Rendering never changes under an existing edit.** If a change would move a
  single pixel of a published pipeline stage, it is a *new stage version* next
  to the old one, never an edit of it. Reference renders enforce this
  mechanically ([`docs/pipeline.md`](docs/pipeline.md) §5).
* **Public APIs are documented, and every feature has tests** — unit,
  integration, and a benchmark when it touches the pipeline.
* **Commits follow [Conventional Commits](https://www.conventionalcommits.org/)**,
  and their body says *why*, not what the diff already shows.

## Licence and the CLA

Leyline is [GPL-3.0-only](LICENSE), plus one [additional permission under
section 7](LICENSE-EXCEPTION.md) that lets Leyline be combined with software
under other terms. Your contribution is covered by it like the rest of the
code, and the permission does not survive modification: a fork is granted
nothing by it.

Commercial licences are intended alongside the GPL community edition over
time, so every contribution is subject to the [Contributor License
Agreement](CLA.md): you keep your copyright, and grant the project the right
to distribute your contribution under other licences as well. Opening a pull
request means you accept it.

The name "Leyline", the logo and the mark are not covered by the code licence —
see [`TRADEMARK.md`](TRADEMARK.md).

## Conduct and security

Everyone taking part is held to the [Code of Conduct](CODE_OF_CONDUCT.md).

**Security vulnerabilities are reported privately**, never as a public issue —
see [`SECURITY.md`](SECURITY.md).

## Where to start reading

[`docs/readme.md`](docs/readme.md) is the map: what the project is, its real
status, and the order the documents are meant to be read in.
