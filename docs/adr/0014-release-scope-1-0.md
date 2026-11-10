# ADR 0014: Mnemosyne release scope for 1.0

- Status: Accepted
- Date: 2026-11-10
- Class: `[arch]` `[minor]`

## Context

Mnemosyne (`mnemosyne-memory`, workspace packages 0.10.x) is a high-performance
user-space memory allocator for the Atlas stack, providing global allocation,
policies, heaps, backends, and memory management primitives. The backlog
contains open correctness and verification items, and no milestone ladder
exists to 1.0.

## Decision

Adopt a three-milestone ladder to 1.0 recorded in `backlog/milestones.md`:

- **M-core-allocation** (0.11.0): users allocate and free memory through the
  global allocator and core heap/arena primitives with verified behavior.
- **M-policies-backends** (0.12.0): users apply allocation policies, backends,
  and supporting components (profiling/scratch/hardening as applicable) with
  verified contracts.
- **M-1.0** (1.0.0): stable allocator surface (authority-gated) with
  documented contracts and release gates satisfied.

## Alternatives

- Plan 1.0 as a single release: rejected because open correctness and
  verification work remains.
- Keep roadmap implicit in backlog items: rejected because stack policy
  requires a runnable capability-oriented milestone ladder.

## Consequences

- Open items carry `milestone:` assignments and feature items name the exit
  check they serve.
- Milestones close only when exit checks pass on fetched default commits; 1.0
  release/publication remains authority-gated.
- This ADR is rewritten in place as scope evidence changes.
