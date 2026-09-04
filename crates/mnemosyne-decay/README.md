# mnemosyne-decay

Background decay and reclamation for
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) arenas.

```toml
[dependencies]
mnemosyne-decay = "0.3"
```

Segments freed by the allocator are not returned to the operating system
immediately: holding them lets a later allocation of the same size class reuse
the mapping instead of paying another syscall. This crate owns the other side of
that trade — it periodically purges segments that have gone cold, so a burst of
allocation does not pin resident memory indefinitely.

`init_decay_engine` lazily spawns the worker thread, gated on the
`MNEMOSYNE_PURGE_CADENCE_MS` cadence; a cadence of zero disables decay entirely
and spawns no thread. `decay_step` performs one sweep across the active
segments and is callable directly when a caller wants to drive reclamation
without the background worker. `decay_step_generation` and
`wait_for_decay_step` provide a bounded event seam for callers that need to
observe completed background sweeps without polling. `request_decay_step`
coalesces an immediate wake for an active worker; it is useful when a caller
has just produced reclaimable work and does not need to wait for the next
periodic deadline.

Licensed under MIT OR Apache-2.0.
