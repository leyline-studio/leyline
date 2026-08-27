# ADR 0068 — An export batch processes several photos at once

**Status:** Accepted — 2026-08

## Context

[Survey B2](../measured-findings.md) timed the export path photo by photo, and
drew a lead from it: the fast encoders (JPEG, PNG, TIFF, WebP) are
single-threaded, so *"overlapping file n's encoding with n+1's rendering"* would
recover "on the order of 10 to 15 %" of a batch.

Measuring the batch itself gives an altogether different order of magnitude.
Twelve 30 Mpx files from one folder of the corpus, a neutral revision, JPEG q90
output, an i9-9900K with 16 threads:

```
sequential batch (today): 30.02 s — 77.5 s CPU, that is 280 % of 1,600 %
```

**The batch does not use 3 cores out of 16.** The problem is therefore not that
encoding leaves cores idle during encoding: it is that **one** photo's
pipeline, LibRaw decoding included, cannot fill the machine — not during
decoding, not during encoding, and barely during rendering. Overlapping two
stages of one file attacks only a fraction of what stays empty.

The same batch, processing several photos at once, measures what there really
is to take — figures from the implementation decided here:

| Photos in flight | Time | Speed-up | CPU | Peak RSS |
|---|---|---|---|---|
| 1 — the old behaviour | 30.05 s | 1.00× | 289 % | 845 MB |
| 2 | 16.84 s | 1.78× | 545 % | 1.45 GB |
| **4 — the default retained** | **10.68 s** | **2.81×** | 916 % | 2.38 GB |
| 6 | 8.81 s | 3.41× | 1,136 % | 3.27 GB |
| 8 | 9.05 s | 3.32× | 1,134 % | 4.69 GB |

The gain plateaus around 6, and **regresses** at 8: beyond that, one photo's
pipeline no longer knows what to do with the extra cores, and memory begins to
cost. **So it is not 10 to 15 %, but a factor of 2.8 to 3.4.**

### What a photo in flight costs

That is the constraint that decides everything else. The "Peak RSS" column above
grows linearly: ~600 MB per additional photo at 30 Mpx, and a loaded revision
(exposure, clarity, denoising) measures 785 MB where a neutral one takes 665. At
45 Mpx that exceeds a gigabyte per photo. **Parallelism is paid for in memory,
linearly**, and a machine that starts paging will lose far more than the 2.8
factor gained.

## Decision

**`Library::export` processes several photos in parallel, with a bounded and
adjustable number of photos in flight.**

### 1. The degree: 4 by default

`min(4, available_parallelism())`.

4 takes **2.81× of the 3.41× available** — 82 % of the gain — for a 2.4 GB peak
at 30 Mpx. 6 would take 3.41× but for 3.3 GB, and 8 already does *worse* than 6
for 4.7 GB. On a machine that edits photos, memory is not free: the default
takes the plain share of the gain and leaves the machine usable.

Adjustable through `ExportRequest.concurrency: Option<usize>`, because the right
number depends on the machine and on the files' size — two things the engine
cannot guess. That field belongs to the **request**, not to `ExportSettings`: it
is a property of the execution, not of the recipe, and it therefore has no
business in a preset's `settings_json`
([ADR 0067](0067-avif-encode-speed.md) §1 settled the opposite for AVIF speed,
which really is a property of the file produced).

### 2. Output names are reserved in advance, in request order

The batch today guarantees two things a concurrent execution would silently
break:

* an export **never overwrites** an existing file;
* two versions of the same asset in a batch contend for the same name, and it
  is **the second** that fails.

With concurrent renders, one's `exists()` and the other's write interleave: two
photos can pass the test and then overwrite each other. That is not a
scheduling detail, it is the loss of a file.

Hence a **prior planning pass**, sequential and in request order, which reads
the catalog once for the whole batch and assigns its output name to every
version. A name already reserved by an *earlier* version fails the later one
immediately, before any decoding. The documented behaviour is therefore kept
**and made deterministic**, where it previously depended on the order of
arrival on disk.

That pass has a second effect: the catalog lock is taken **once** for the whole
batch instead of once per photo, and not at all during the renders.

### 3. The batch's threads are not rayon tasks

`docs/engine-api.md` §3.1 already settled the question for the job pool:
rayon's work stealing can recruit one job's thread to run another, and if the
first holds the catalog mutex — which is not reentrant — that is a deadlock. The
same rule applies here. The photos in flight are carried by ordinary threads
(`std::thread::scope`); **rendering goes on using the global rayon pool inside
each photo**, which is precisely why 4 photos suffice to fill 16 cores.

[ADR 0024](0024-catalog-lock-narrowing-export.md)'s locking discipline is not
merely preserved, it becomes necessary: no worker holds the catalog while it
renders.

### 4. What concurrency does not change

* **The pixels.** Each photo is rendered independently; nothing is shared
  between two renders. `pipeline.md` §5.1 is intact, and a test verifies it
  rather than asserting it: the same batch, sequentially and then concurrently,
  produces files **identical byte for byte**. That test was not writable before
  the ICC timestamp fix — the embedded profile carried the time, so two
  identical exports already differed by a byte without anyone knowing.
* **The report's order.** `ExportReport.exported` and `.failed` stay in request
  order, whatever the order of completion.
* **Progress.** `progress(done, total)` counts the files written; only the
  order in which they finish changes, which no client observes.
* **Fault tolerance.** A photo that fails does not bring the batch down, as
  before.
* **`export_batch`**, the free function, stays sequential: it takes a
  `&mut Catalog`, hence exclusive access, and by construction has nothing to
  parallelize. It is the path the engine's tests use directly; all three
  clients go through `Library::export`.

## Consequences

* An export batch goes **~2.8× faster** without any setting changing, and ~3.4×
  for whoever raises the degree to 6.
* `--concurrency <n>` in the CLI; Studio and the SDK take the default, Studio
  having no reason to know more than the engine about the machine.
* An export's peak memory is multiplied by the degree. That is the reason for
  the cautious default, and the first thing to look at if a batch becomes slow
  instead of getting faster.
* The `export_history` journal is now written in completion order, no longer in
  request order. No reading depends on it: the rows carry their timestamp.
* The planning pass holds the whole batch in memory (one parsed `Settings` per
  version). Negligible beside a single image, but it is what forbids applying
  the same recipe to a batch of several hundred thousand photos without
  splitting it.

## Alternatives rejected

* **Overlapping file *n*'s encoding with *n+1*'s rendering**, B2's original
  lead. It aims at the right symptom but far too small: the measurement shows
  13 cores out of 16 missing, not merely those of a single-threaded encoding.
  It is also more intrusive — `render_export` has to be split into two halves
  made to communicate — for a fraction of the gain processing several photos
  gives.
* **Parallelizing with `rayon::par_iter` over the versions.** The obvious move,
  and the deadlock described in §3: it is the reason the job pool is not one
  either.
* **A degree equal to `available_parallelism()`.** On the reference machine, 8
  photos in flight already go *slower* than 6 for 1.4 GB more — and on a
  32-thread machine, enough to page with 45 Mpx files.
* **Adapting the degree to the images' size.** An unwritten heuristic, of the
  kind [ADR 0067](0067-avif-encode-speed.md) rejected: §1's field gives the
  decision back to whoever knows their machine.
