# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-11-21

## Context

Rustflare reuses ideas and code from another project, targets specific hardware, and is meant
to grow to more cameras and mounts. The reasons behind early choices will be forgotten unless written down.

## Decision

Significant decisions are recorded as short ADRs in `docs/adr/`, numbered sequentially, in
Markdown, versioned with the code. Documentation lives in `docs/` as Markdown and is
renderable with MkDocs (`mkdocs.yml`) but readable as plain files.

## Consequences

Small overhead per decision; new contributors can read the history. Records are append-only.
