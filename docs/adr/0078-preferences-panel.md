# ADR 0078 — Preferences: an admission rule, then two settings

**Status:** Accepted — 2026-08

## Context

The **File ▸ Preferences…** entry has existed since
[ADR 0020](0020-menu-bar.md), visible and **disabled**, because
[ADR 0019](0019-distribution-i18n.md) had put the language choice "out of
immediate scope". Fifty-eight ADRs later, it is still the menu bar's only inert
element.

Two decisions are now waiting on this place:

* [ADR 0019](0019-distribution-i18n.md)'s **language choice**, deferred;
* [ADR 0077](0077-application-updates.md) §2's **consent to the update check**,
  which explicitly declares that it depends on this panel to be deliverable.

### The real problem is not the two settings

Writing a two-line dialog does not call for an ADR. What does is that a
preferences panel is a **room that fills up**: every future decision that
hesitates between "decide" and "let them choose" will from now on have a place
to drop its question, and a preference is the commonest way of not deciding. The
repository has so far refused that reflex three times, without ever writing it
down as a rule:

* [ADR 0022](0022-default-library-fallback.md) — the default library's location
  is shown in *About*, "no new setting, no dedicated window";
* [ADR 0075](0075-preview-cache-retention.md) §2 — the retention window is "a
  named constant, not a setting: a user should not have to arbitrate a cache
  size";
* [ADR 0077](0077-application-updates.md) — the update URL is hard-compiled, "a
  configurable update URL is a hijacking surface, not a convenience".

Those three refusals were each argued on their own. The day the panel exists,
they need a common reason, otherwise the fourth case will be decided by what is
easy.

### What the brick already gives, which settles half the questions

Three facts, verified in `slint` 1.13.1 and in the repository, not assumed:

* bundled translations go through `translate_from_bundle`, which reads a
  **property** (`translations_dirty`) and therefore registers a dependency:
  changing language **re-evaluates everything displayed**, without rebuilding
  anything;
* `select_bundled_translation` must be called **after** the first component is
  created — which is already what `main.rs` does;
* the repository contains **no `slint::tr!()` on the Rust side**: every visible
  string lives in the `.slint` files, in `@tr(...)`. Nothing therefore escapes
  the re-evaluation.

A hot language change is possible, and it did not have to be designed: it had to
be observed.

## Decision

### 1. An admission rule, written before the first setting

A setting enters Preferences if **all three** conditions hold:

1. it concerns the **installation** — not a photo, not a library, not a view;
2. it must **survive a relaunch**, failing which it belongs to the screen where
   it acts;
3. it has **no natural place** in the surface it governs.

And one prohibition that is not negotiable, whatever the three conditions say:
**no preference may change a pixel.** `pipeline.md` §5.1 promises that a render
is a function of the revision alone; an application setting that entered the
render would make the same revision produce two images on two machines, and the
promise would become false without a single line of the pipeline having moved.
That is what definitively excludes from here the export defaults, the demosaic
algorithm ([ADR 0061](0061-demosaic-algorithm.md), which is a field of the
revision, precisely for that reason), and everything that will resemble them.

The rule also rejects, right away, an obvious candidate: the **Basic/Full** mode
of [ADR 0054](0054-first-run-and-basic-mode.md) fails condition (3). Its switch
sits at the head of the panel it governs, where one finds it by looking for it;
moving it into Preferences would make it **less** discoverable, not more
configurable.

### 2. A modal dialog, not a window and not a view

`ui/dialogs/preferences.slint` and `wiring/dialogs/preferences.rs`, mounted by
the existing `DialogOverlay` ([ADR 0045](0045-studio-ui-modularisation.md) §3),
like the ten other dialogs.

A second window would be the first thing to break: Studio has only one, and its
library switch **relaunches the process** (`relaunch_into`, `src/library.rs`)
precisely so as not to have to tear down and rebuild a window's state. A view on
the same footing as Library, Develop and Map would be the other mistake: a view
is a place where one works, Preferences is a place one passes through.

The **File ▸ Preferences…** entry is enabled. **No keyboard shortcut**: one
opens this panel twice in an installation's life, and a key is better spent on a
develop tool.

**Neither OK nor Cancel.** Every change applies and is written the moment it is
made; the only button is *Close*. A Cancel button announces a transaction, and
there is nothing here that can be half-applied. It is also what the rest of
Studio already does: a develop slider is not confirmed.

**No tabs, no search, no "restore defaults".** A two-setting panel disguised as
a suite of settings is worse than a two-setting panel. The day the content calls
for groups, that is that day's decision.

### 3. The language changes hot, and the default stays the system

Three choices: **System language** (default), **English**, **Français**.

The first is not a synonym of the second, and the stored setting distinguishes
them: *absent* means "follow the system", an explicit value means "this one,
whatever the system says". Conflating the two would work today and be wrong the
day a third translation arrives, or the day somebody changes their operating
system's language.

The change is **immediate**: the whole interface switches under the cursor,
without relaunching, for the reasons observed in Context. Two limits, which are
stated rather than repaired:

* a message **already displayed** in the status line keeps the words it was
  produced with. That is correct: it is the account of a past event, not an
  interface label;
* the text of errors reported by the engine is translated in **no** language
  today — the switch regresses nothing, it only makes visible what was already
  true.

**Startup order**, constrained by the brick: read the preferences, create the
window, then apply the language — `select_bundled_translation` requires an
existing component. A stored language wins over the system locale; in its
absence, [ADR 0019](0019-distribution-i18n.md)'s behaviour is unchanged.

**Naming the languages costs one line of Rust, and
[ADR 0019](0019-distribution-i18n.md) is corrected accordingly.** That ADR
promised that adding a language would touch "neither the Rust code nor the
`.slint` files". That was true as long as no menu named them. The list the brick
knows is `["", "fr"]`: an empty tag for the source language, and not a single
readable name — a language menu shows *Français*, not `fr`, and above all not an
empty string. Each language's native name therefore lives in a small Rust table,
and adding a language now costs a `.po` **and** a line. Inventing a private
`.po` header to carry that name would mean giving ourselves a format extension
for two entries.

The safeguard fits in a unit test: the directories of `translations/` and the
name table must correspond exactly. A `.po` added without its name fails the test
instead of producing a dead menu entry.

### 4. The update consent: asked at the second launch, and every exit counts as "no"

[ADR 0077](0077-application-updates.md) §2 fixes three things this one cannot
change: the question is asked **once**, the default before an answer is **no**,
and it **never** interposes itself in front of a first launch — that screen
belongs to [ADR 0054](0054-first-run-and-basic-mode.md). What remains to decide
is *when* it is asked, and what a non-answer is worth.

**At the second launch**, when the window opens. The first launch already has
its purpose; the second is the first moment the application has nothing else to
say. Knowing that requires state, and it is a launch counter that **saturates at
2**: the file never learns anything more than "this is not the first time",
which is exactly what the decision needs.

**Every way of leaving the dialog counts as "no", and is stored as such.**
Escape, a click on the backdrop, the *Don't check* button: three gestures, one
result, and the question is never asked again. That is what structurally
prevents this dialog from becoming harassment — it has no second try. It says so
itself, in one line under the buttons: *you can change your mind in
Preferences*. Without that sentence, a silence read as a refusal would be a
trap; with it, it is a reversible default.

If the answer is yes, this launch's check takes place — it follows
[ADR 0077](0077-application-updates.md) §2's cadence like any other, there is no
special first case.

**Why ask the question, rather than settle for the checkbox.** Because
[ADR 0077](0077-application-updates.md) already rejected that option under its
real name: "check only manually" fails because almost nobody clicks. A checkbox
nobody opens is the same thing, with one extra box.

### 5. One file, next to the ones that already exist

`preferences.json`, in the configuration directory that
`directories::ProjectDirs::from("", "", "Leyline")` designates — the one where
`recent_libraries.json` (the recent-libraries list, `src/library.rs`) and the
`detectors/` folder ([ADR 0073](0073-external-mask-detectors.md) §3, which chose
that location for the same reason) already live. A third occupant, no new
convention.

Four fields, all optional:

| Field | Meaning |
| :--- | :--- |
| `language` | language tag; **absent** = follow the system |
| `update_check` | `true` / `false`; **absent** = never asked |
| `last_update_check` | timestamp of the last successful check ([ADR 0077](0077-application-updates.md) §2, 24 h ceiling) |
| `launches` | launch counter, saturated at 2 (§4) |

The last two are not settings but state written by the application; they are
named here because **this ADR owns the file**, and state of the same scope and
the same lifetime does not deserve a second file for the sheer beauty of
classification.

Two properties to hold:

* **atomic writing** (temporary file then rename): a file truncated by a power
  cut must not erase a consent already given;
* **every failing read falls back to the defaults, and every default is
  offline.** A file that is missing, unreadable or corrupt can therefore *never*
  enable a network check — at worst it asks the question again, whose
  before-answer answer is "no". The direction of that degradation is the only
  security point in this whole ADR.

### 6. What this ADR does not do

* **No migration, no catalog schema change**: the preferences do not go near it.
* **No pixel, no stage version** — by construction (§1).
* **Neither the CLI nor the SDK reads this file.** A command takes its
  arguments; a script whose behaviour depended on a box ticked one day in a
  graphical interface would be irreproducible for a reason invisible from its
  command line.
* **Basic/Full mode is not moved here**, and its *persistence* — which it does
  not have today — stays an open question belonging to
  [ADR 0054](0054-first-run-and-basic-mode.md), not to this one.
* **macOS is not handled**: the convention there is a `Cmd+,` in the application
  menu, not a File-menu entry. As long as macOS distribution stays behind
  ([ADR 0019](0019-distribution-i18n.md),
  [ADR 0077](0077-application-updates.md)), it is a difference with no carrier.

## Consequences

* **[ADR 0077](0077-application-updates.md) becomes deliverable.** That was its
  only unsatisfied dependency.
* **The menu bar's last inert element disappears.**
  [ADR 0020](0020-menu-bar.md) had placed a disabled entry "for the time being";
  it will have waited fifty-eight ADRs, which is a lesson about disabled entries
  more than about preferences.
* **The admission rule (§1) is the part that will get used.** The two settings
  are written once; the rule will be cited every time a future decision wants to
  turn itself into a checkbox. It also makes retroactively explicit the refusals
  of [ADR 0022](0022-default-library-fallback.md),
  [0075](0075-preview-cache-retention.md) and
  [0077](0077-application-updates.md), which had each argued them on their own.
* **A hot language switch is a free translation-audit tool.** Choosing *Français*
  and walking through the window shows in seconds every string left outside
  `@tr(...)` — which is exactly the silent failure mode the stale `.pot`
  produces at every interface slice.
* **A consent can be lost with the configuration file**, and the question will
  then be asked again. That is the accepted price of making every failure path
  degrade towards "no".
* **The configuration directory now has three occupants** — the recent-libraries
  list, the detector manifests, the preferences. It is still a flat directory;
  it will not be indefinitely.

## Alternatives rejected

* **A separate preferences window.** The convention of most desktop
  applications, and the one thing that would force Studio to know how to manage
  two windows — whereas its library switch is built on the opposite assumption
  (relaunch the process rather than rebuild a state). A modal dialog gives the
  same result without touching that assumption.
* **A Preferences view**, on the footing of Library, Develop and Map. A view is
  a workplace with its own layout and its own shortcuts; two settings do not
  make one.
* **Storing the preferences in the catalog.** The wrong scope, and two concrete
  effects: someone keeping two libraries would answer the consent question
  twice, and a library placed on an external disk would carry the language of
  the application that created it. The catalog describes photos, not an
  installation.
* **A commented TOML file, designed for hand editing.** More pleasant to read,
  but hand editing is not a goal — and it would be a second format in a
  directory that already has one (`recent_libraries.json`, the detector
  manifests). Consistency is worth more than the comfort of a file nobody will
  open.
* **Asking the consent question at the first launch.** Forbidden by
  [ADR 0077](0077-application-updates.md) §2, and for a good reason:
  [ADR 0054](0054-first-run-and-basic-mode.md) designed that moment to explain
  how to get photos in. A question about the network would arrive there before
  anyone knew what the software asking it is.
* **Never asking the question, and settling for the checkbox in the panel.** The
  most discreet, and already rejected under another name by
  [ADR 0077](0077-application-updates.md): attentive people are informed, the
  others stay on their version indefinitely, security fixes included.
* **Asking again if the dialog is closed without an answer.** It would catch one
  or two more answers, and it is exactly the mechanism by which software becomes
  unpleasant. A non-answer is an answer; the panel exists to change it.
* **A restart to apply the language**, as the library switch does. It would have
  been the default choice without the verification made in Context — and it would
  have been pointless: the brick re-evaluates the strings by itself, and the only
  reason to relaunch would have been not having read its code.
* **Deriving the language list from the brick alone**, to keep
  [ADR 0019](0019-distribution-i18n.md)'s promise to the letter. The list exists
  (`["", "fr"]`) but is reachable only in `select_bundled_translation`'s error
  variant — one would have to request an impossible language to read the answer —
  and it carries no displayable name. A menu built that way would offer an empty
  entry.
