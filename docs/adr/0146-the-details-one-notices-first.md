# ADR 0146 — The details one notices first

**Status:** Accepted — 2026-09

## Context

`UX-REVIEW.md` §2.8 lists five small things, none of which is a module and all
of which are seen in the first ten minutes: a metadata row that is blank where
every other row says « — », a grid whose rhythm breaks on a portrait frame, a
survey view that looks like the loupe when one photograph is selected, a window
that ignores a dropped folder, and a *View* menu that lists places to go and
nothing one does to a view.

They are grouped here because they share a cause worth naming: each is a place
where the interface knows the answer and declines to draw it. Nothing below
needs the engine.

## Decision

### 1. A row with nothing to say says « — »

`format::exposure_line` joins ISO, shutter, aperture and focal length, and
returns the empty string when it has none of them — which happens on a scanned
JPEG, on a phone photograph stripped by an exporter, and on every file whose
EXIF the decoder could not read. The two rows above it, *Camera* and *Lens*,
already answer « — » in the same case (`wiring/grid.rs`), so the panel showed
two dashes and a hole.

The function returns « — » when it has nothing, and the call site's
`String::new` fallback — metadata absent altogether — becomes the same string.
The rule, for the next row added to that panel: **an absent value is a dash.**
A blank row reads as a defect in the layout; a dash reads as an answer.

### 2. One rhythm for the grid

A grid cell was square. The image area inside it is not: the filename and
stars take the foot, so at the default 176 px cell the thumbnail was fitted
into 156 × 138. `image-fit: contain` then draws a 3:2 landscape frame at
156 × 104 and a 2:3 portrait at 92 × 138 — the portrait stands a third taller
than both its neighbours, and a row of mixed orientations has no baseline.

**The cell becomes 20 px taller than it is wide**, which is exactly the foot
it carries, so the image area above the name is itself square; the thumbnail
is fitted into that square, centred. Both orientations then have the same long
edge, and neighbouring cells share a top and a bottom.

That is the rule of a physical contact sheet, which is what a grid is a
picture of — and the same rule [ADR 0110](0110-contact-sheets.md) already
applies when Leyline prints one.

Squaring the *image area* and not the *cell* is the decision, and it was made
on screen. Inscribing the photograph in the largest square a **square** cell
can hold evens the rhythm just as well, and does it by shrinking every
landscape thumbnail from 156 px to 138 px: a rhythm bought by taking 11 % away
from the orientation most photographs are in. Growing the cell instead costs
20 px of vertical density and nothing else.

Nothing is cropped to fill the square: a photograph in this product is never
silently reframed, and the review's complaint was the *rhythm*, not the size.

### 3. A survey of one says what a survey is for

Entering *Survey* (`N`) with a single photograph selected gives a view that
cannot be distinguished from the loupe: the ✕ that makes the mode a funnel is
hidden below two photographs ([ADR 0057](0057-compare-and-survey.md) §4), and
the view has no filmstrip to add a second from.

The toolbar line, which today reads « Cliquez ✕ pour retirer une photo de la
sélection » whatever the count, now says the state instead: « 4 photos
comparées · ✕ en retire une », and with one photograph, « Une seule photo : la
mosaïque compare une sélection — G, puis Ctrl+clic pour en ajouter. » The mode
stays enterable and the way out is the sentence.

Two refusals. **Disabling `N` below two photographs**: a shortcut that does
nothing teaches nothing, and the photographer who pressed it is exactly the one
who needs the sentence. **Extending the selection to the neighbours** on entry:
the selection is the photographer's, and no view may write it — the same rule
[ADR 0084](0084-assisted-culling.md) §2 keeps for culling.

### 4. A dropped folder opens the import dialog

Dragging a folder onto the window did nothing at all, and it is the first
gesture many people try. Slint 1.13 has no drop target — it has no drag either,
which [ADR 0130](0130-direct-manipulation.md) §1 had to work around from the
inside. But Studio holds the winit backend (`unstable-winit-030`, already used
to size the window to the screen), and
`BackendSelector::with_winit_custom_application_handler` delivers
`HoveredFile`, `DroppedFile` and `HoveredFileCancelled` on the UI thread.

* **Hovering** draws a frame across the window naming what a drop will do. A
  gesture that gives no feedback until it is finished is one people abandon
  halfway.
* **Dropping opens the import dialog, source filled in — never an import.**
  Nothing is written by a gesture a pointer can make by accident. That is the
  rule [ADR 0065](0065-selective-import.md) already keeps, and the dialog
  it built is exactly the surface a drop should land on.
* **Dropped files, rather than a folder**: the source becomes their common
  parent, the scan runs on its own, and **only the dropped files are ticked**.
  Dropping three photographs and importing the eight hundred beside them would
  be a lie about the gesture. A dropped folder ticks nothing in particular —
  the folder *is* what was pointed at.
* winit sends one event per file and no end-of-batch marker, so the paths are
  accumulated and flushed from `about_to_wait`, which runs once the burst has
  been dispatched.
* **The overlay clears itself on the next pointer or key event**, and not only
  on `HoveredFileCancelled`. That event comes from the *other* process, and a
  drag source that dies mid-drag never sends it — reproduced, and the window
  then stays dimmed with nothing able to clear it. During a real drag the
  source holds the pointer grab and the target sees no input of its own, so
  the first event that arrives means the drag is over either way.
* Installing the handler is **best-effort**: a `BackendSelector::select()` that
  fails is ignored and Studio starts exactly as it did before, the rule
  [ADR 0122](0122-startup-failure-window.md) §5 sets for everything on the
  launch path.

What is testable is kept pure: `drop_target(paths)` answers the folder and the
files to tick, and is where the common-parent cases live.

### 5. The View menu carries the view

*View* listed six places — Library, Loupe, Compare, Survey, Develop, Map — and
nothing one does *to* a view. Four of the five entries below existed only as
keys on the shortcut card, which is to say they were learnable by someone who
already knew they existed.

| Entry | Key | Where |
|---|---|---|
| Plein écran | `F` | anywhere |
| Replier les panneaux | `Tab` | anywhere |
| Tout replier | `Maj+Tab` | anywhere |
| Vignettes plus grandes / plus petites | `+` / `-` | library grid |
| Zoom 100 % | `Z` | loupe, compare, develop |

Full screen is the only new capability, and it is one property:
`Window.full-screen`. `F` is free, and is the letter Lightroom uses — the
keyboard of this product is that one ([ADR 0055](0055-library-navigation.md)).
`+` and `-` step the thumbnail size by 24 px between the slider's own bounds,
110 and 320: the menu and the slider must not disagree about the range, so the
menu asks the same clamp.

Every row carries its shortcut, which is the point of putting them here at all:
a menu is where a keyboard is learned.

## Consequences

* Five defects of `UX-REVIEW.md` §2.8 close, and with them the last open row of
  its tracking table.
* Studio gains a backend selection step before its first window. It is
  best-effort, and its failure mode is the behaviour of every build until now.
* `F`, `+` and `-` become bound keys; the shortcut card lists them.
* The grid change is geometry in `browser.slint` only — the row height the
  virtual scrolling, the scroll-into-view and the cell placement all count in.
  Thumbnail *generation* is untouched: nothing in the cache, the catalog or the
  pipeline moves.

## Alternatives rejected

* **Cropping grid thumbnails to fill the square** (Apple Photos, Google Photos).
  It gives a perfect rhythm and shows a frame the photographer never chose —
  and in a culling tool the frame is the thing being judged.
* **Making the cell itself follow the photograph's aspect** (a masonry grid).
  The cell is a hit target, a badge frame and a drop target; a variable one
  breaks arrow-key navigation, which counts in rows and columns.
* **Keeping the cell square** and inscribing the thumbnail in the largest
  square it holds. Same rhythm, and every landscape thumbnail loses 11 % of
  its long edge — see §2.
* **Importing on drop, without the dialog.** Faster, and the first accidental
  drag of a 38 000-file folder would copy it into the library — the defect
  [ADR 0139](0139-cancelling-a-batch.md) had to build cancellation for.
* **A drop target per panel** (drop on a collection to import into it). The
  window is one target because the answer is one dialog; a collection accepts
  photographs already in the library, and that drag already exists (ADR 0130).
* **`F11` for full screen.** It is the browser's key, not the photo
  application's; `F` is what a Lightroom user presses.
