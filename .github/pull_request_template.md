<!--
The body of a commit — and of this description — says *why*. The diff already
says what.
-->

## What this changes, and why

## Decision record

<!--
Does this change a decision (pipeline order, what a stage does to pixels, where
data lives, what the application refuses to be)? Then it needs an ADR in
docs/adr/, in the same change. Link it here, or say why none is needed.
-->

- [ ] No decision changes here, or the ADR is included: <!-- ADR number -->

## Checks

- [ ] `make check` passes (rustfmt, clippy with zero warnings, the full test suite)
- [ ] Tests cover the change
- [ ] Public APIs are documented
- [ ] No published stage version had its rendering changed — a changed rendering
      is a *new* version next to the old one (`docs/pipeline.md` §5.1)
- [ ] I accept the [CLA](../CLA.md)
