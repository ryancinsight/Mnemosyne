# Decay, Purge, and Reset

Mnemosyne provides three mechanisms for reclaiming OS-mapped memory.

## `decay()`

Triggers one manual decay sweep across all registered backends. Drains the
orphan pool and consolidates free segments.

```rust,ignore
mnemosyne::decay();
```

## `reset()`

Drops the physical backing of retained free segments without unmapping virtual
addresses. The virtual address space stays warm (avoiding future re-fault cost),
but physical pages are returned to the OS. Lighter than `purge`.

```rust,ignore
mnemosyne::reset();
```

## `purge()`

Releases all retained free segments to the OS, unmapping their virtual address
ranges. Use after a large allocation spike to recover physical memory.

```rust,ignore
mnemosyne::purge();
```

## Background Decay Engine

When `purge_cadence_ms > 0` in `MnemosyneOptions`, Mnemosyne spawns a
background thread (`mnemosyne-decay`) that runs `decay_step()` on the
configured interval. The decay engine uses an AcqRel RMW protocol to avoid
lost-wakeup races during concurrent shutdown and cadence reconfiguration.

```rust,ignore
mnemosyne::configure(MnemosyneOptions {
    purge_cadence_ms: 5000,  // decay every 5 seconds
    ..Default::default()
});
```
