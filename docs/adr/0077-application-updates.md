# ADR 0077 — Updating: a signed manifest, and a question asked once

**Status:** Accepted — 2026-08

## Context

The repository opens soon, and [ADR 0019](0019-distribution-i18n.md) delivered
the three ways to install Leyline — AppImage, NSIS installer, `.dmg`. None of
them says what happens **next**. Someone who installs 0.1.0 today has no way of
learning that 0.2.0 exists, short of going back on their own to a download page
whose address nothing gave them.

It is the last gap in getting distribution done, and it was **explicitly
deferred** on 2026-08-02 to "before opening up": with no published binaries, an
update mechanism points at a void.

### What the vision imposes, and what is not negotiable here

[`vision.md`](../vision.md) allows the network for exactly this use, and sets
the constraint in the same sentence:

> The network serves only **optional and explicit** uses: checking that an
> update exists, downloading it, possibly sharing a preset.

"Optional and explicit" is not a matter of style: it is what separates Leyline
from software that phones home. Nothing that follows can send anything other
than **the installed version and the platform**, nor leave without someone
having asked for it.

### What the packaging brick already gives

`cargo packager` is already the packaging chain (ADR 0019). Its companion brick
`cargo-packager-updater` reads a JSON manifest, compares versions, downloads the
current platform's package and replaces it. It brings three things we do not
have to write:

* the **package signature** (a minisign key pair: the private key signs at
  publication, the public key is compiled into the binary) — without it, an
  update is a download of an arbitrary executable.

  > **What is signed, precisely** — verified in `cargo-packager-updater` 0.2.3:
  > the JSON manifest itself is **not signed**, it *carries* each package's
  > minisign signature, and that signature is verified against the downloaded
  > bytes before any installation (`verify_signature`, called by
  > `Update::download`). The property obtained is the one that matters: nobody
  > can get a binary installed that they did not sign. What it does not cover,
  > and what HTTPS alone holds: redirecting to an **older published version**,
  > or preventing the discovery of an update. A repository that lost control of
  > its releases would have a bigger problem than that one anyway;
* the **in-place** replacement per platform: an AppImage rewrites itself, and
  the NSIS installer is already in `installer-mode = "currentUser"`
  (`crates/leyline-studio/Cargo.toml`), so **no UAC elevation** is asked for;
* the macOS case, which requires the `.app` to be **notarized** before a
  replacement is acceptable — a platform constraint, not a design one.

## Decision

### 1. The manifest lives on GitHub Releases, at a URL that never moves

The manifest is a release asset named `latest.json`, and the binary queries the
stable redirect GitHub maintains:

```
https://github.com/leyline-studio/leyline/releases/latest/download/latest.json
```

That URL always designates the most recent release's asset, without its text
changing from one version to the next. That is what allows it to be
**hard-compiled** — and it must be: a configurable update URL is a hijacking
surface, not a convenience. Changing it is a release, not a setting.

The choice turns on a single question: **what is the project willing to have to
keep alive?** An installed binary queries its URL for years. A domain, a TLS
certificate and hosting are three things to renew indefinitely on pain of
breaking already-installed copies; releases are where the binaries go
**already**, and the manifest travels with them, published by the same gesture.
A local-first project that refuses to operate a service for its users is not
going to operate one for its own updates.

The private signing key lives **neither in the repository nor in CI**: it signs
by hand at publication time. The repository carries only the public key.

### 2. We ask once, then check by ourselves — but we never install by ourselves

It is Firefox's cadence without its silence, and the two halves separate
cleanly.

**Consent is asked once, and counts as "no" until it has been given.** "Check
automatically whether an update exists?" — one question, one stored answer,
never asked again. That is what makes the rest conform to `vision.md`: the check
stays **optional and explicit** in the strong sense — someone authorized it
knowingly — rather than in the literal sense of a click every time. A default of
"no" is what separates that reading from a permission extracted by pressure.

**If it is yes: one check per launch, and no more.** A few seconds after the
library opens, never during — and skipped if the last one succeeded less than
24 h ago, so that five launches in an afternoon do not make five requests. **No
timer during a session:** an editing session lasts hours, and a network call
leaving in the middle of it is exactly the surprise the local-first principle
refuses.

**Nothing found, nothing said.** No "you are up to date" notification, no log
entry. The normal case is total silence.

**Something found: a badge, not a window.** The menu bar's **Help** label
carries a discreet mark, and the *Check for updates…* entry becomes *Update to
0.2.0…*. No modal interposes itself, nothing interrupts the work in progress,
and the information waits until it is looked at.

> That badge is possible **because the menu bar is hand-written**
> ([ADR 0020](0020-menu-bar.md), correction of 2026-08-05): a native `MenuBar`
> cannot be decorated. The workaround for a repaint bug turns out after the fact
> to be what makes this surface available.

**Installation always asks for a click**, and that closes §4's danger for free:
a catalog migration is irreversible, and nobody can undergo one they did not
trigger. It is the half of Firefox we do not take, and not only out of caution —
the brick cannot give it (see §Alternatives rejected).

**Where the setting lives.** In the **Preferences** panel — the File menu entry
had been disabled since [ADR 0020](0020-menu-bar.md) because
[ADR 0019](0019-distribution-i18n.md) had put the language choice "out of
immediate scope". That panel had **two tenants** and a reason to be built; it is
its own ADR, and the present decision depended on it to be deliverable.
[ADR 0078](0078-preferences-panel.md) wrote it, and both are delivered. What is
fixed here and what it cannot change: the question is asked once, the default
before an answer is "no", and it never interposes itself in front of a first
launch ([ADR 0054](0054-first-run-and-basic-mode.md) owns that screen).

> It is [ADR 0078](0078-preferences-panel.md) that writes it, respecting those
> three constraints: the question is asked on the **second** launch, **every way
> of closing the dialog counts as "no"** and is stored as such, and the setting
> lives in `preferences.json` alongside `last_update_check` — the present
> paragraph's 24 h ceiling reads that field.

### 3. Nothing is sent, and nothing is installed without a second gesture

* The request carries only what the URL contains — **no identifier, no counter,
  no library data**. The installed version and the platform are known to the
  client, not transmitted as telemetry: they serve to pick a line of the
  manifest, locally.
* A network failure is **not** an application error: offline is Leyline's normal
  state. The dialog says it could not reach the server and closes; nothing
  retries.
* Finding a version does not install it. The dialog shows the number and the
  release notes, and waits for a second click.

### 4. The catalog is backed up before a newer version touches it

This is the part of this decision that has nothing to do with the network, and
the only one whose absence can **lose work**.

`Catalog::open` applies the pending migrations, one transaction per migration.
They have **no rollback**: there is no down script, and `Catalog::open`
**refuses** a catalog newer than the engine (`LeylineError::NewerCatalog`).
Today that is inconsequential — one only installs a new version deliberately.
With a one-click update, the sequence "I update, the migration runs, I want to
go back" becomes reachable by accident, and the old version can no longer open
the library.

So: **before applying a migration, `catalog.db` is copied into `Backups/`**,
under a name carrying the schema version being left. The `Backups/` folder has
existed in a library's skeleton since [ADR 0010](0010-relative-paths.md) and
`catalog.md` §3 — it is created by every `Library::create` and **nothing has
ever written to it**. This is its use.

**It is not a file copy, and it could not have been one.** The connections are
in **WAL** mode (`docs/catalog.md` §6): committed pages may still live in
`catalog.db-wal` and not in `catalog.db`. Copying the one file therefore
produces a backup that is silently missing the most recent work — precisely what
one would want to get back. `VACUUM INTO` is what is used: SQLite itself builds
a coherent image of the whole database into one file. The test verifies it by
keeping a write connection **open** during the migration, because closing it
would be enough to fold the WAL back into the main file and let a naive copy
pass.

Three details that make the difference between a backup and an illusion:

* the copy is made **before** the first migration and **only once** per open,
  not one per migration: what we want to restore is the state before the update,
  not an intermediate state;
* if the copy fails, the open **fails** instead of migrating anyway. An
  irreversible migration on an unsaved catalog is precisely what this paragraph
  exists to prevent;
* it triggers only if there really is a migration to apply — opening an
  up-to-date library copies nothing, otherwise the folder would grow at every
  launch.

This part is **independent of the rest of the ADR**: it does not touch the
network, it is useful immediately, and it is delivered without waiting for there
to be binaries to download.

### 5. What this ADR does not do

* **No library migration, no schema change.**
* **No pixel, no stage version.** `pipeline.md` §5 is not implicated: an update
  can change the rendering — that is what stage versions are for
  ([ADR 0042](0042-versioned-stage-pipeline.md)) — but nothing here touches the
  contract. And the reassuring corollary is worth writing, because the
  spontaneous fear is the opposite: **an update does not retouch any
  already-developed photo.** A revision carries its stage map and the engine
  honours the version it records
  ([ADR 0043](0043-collapse-prerelease-render-history.md)); a newer stage version
  applies only on **reprocessing**, which is an explicit act. The only state an
  update modifies unasked is the **catalog schema** — hence §4.
* **The CLI and the SDK do not update themselves.** A Rust library is updated by
  the package manager of whoever uses it; a command-line binary, by the
  distribution that installed it. It is Studio, an application delivered by
  installer, that has the problem.

## Consequences

* **Someone who installs Leyline learns that a version exists without having to
  think about it**, and without Leyline observing who they are. That was ADR
  0019's last distribution gap.
* **What the check nevertheless reveals, and must be said.** No data is
  *transmitted* (§3), but an HTTP request reveals some by its mere existence: the
  IP address, and the fact that a copy of Leyline launched at that instant. One
  check per launch, capped at one per 24 h, makes that signal a coarse trace of
  usage at the host — not an identity, not an editing history, but not nothing
  either. That is exactly what the default "no" and the question asked once leave
  each person to decide, and it is also why there is **no in-session timer**: one
  request per working day, not a clock ticking.
* **The project operates no service.** No domain, no certificate, no hosting:
  update availability is GitHub's availability, which is already the source
  code's availability. The corollary is owned: changing host one day will require
  a transition release, which copies installed before it will not see.
* **A private key becomes a project asset.** Losing it means publishing a new
  public key in a binary, hence a manual update for everyone. It does not live in
  CI, which makes publishing a release manual — that is the price of not letting
  a machine sign executables on its own.
* **`Backups/` stops being an empty folder** and becomes the place one starts
  again from when an update has migrated a library one wanted left as it was.
* **macOS stays behind**, as for packaging
  ([ADR 0019](0019-distribution-i18n.md), and the deferred `.dmg` build):
  without notarization, replacing the `.app` is not offered. It is not a design
  exception, it is the same dependency that already blocks macOS distribution.

## Alternatives rejected

* **An update server operated by the project** (domain + static manifest). It
  would give independence from GitHub, at the price of infrastructure to
  maintain as long as the oldest installed copy. A project that puts "no cloud,
  no accounts" in its vision does not start by giving itself a service every
  installation depends on.
* **GitHub Pages rather than release assets.** A second publication surface for
  the same bytes, to be kept in sync with the releases by hand. The
  `releases/latest/download/` redirect does the same work with no second gesture.
* **A configurable update URL.** Useful for testing, and that is exactly why it
  is dangerous: the setting that helps testing is the setting that redirects an
  installation to a binary chosen by someone else. The tests target a
  compile-time URL, not a runtime setting.
* **Checking at startup by default, with a setting to disable it.** That is most
  applications' behaviour, and it inverts `vision.md`'s sentence: the network
  would leave without having been asked, the checkbox serving only to repair
  after the fact. The default is what counts in "no silent check" — hence §2's
  question asked once, which gets the same result by asking for it.
* **Checking only manually** (the position held by this ADR's first draft).
  Defensible — a click *is* consent, and nothing to design — but almost nobody
  clicks: attentive people are informed, the others stay on their version
  indefinitely. The risk that leaves open is nameable: Leyline has no account, no
  sync and no server, so its attack surface is **parsing a file one opens
  oneself**, essentially LibRaw. A fix that never arrives protects from files one
  will open anyway. The question asked once costs a Preferences panel that has to
  be built regardless.
* **The full Firefox model: download and apply silently**, the new version taking
  effect at the next launch. That is what makes Firefox pleasant — one never sees
  it coming — and `cargo-packager-updater` **cannot do it**:
  `download_and_install()` installs *now*, there is no queuing. "Firefox-style"
  with this brick would therefore be **more** intrusive than Firefox, the
  installer firing while one works. Reproducing it would require writing deferred
  replacement per platform (AppImage, NSIS), which is exactly the work
  [ADR 0019](0019-distribution-i18n.md) chose `cargo packager` to avoid. There is
  also a reason specific to Leyline that Firefox does not have: an update
  **migrates the catalog** with no way back, after which the previous version can
  no longer open the library. Undergoing that without having triggered it is not
  acceptable. To be reopened if the brick ever gains deferred installation — the
  question would then be the catalog's alone.
* **Doing nothing and letting package managers handle it** (Flatpak, winget,
  Homebrew). That would be the right answer if Leyline were published there; ADR
  0019 chose three standalone installers precisely because it is not. To be
  reopened the day it is — it would then be this ADR being replaced, not
  completed.
* **Backing up the catalog at every open**, rather than before a migration.
  Copying tens of megabytes at every launch for an event that happens once a
  year, and filling `Backups/` with copies one could no longer tell apart.
