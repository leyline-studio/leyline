# ADR 0080 — The promise, and its boundary

**Status:** Accepted — 2026-08

## Context

The project has never written down, in one place, what it promises its users.
The pieces exist and are scattered: the principles of [`vision.md`](../vision.md),
the reproducibility contract of [`pipeline.md`](../pipeline.md) §5, and the
exclusions of [`specification.md`](../specification.md) §4. Nobody has ever had
to read all three to know what they are entitled to.

That was harmless as long as nothing pressed against it. Two things now do.

### The exclusions are written as principles, and principles bind the future

`specification.md` §4 gives three reasons in this form:

| Excluded | Reason as written today |
|---|---|
| Cloud | Contradicts the Local First principle |
| User accounts | There is no service to authenticate against |
| Subscription | Contradicts the photographer's ownership of their data |

Read literally, those are not scope decisions — they are statements about what
the project may never become. The day a shared catalog exists, hosting other
people's developed photographs costs real money, and money means accounts and
licences. Under this wording, offering that service would not be an addition to
the product: it would be **the repudiation of a written principle**. The
project would have to choose between its documents and its viability, which is
the one position no document should ever create.

The repository has already refused this trap once, deliberately.
[ADR 0069](0069-closed-extension-boundary.md) settled that a **closed,
commercial** extension is legitimate — provided it produces settings and never
pixels, and never enters the render path. That decision exists; §4's wording
contradicts it.

### The promise's vagueness has already blocked an evolution

[`competitive-plan.md`](../competitive-plan.md) §B3 states that "on export, the
GPU is **forbidden** by the `pipeline.md` §5.1 promise". That reading is wrong,
and it has already been used as a reason not to investigate.

§5.1 does not say the CPU is authoritative. It says that a revision, rendered
with the same settings, the same stage versions, the same input, on the same
platform and toolchain, produces the same bytes. What it forbids is **changing
the pixels of an already published stage version** — not publishing new ones. A
render path that arrives as new stage versions has never been forbidden by
anything.

The distinction was never written, so the strongest available reading won. A
promise that stops work it does not actually forbid is a defect of the promise,
not a virtue.

## Decision

**The project states five guarantees, states what it does not promise, and
fixes in advance the conditions any future optional service must satisfy.**

### 1. The five guarantees

They are properties of the **installed application**, not intentions of the
project. Each is written so that its falsification is obvious:

1. **Your files are never modified.** A source file is read, never rewritten.
2. **Your edits are readable without us.** Settings are documented JSON
   (`pipeline.md` §3.2) inside an ordinary SQLite database with a documented
   schema (`catalog.md`), on your disk. Nothing is encrypted or obfuscated.
3. **The application runs offline, with no account and no licence check, for
   as long as you keep it.** No installed feature stops working because a
   server, a subscription or a company stops. There is no expiry, no
   activation, and nothing to renew.
4. **The same settings give the same pixels**, within the scope
   `pipeline.md` §5.1 states and §5.2 bounds.
5. **Nothing leaves your machine without an explicit action.** No telemetry, no
   usage counter, no silent check. The one network call that exists is the
   update check, which is off until asked for
   ([ADR 0077](0077-application-updates.md)).

These five are the promise. Everything else the project says about itself is
commentary.

### 2. What is explicitly not promised

Stating the non-promises is what makes the promises worth anything.

* **Free hosting of anything, ever.** Storage and bandwidth are somebody's bill.
* **That the project will never sell anything.** [ADR 0069](0069-closed-extension-boundary.md)
  already decided the opposite, and [ADR 0009](0009-gpl3-cla-dual-license.md)
  set up a dual-licence model on purpose.
* **Identical pixels across machines, builds or execution backends.** §5.2 has
  always said so; this ADR does not widen it and does not narrow it.

### 3. An execution backend belongs to the stage version, never to a fallback

This is what unblocks the GPU without touching the promise by a single word.

* A stage version **declares the backend it runs on**. A GPU implementation of
  an operator is a **new stage version**, exactly as a changed formula is
  ([ADR 0042](0042-versioned-stage-pipeline.md)). A published stage version
  never changes backend.
* **A silent CPU fallback is forbidden.** A machine that cannot provide the
  backend a revision cites **refuses to render**, with a named error, in the
  family of `UnknownStage` and `MixedWorkingSpaces` (`pipeline.md` §3.4).
  Rendering the photo through another backend would be showing the user other
  pixels without saying so, which is the failure mode the whole contract exists
  to prevent.
* The residual variation — driver versions, vendor differences — is the same
  category as libm and LLVM, and is already covered by §5.2. It is named there
  rather than added as a new condition to §5.1.
* **The preview path is untouched**, because it was never inside §5.1: it
  already renders from a reduced proxy with scaled radii ([ADR 0041](0041-interactive-preview-rendering.md)).
  A GPU preview requires no decision from this ADR at all.

The consequence is worth stating plainly: **the GPU is not a forbidden subject.
It is an ordinary stage version, with an ordinary cost — every operator ported
must be published as a new version, and the old one kept forever.**

### 4. What any future optional service must satisfy

Written now, while no such service exists and no revenue depends on it. A
project that meets all five is an addition; a project that fails one is not
shipped, whatever it earns.

* **a. The local application stays complete without it.** No existing feature
  moves behind an account. What the service adds is sharing and hosting, never
  editing.
* **b. The catalog on your disk stays the original.** The service holds a copy.
  Never the reverse.
* **c. Everything the service holds is exportable in a documented format, and
  stays usable after the service ends.** The test is blunt: if it shuts down
  tomorrow, the user loses a convenience, never a photograph and never an edit.
* **d. What is sold is hosting and sharing — never the right to run what is
  already installed.** Guarantee 3 is not negotiable against revenue.
* **e. Nothing is transmitted without an explicit action**, and no telemetry
  rides along with it.

These conditions are deliberately the same shape as the six that
[`competitive-plan.md`](../competitive-plan.md) §4 fixed for local AI, and as
the boundary [ADR 0069](0069-closed-extension-boundary.md) fixed for a closed
extension. The project now has one rule for all three: **a paid thing may
propose, host or accelerate; it may never become the condition of what the user
already has.**

### 5. Where the guarantees live

One owning place, as the documentation rule requires:

* [`vision.md`](../vision.md) carries the five guarantees, in the section about
  what the photographer owns. It is the document a user reads.
* [`specification.md`](../specification.md) §4 keeps the exclusions, restated as
  **scope decisions for the local application** rather than as principles about
  the project's future.
* [`pipeline.md`](../pipeline.md) §5.2 gains the execution backend, next to libm
  and the toolchain.

## Consequences

* **The GPU becomes an ordinary evolution**, costed like any other: a new stage
  version per operator ported, kept forever. The claim in `competitive-plan.md`
  §B3 that it is forbidden is corrected there.
* **A future service has a specification before it has a customer.** The
  conditions were written with nothing at stake, which is the only moment they
  can be written honestly.
* **`specification.md` §4's three reasons are rewritten.** Cloud, accounts and
  subscription stay out of the delivered scope; they stop being statements about
  what the project may ever do.
* **The project can no longer say "never any cloud" as a slogan.** That is the
  actual price of this ADR, and it is paid on purpose: a promise that will be
  broken the day it becomes expensive is worth less than a narrower one that
  holds.
* Nothing in the render changes. No stage version, no schema, no migration. This
  ADR moves words, and unblocks one that was standing in the way of code.

## Alternatives rejected

* **Promise "no cloud, ever", and keep it.** Simple, marketable, and a
  commitment the project cannot fund. Refusing to host is not the same as
  refusing that hosting should ever exist; the first is a fact about today, the
  second mortgages every tomorrow.
* **Say nothing, and decide when the question arises.** That is deciding under
  commercial pressure, which is when principles lose. It also leaves the GPU
  blocked by a reading nobody ever intended.
* **Relax §5.1 to "visually identical" so the GPU fits.** Rejected outright.
  §5.1 is the one guarantee the project makes that comparable engines do not;
  weakening it to accommodate an optimisation would trade the reason to exist
  for a speed-up.
* **Allow a GPU path with a silent CPU fallback.** The convenient answer, and
  the one that produces two different images from the same revision without ever
  telling the user. Refusing to render is worse ergonomics and better honesty,
  and it is the posture already chosen for unknown stages and mixed working
  spaces.
* **Record the backend in the revision, next to the stage map.** Considered,
  then dropped: it makes the revision carry a fact about an execution rather
  than about an intention (`catalog.md` §17). Making the backend part of the
  stage version's identity achieves the same guarantee with a mechanism that
  already exists.

## What this ADR does not do

* It does not create a service, plan one, or commit to one. It says what one
  would have to satisfy if it were ever built.
* It does not authorise a GPU implementation. It removes a false obstacle and
  prices the real one; the port itself would need its own ADR, per operator
  ported.
* It does not reopen [ADR 0009](0009-gpl3-cla-dual-license.md) (dual licence) or
  [ADR 0069](0069-closed-extension-boundary.md) (closed extensions). It states
  the rule those two already followed.
