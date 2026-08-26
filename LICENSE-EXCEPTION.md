# Additional permission under GNU GPL version 3 section 7

Leyline is licensed under the GNU General Public License, version 3, whose
text is reproduced verbatim and unmodified in [`LICENSE`](LICENSE). This file
does not change it. It records one **additional permission** granted under
section 7 of that licence, decided in
[ADR 0069](docs/adr/0069-closed-extension-boundary.md) §4.

## The permission

> **Additional permission under GNU GPL version 3 section 7**
>
> The copyright holders of Leyline give you permission to combine Leyline with
> software released under terms of your choice, and to convey the resulting
> work, provided that every part of Leyline itself remains governed by the GNU
> General Public License version 3 and is conveyed under those terms.
>
> This permission does not extend to modified versions of Leyline: if you
> modify Leyline, this additional permission does not apply to your modified
> version, and you may remove it.

## Why it exists

*This section explains the permission. It grants nothing and takes nothing
away: only the text above is operative.*

Leyline is designed to be extended from the outside rather than from within.
An extension is a **client of `leyline-sdk`**, exactly like Leyline Studio or
the command-line tool: it reads a decoded image, computes something, and
writes settings through an ordinary edit session. It is never a render stage,
it holds no stage version, and it appears nowhere in a revision's `stages`
map — the engine offers no plugin registry, no dynamic loading and no callback
invoked during a render, so there is no channel through which closed code
could reach the render path.

Linking a proprietary program to `leyline-sdk` nevertheless forms a combined
work that the GPLv3 governs. The copyright holders can authorise it, and this
file is that authorisation written down — because a public repository saying
one thing while a shipped binary does another would be worse than either.

What the permission deliberately does **not** do:

- It does not relicense any part of Leyline. Every file of Leyline remains
  GPL-3.0, and anything conveyed with a combined work must still be conveyed
  under those terms.
- It does not survive modification. Someone who modifies Leyline is granted
  nothing by this file, and may remove it from their fork.
- It does not create a private render path. The boundary that makes this safe
  is architectural, not contractual: an extension produces **settings, never
  pixels**, so a photo edited with one opens, renders and exports identically
  on a build without it. See ADR 0069 §1.

## What is not decided here

Nothing about a paid edition: key verification, activation, or which features
would be sold. ADR 0069 §5 leaves that open on purpose, and names two
constraints for the day it is settled — any verification must work **offline**,
and [`docs/specification.md`](docs/specification.md) §4 must be corrected
rather than worked around.
