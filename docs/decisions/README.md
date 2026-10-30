# ADR-0001. Decision records

- Date: 2026-10-30
- Status: accepted

## Context

starmap needed one place for the reasons behind its choices, apart from the
docs and rustdoc that say what it does now.

## Decision

- A decision record is a dated record of one decision: its context, the
  decision, its consequences and the alternatives considered. A record
  starts with `Date` and `Status` lines, and a record that replaces or
  amends an earlier one adds a `Supersedes` line that names it.
- A record is frozen once it has a `Date` line on `main`. A later decision
  gets a new record, and the earlier record's `Status` line changes to
  `superseded by ADR-NNNN` or `amended by ADR-NNNN`. The Status line and a
  node's `## Children` list are the only parts of a frozen record that may
  change.
- Records form a tree. Every directory's `README.md` is a node record that
  states which decisions belong under it and lists its children, so every
  record can be reached from this one.
- A record's number is global and never reused, and moving a record keeps
  its number. Text anywhere in the repository cites a record as
  `ADR-NNNN`, so one search finds every reference to a decision.

## Consequences

Changing a decision costs a new record, which keeps the old decision
readable. A reader finds current behaviour in the docs and rustdoc, which
cite records only for their reasons.

Considered: a flat directory, numbers local to each directory, editing
records in place.

## Children

- [ADR-0002. Project](project/README.md)
