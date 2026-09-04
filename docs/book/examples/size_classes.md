# Example: Size Classes

**Crate**: `mnemosyne`
**Source**: `crates/mnemosyne/examples/book_size_classes.rs`

This example exercises the canonical `mnemosyne-core` size-class mapper at
small-class boundaries and verifies the rounded block size and page-density
calculation used by the allocator.

## Source

```rust
{{#include ../../../crates/mnemosyne/examples/book_size_classes.rs}}
```

## Output

```text
small ceiling: 16384 B
size classes: 52
request=16 B -> class=0, block=16 B, blocks/page=4096
request=128 B -> class=7, block=128 B, blocks/page=512
request=512 B -> class=19, block=512 B, blocks/page=128
request=2048 B -> class=31, block=2048 B, blocks/page=32
request=8192 B -> class=43, block=8192 B, blocks/page=8
request=8193 B -> class=44, block=9216 B, blocks/page=7
request=16384 B -> class=51, block=16384 B, blocks/page=4
above ceiling: large/huge route
all size-class assertions passed
```

## What to notice

- The example calls `size_to_class`, `round_up_size`, `class_to_size`, and
  `class_to_max_blocks` directly. The checks therefore exercise the same
  const-generated mapping and class table used by allocator routing.
- The 8193-B request takes the first 1024-byte high-band class, while the
  16384-B request remains small. The next byte is an explicit large/huge route
  signal rather than an implicit table miss.
- The page-density assertion makes the memory tradeoff visible: a larger class
  reduces the number of blocks that fit in a 64 KiB page, while the fixed class
  table bounds internal rounding slack.
