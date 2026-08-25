# Vision

This document answers one question: **why Leyline exists**. The technical breakdown is in [`architecture.md`](architecture.md), the functional scope in [`specification.md`](specification.md).

---

## Mission

Build an open source RAW development platform that is modern, fast and durable.

Leyline is not one more engine behind an interface: it is an engine around which several applications can be built — Studio, the CLI, an SDK, and whatever others make of it.

---

## The starting observation

A demanding amateur photographer has, today, essentially two options:

* **Adobe Lightroom** — capable and coherent, but conditional on a subscription: stop paying and you lose access to your own editing work.
* **The free alternatives** — often very powerful, sometimes hard to approach, and frequently heirs to older architectures that make every evolution expensive.

Leyline offers a third way: software that is fast, modern, local, cross-platform, pleasant to use, entirely non-destructive, and whose architecture is clean **from the start** — because no architecture is ever straightened out afterwards at a reasonable cost.

---

## Principles

Six principles, in this order of priority:

**Local First.** The software works entirely offline. The network serves optional, explicit purposes only: checking that an update exists, downloading it, possibly sharing a preset. No editing feature depends on a connection.

**Non-destructive.** A RAW file is **never** modified. Every correction is stored separately, as a pipeline of independent steps. The corollary is a strong promise: the same settings on the same RAW give the same pixels ten years from now — its exact scope is defined in [`pipeline.md`](pipeline.md) §5.

**Fast.** Performance is not an end-of-the-road optimisation; it governs the choice of language, the structure of the cache and the rendering model.

**Modular.** Every crate has a single responsibility, and the engine is unaware that a graphical interface even exists.

**Built to last.** Every structural decision is recorded in an ADR, with its context, its rejected alternatives and its consequences. A maintainer in 2036 must be able to reconstruct *why* a thing is the way it is, not merely observe that it is.

**Open Source.** The core is published under a free licence, so that the platform can keep evolving independently of any company — including its author's.

---

## What the photographer owns

**Their data.** No mandatory cloud, no subscription, no proprietary lock. Everything is stored locally: the catalog holds only references, metadata, settings, collections and indexes — never the photos themselves.

**Their RAW files.** The original file is treated as an archival item: read, never rewritten.

**Their editing work.** Settings are stored in a documented format ([`pipeline.md`](pipeline.md) §3.2), inside an ordinary SQLite database ([`catalog.md`](catalog.md)). Nothing is encrypted or obfuscated: a Leyline catalog stays readable even without Leyline.

---

## What Leyline is not

Leyline is not a clone of Lightroom, nor of darktable, nor of Capture One. Some of their solutions are taken up when they are good, and rejected with a written reason when they are not — see for instance [ADR 0042](adr/0042-versioned-stage-pipeline.md), which explicitly compares all three approaches to freezing a render before deciding.

Nor is it a project that accumulates features: the exclusions in [`specification.md`](specification.md) are decisions, not delays.

---

## Order of work

* Vision before architecture.
* Architecture before implementation.
* Documentation before code.
* API before graphical interface.
* Simplicity before feature accumulation.

> **No code before architecture. No architecture before vision.**

---

## Intended audience

**The passionate amateur** — a few thousand photos a year, wants to stop paying a subscription, looks for a simple, fast workflow.

**The expert amateur** — a large library, uses ratings, collections, keywords and batch processing.

**The developer** — uses the engine as a Rust SDK, or the CLI inside their own scripts. This audience is not a bonus: it is what justifies the engine being independent of the interface.

---

## Why "Leyline"

The name comes from *ley lines*, those theoretical lines connecting notable places. The image describes exactly how the software works inside: a photograph follows a path made of transformations, each operation linked to the next. Developing a photo becomes a journey.

Hence: **Leyline** for the platform, **Leyline Engine** for the engine, **Leyline Studio** for the application.

---

## Ambition

To build an open, fast and elegant photographic platform whose architecture is still maintainable in ten or twenty years, where every decision is documented, every technical choice justified, every module independent — and where photographers remain the owners of their images.

Leyline's first user is its creator. If the vision is shared by other photographers and developers, the project may become an open source reference for RAW development.
