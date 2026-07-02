//! Op-sequence executor: a bounded slot-table interpreter over the C shim.
//!
//! Single-op inputs cannot reach adjacent-block metadata clobber or realloc
//! chains: those defects require several live allocations interacting. This
//! mode decodes a sequence of operations from the input byte stream against a
//! table of at most [`MAX_SLOTS`] live allocations and asserts value-semantic
//! oracles between operations.
//!
//! # Byte grammar
//!
//! Each operation starts with one opcode byte: bits 0-2 select the operation,
//! bits 3-5 select the slot hint (bits 6-7 are ignored). Operand bytes follow
//! the opcode; a truncated operand ends the sequence cleanly.
//!
//! | op | operation      | operands                    |
//! |----|----------------|-----------------------------|
//! | 0  | `malloc`       | 2 bytes size (LE)           |
//! | 1  | `calloc`       | 1 byte nmemb, 2 bytes size  |
//! | 2  | `aligned_alloc`| 1 byte align, 2 bytes size  |
//! | 3  | `realloc`      | 2 bytes new size (LE)       |
//! | 4  | `free`         | none                        |
//! | 5  | write pattern  | 1 byte seed                 |
//! | 6  | verify pattern | none                        |
//! | 7  | usable query   | none                        |
//!
//! Alloc-family ops take the first free slot at or after the hint
//! (wrapping); pointer ops take the first live slot the same way, so
//! arbitrary bytes rarely waste an opcode.
//!
//! # Oracles (value-semantic)
//!
//! - write/verify: every byte written to a live slot (up to its recorded
//!   usable size) must read back unchanged across arbitrary interleaved
//!   operations on other slots — the adjacent-block clobber oracle.
//! - realloc: the shim contract (`realloc` Rustdoc) preserves
//!   `min(old usable, new_size)` bytes; since `written <= old usable`, the
//!   first `min(written, new_size)` pattern bytes must survive every hop of a
//!   realloc chain. `realloc(p, 0)` frees and returns null.
//! - calloc: the full requested payload must read as zero.
//! - usable size: stable for a live block, at least the requested size, and
//!   zero for null.
//!
//! # Resource bounds (panic-free-by-design on arbitrary input)
//!
//! - Per-op size is shaped through [`sequence_size`], capped at
//!   `MAX_FUZZ_ALLOC` (64 KiB).
//! - Total live requested bytes are capped at [`MAX_LIVE_BYTES`]; an op that
//!   would exceed the cap is skipped.
//! - At most [`MAX_OPS`] operations per input.
//! - All decode failures (short input) end the run cleanly; no indexing or
//!   arithmetic can overflow (all quantities are bounded by the caps above).
//! - Every live slot is freed when the run ends ([`SlotTable`]'s `Drop`), so
//!   no memory leaks across fuzz iterations.

use core::ffi::c_void;

use crate::c_shim_api::{malloc_request, pattern, shaped_size, MAX_FUZZ_ALLOC};

/// Maximum simultaneously live allocations.
pub(crate) const MAX_SLOTS: usize = 8;
/// Cap on the sum of live requested payload bytes (256 KiB).
pub(crate) const MAX_LIVE_BYTES: usize = 4 * MAX_FUZZ_ALLOC;
/// Cap on decoded operations per input.
pub(crate) const MAX_OPS: usize = 1024;

/// One live allocation and the state its oracles depend on.
struct Slot {
    ptr: *mut u8,
    /// Requested payload bytes (after `malloc_request` shaping); the
    /// lower bound every usable-size oracle checks against.
    request: usize,
    /// `malloc_usable_size` recorded at (re)allocation time.
    usable: usize,
    /// Pattern bytes currently guaranteed present at the front of the block.
    written: usize,
    /// Seed of the pattern the `written` prefix was filled with.
    seed: u8,
}

/// Bounded table of live slots; owns the allocations it holds.
struct SlotTable {
    slots: [Option<Slot>; MAX_SLOTS],
    /// Sum of `request` over live slots; enforces [`MAX_LIVE_BYTES`].
    live_requested: usize,
}

impl SlotTable {
    const fn new() -> Self {
        Self {
            slots: [const { None }; MAX_SLOTS],
            live_requested: 0,
        }
    }

    /// First free slot index at or after `hint`, wrapping.
    fn scan_free(&self, hint: usize) -> Option<usize> {
        (0..MAX_SLOTS)
            .map(|offset| (hint + offset) % MAX_SLOTS)
            .find(|&index| self.slots[index].is_none())
    }

    /// First live slot index at or after `hint`, wrapping.
    fn scan_live(&self, hint: usize) -> Option<usize> {
        (0..MAX_SLOTS)
            .map(|offset| (hint + offset) % MAX_SLOTS)
            .find(|&index| self.slots[index].is_some())
    }

    fn insert(&mut self, index: usize, slot: Slot) {
        self.live_requested += slot.request;
        self.slots[index] = Some(slot);
    }

    /// Removes and returns the slot at `index`, updating the live-byte total.
    fn take(&mut self, index: usize) -> Slot {
        let slot = self.slots[index]
            .take()
            .expect("invariant: take is only called on an index scan_live returned");
        self.live_requested -= slot.request;
        slot
    }
}

impl Drop for SlotTable {
    /// Frees every live allocation: no leaks across fuzz iterations, even
    /// when a run ends mid-sequence or an oracle assertion unwinds in tests.
    fn drop(&mut self) {
        for entry in &mut self.slots {
            if let Some(slot) = entry.take() {
                // Safety: the table owns `slot.ptr`; it is a live allocation
                // from this shim, freed exactly once here.
                unsafe { mnemosyne_c_shim::free(slot.ptr.cast::<c_void>()) };
            }
        }
    }
}

/// Forward-only reader over the input byte stream.
struct Cursor<'a> {
    data: &'a [u8],
}

impl Cursor<'_> {
    fn next_byte(&mut self) -> Option<u8> {
        let (&byte, rest) = self.data.split_first()?;
        self.data = rest;
        Some(byte)
    }

    /// Two bytes, little-endian.
    fn next_word(&mut self) -> Option<u16> {
        let lo = self.next_byte()?;
        let hi = self.next_byte()?;
        Some(u16::from_le_bytes([lo, hi]))
    }
}

/// Runs the op-sequence mode over `data` (the payload after the mode byte).
pub(crate) fn run_sequence(data: &[u8]) {
    let mut cursor = Cursor { data };
    let mut table = SlotTable::new();

    for _ in 0..MAX_OPS {
        let Some(opcode) = cursor.next_byte() else {
            break;
        };
        let hint = usize::from((opcode >> 3) & 0x07);
        let more = match opcode & 0x07 {
            0 => op_malloc(&mut table, hint, &mut cursor),
            1 => op_calloc(&mut table, hint, &mut cursor),
            2 => op_aligned_alloc(&mut table, hint, &mut cursor),
            3 => op_realloc(&mut table, hint, &mut cursor),
            4 => op_free(&mut table, hint),
            5 => op_write(&mut table, hint, &mut cursor),
            6 => op_verify(&table, hint),
            _ => op_usable_query(&table, hint),
        };
        if !more {
            break;
        }
    }
    // `table` drops here, freeing all remaining live slots.
}

/// Shapes a two-byte raw operand into a per-op-bounded size.
fn sequence_size(raw: u16) -> usize {
    shaped_size(usize::from(raw)).min(MAX_FUZZ_ALLOC)
}

fn usable_of(ptr: *mut u8) -> usize {
    // Safety: callers pass a live shim allocation (or the shim tolerates
    // null, returning 0).
    unsafe { mnemosyne_c_shim::malloc_usable_size(ptr.cast::<c_void>()) }
}

/// `malloc` into the first free slot at/after `hint`. Returns `false` when
/// operand bytes are exhausted.
fn op_malloc(table: &mut SlotTable, hint: usize, cursor: &mut Cursor<'_>) -> bool {
    let Some(raw) = cursor.next_word() else {
        return false;
    };
    let request = malloc_request(sequence_size(raw));
    let Some(index) = table.scan_free(hint) else {
        return true;
    };
    if table.live_requested + request > MAX_LIVE_BYTES {
        return true;
    }
    // Safety: plain allocation request; null is handled below.
    let ptr = unsafe { mnemosyne_c_shim::malloc(request) }.cast::<u8>();
    if ptr.is_null() {
        return true;
    }
    let usable = usable_of(ptr);
    assert!(
        usable >= request,
        "usable size {usable} under-reports request {request}"
    );
    table.insert(
        index,
        Slot {
            ptr,
            request,
            usable,
            written: 0,
            seed: 0,
        },
    );
    true
}

/// `calloc` into a free slot; asserts the full payload reads as zero.
fn op_calloc(table: &mut SlotTable, hint: usize, cursor: &mut Cursor<'_>) -> bool {
    let Some(nmemb_byte) = cursor.next_byte() else {
        return false;
    };
    let Some(raw) = cursor.next_word() else {
        return false;
    };
    let nmemb = usize::from(nmemb_byte & 0x07) + 1; // 1..=8
    let size = sequence_size(raw).min(MAX_FUZZ_ALLOC / nmemb);
    let total = nmemb * size; // <= MAX_FUZZ_ALLOC by construction
    let request = malloc_request(total);
    let Some(index) = table.scan_free(hint) else {
        return true;
    };
    if table.live_requested + request > MAX_LIVE_BYTES {
        return true;
    }
    // Safety: nmemb * size cannot overflow (bounded above); null handled below.
    let ptr = unsafe { mnemosyne_c_shim::calloc(nmemb, size) }.cast::<u8>();
    if ptr.is_null() {
        return true;
    }
    let usable = usable_of(ptr);
    assert!(
        usable >= request,
        "usable size {usable} under-reports calloc request {request}"
    );
    for offset in 0..total {
        // Safety: offset < total <= usable, inside the live allocation.
        let byte = unsafe { ptr.add(offset).read() };
        assert_eq!(byte, 0, "calloc byte {offset} was not zero-initialized");
    }
    table.insert(
        index,
        Slot {
            ptr,
            request,
            usable,
            written: 0,
            seed: 0,
        },
    );
    true
}

/// `aligned_alloc` (always contract-valid: power-of-two alignment 16..=4096,
/// size a nonzero multiple of it) into a free slot; asserts alignment.
fn op_aligned_alloc(table: &mut SlotTable, hint: usize, cursor: &mut Cursor<'_>) -> bool {
    let Some(align_byte) = cursor.next_byte() else {
        return false;
    };
    let Some(raw) = cursor.next_word() else {
        return false;
    };
    let alignment = 1_usize << ((usize::from(align_byte) % 9) + 4); // 16..=4096
    let shaped = sequence_size(raw);
    let rounded_down = shaped - (shaped % alignment);
    let size = if rounded_down == 0 {
        alignment
    } else {
        rounded_down
    };
    let Some(index) = table.scan_free(hint) else {
        return true;
    };
    if table.live_requested + size > MAX_LIVE_BYTES {
        return true;
    }
    // Safety: contract-valid request by construction; null (OOM) handled below.
    let ptr = unsafe { mnemosyne_c_shim::aligned_alloc(alignment, size) }.cast::<u8>();
    if ptr.is_null() {
        return true;
    }
    assert_eq!(
        ptr as usize % alignment,
        0,
        "aligned_alloc returned a misaligned pointer"
    );
    let usable = usable_of(ptr);
    assert!(
        usable >= size,
        "usable size {usable} under-reports aligned request {size}"
    );
    table.insert(
        index,
        Slot {
            ptr,
            request: size,
            usable,
            written: 0,
            seed: 0,
        },
    );
    true
}

/// `realloc` of a live slot; asserts the shim's preservation contract
/// (`min(written, new_size)` pattern bytes survive) and the
/// `realloc(_, 0)` free-and-null contract.
fn op_realloc(table: &mut SlotTable, hint: usize, cursor: &mut Cursor<'_>) -> bool {
    let Some(raw) = cursor.next_word() else {
        return false;
    };
    let Some(index) = table.scan_live(hint) else {
        return true;
    };
    let new_size = sequence_size(raw);

    if new_size == 0 {
        let slot = table.take(index);
        // Safety: slot.ptr is live; realloc(p, 0) frees it per the shim
        // contract, so ownership transfers here.
        let out = unsafe { mnemosyne_c_shim::realloc(slot.ptr.cast::<c_void>(), 0) };
        assert!(out.is_null(), "realloc(_, 0) must return null");
        return true;
    }

    let mut slot = table.take(index);
    if table.live_requested + new_size > MAX_LIVE_BYTES {
        table.insert(index, slot);
        return true;
    }
    // Safety: slot.ptr is live; on success realloc owns/frees the old block,
    // on failure the old block stays live and is re-inserted below.
    let out =
        unsafe { mnemosyne_c_shim::realloc(slot.ptr.cast::<c_void>(), new_size) }.cast::<u8>();
    if out.is_null() {
        table.insert(index, slot);
        return true;
    }

    // Shim contract: min(old usable, new_size) bytes are preserved and
    // written <= old usable, so the first min(written, new_size) pattern
    // bytes must survive.
    let preserved = slot.written.min(new_size);
    for offset in 0..preserved {
        // Safety: offset < preserved <= new_size <= new usable.
        let byte = unsafe { out.add(offset).read() };
        assert_eq!(
            byte,
            pattern(slot.seed, offset),
            "realloc did not preserve initialized byte {offset}"
        );
    }
    let usable = usable_of(out);
    assert!(
        usable >= new_size,
        "usable size {usable} under-reports realloc request {new_size}"
    );
    slot.ptr = out;
    slot.request = new_size;
    slot.usable = usable;
    slot.written = preserved;
    table.insert(index, slot);
    true
}

/// Frees a live slot (no-op when none is live).
fn op_free(table: &mut SlotTable, hint: usize) -> bool {
    if let Some(index) = table.scan_live(hint) {
        let slot = table.take(index);
        // Safety: slot.ptr is a live allocation owned by the table.
        unsafe { mnemosyne_c_shim::free(slot.ptr.cast::<c_void>()) };
    }
    true
}

/// Fills a live slot's full recorded usable span with a seeded pattern.
fn op_write(table: &mut SlotTable, hint: usize, cursor: &mut Cursor<'_>) -> bool {
    let Some(seed) = cursor.next_byte() else {
        return false;
    };
    let Some(index) = table.scan_live(hint) else {
        return true;
    };
    let slot = table.slots[index]
        .as_mut()
        .expect("invariant: scan_live returned a live index");
    for offset in 0..slot.usable {
        // Safety: offset < usable, the span the shim reports writable.
        unsafe { slot.ptr.add(offset).write(pattern(seed, offset)) };
    }
    slot.written = slot.usable;
    slot.seed = seed;
    true
}

/// Adjacent-block clobber oracle: every previously written byte of a live
/// slot must read back unchanged.
fn op_verify(table: &SlotTable, hint: usize) -> bool {
    let Some(index) = table.scan_live(hint) else {
        return true;
    };
    let slot = table.slots[index]
        .as_ref()
        .expect("invariant: scan_live returned a live index");
    for offset in 0..slot.written {
        // Safety: offset < written <= usable, inside the live allocation.
        let byte = unsafe { slot.ptr.add(offset).read() };
        assert_eq!(
            byte,
            pattern(slot.seed, offset),
            "live allocation byte {offset} was clobbered"
        );
    }
    true
}

/// Usable-size invariants: zero for null; stable and at least the request
/// for a live slot.
fn op_usable_query(table: &SlotTable, hint: usize) -> bool {
    assert_eq!(
        usable_of(core::ptr::null_mut()),
        0,
        "malloc_usable_size(NULL) must be 0"
    );
    let Some(index) = table.scan_live(hint) else {
        return true;
    };
    let slot = table.slots[index]
        .as_ref()
        .expect("invariant: scan_live returned a live index");
    let usable = usable_of(slot.ptr);
    assert_eq!(
        usable, slot.usable,
        "usable size changed for a live allocation"
    );
    assert!(
        usable >= slot.request,
        "usable size {usable} under-reports request {}",
        slot.request
    );
    true
}

#[cfg(test)]
mod tests {
    use super::run_sequence;

    /// Encodes an opcode byte: bits 0-2 operation, bits 3-5 slot hint.
    fn opcode(op: u8, slot: u8) -> u8 {
        (slot << 3) | op
    }

    /// Encodes a two-byte little-endian size operand. The values used in
    /// these tests all have low nibble 0xF, which `shaped_size` maps to
    /// `raw % (MAX_FUZZ_ALLOC + 1)` — i.e. the literal value for raw < 64 KiB
    /// — so the requested sizes below are exact.
    fn word(value: u16) -> [u8; 2] {
        value.to_le_bytes()
    }

    #[test]
    fn realloc_chain_preserves_written_prefix() {
        let mut ops = Vec::new();
        ops.push(opcode(0, 0)); // malloc slot 0
        ops.extend(word(31)); // request 31 bytes
        ops.extend([opcode(5, 0), 0xA7]); // write pattern seed 0xA7
        ops.push(opcode(3, 0)); // realloc grow
        ops.extend(word(1023)); // -> 1023 bytes (oracle: prefix preserved)
        ops.push(opcode(6, 0)); // verify surviving pattern
        ops.push(opcode(3, 0)); // realloc shrink
        ops.extend(word(15)); // -> 15 bytes (oracle: 15-byte prefix preserved)
        ops.push(opcode(6, 0)); // verify surviving pattern
        ops.push(opcode(3, 0)); // realloc to 0: free + null contract
        ops.extend(word(0));
        run_sequence(&ops);
    }

    #[test]
    fn adjacent_block_pattern_survives_neighbor_free_and_reuse() {
        let mut ops = Vec::new();
        // Two same-size-class neighbors.
        ops.push(opcode(0, 0));
        ops.extend(word(255));
        ops.push(opcode(0, 1));
        ops.extend(word(255));
        // Distinct patterns in each.
        ops.extend([opcode(5, 0), 0x11]);
        ops.extend([opcode(5, 1), 0x22]);
        ops.push(opcode(6, 0));
        ops.push(opcode(6, 1));
        // Free slot 0: slot 1's bytes must survive the neighbor's free path
        // (metadata updates must stay inside the freed block).
        ops.push(opcode(4, 0));
        ops.push(opcode(6, 1));
        // Reuse the freed region and write through it; slot 1 still intact.
        ops.push(opcode(0, 0));
        ops.extend(word(31));
        ops.extend([opcode(5, 0), 0x33]);
        ops.push(opcode(6, 1));
        ops.push(opcode(6, 0));
        ops.push(opcode(4, 1));
        ops.push(opcode(4, 0));
        run_sequence(&ops);
    }

    #[test]
    fn calloc_zero_oracle_and_drop_frees_trailing_live_slots() {
        let mut ops = Vec::new();
        // calloc nmemb = (3 & 7) + 1 = 4, size = 255 -> 1020 zeroed bytes.
        ops.extend([opcode(1, 2), 0x03]);
        ops.extend(word(255));
        ops.extend([opcode(5, 2), 0x5A]);
        // Input ends with the slot live: SlotTable::drop must free it.
        run_sequence(&ops);
    }

    #[test]
    fn aligned_alloc_and_usable_query_invariants() {
        let mut ops = Vec::new();
        // align_byte 4 -> alignment 1 << (4 % 9 + 4) = 256; size 1023 rounds
        // down to 768 (a multiple of 256).
        ops.extend([opcode(2, 3), 4]);
        ops.extend(word(1023));
        ops.push(opcode(7, 3)); // usable query: stable, >= request, null -> 0
        ops.push(opcode(4, 3));
        run_sequence(&ops);
    }

    #[test]
    fn truncated_operands_end_the_sequence_cleanly() {
        run_sequence(&[]);
        run_sequence(&[opcode(0, 0)]); // malloc missing both size bytes
        run_sequence(&[opcode(3, 0), 0x10]); // realloc missing one size byte
        run_sequence(&[opcode(5, 0)]); // write missing the seed byte
    }

    #[test]
    fn arbitrary_byte_stream_executes_within_bounds() {
        // Deterministic pseudo-random stream: exercises every opcode family,
        // slot exhaustion, the live-byte budget, and end-of-input cleanup.
        let data: Vec<u8> = (0..4096_u32)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
            .collect();
        run_sequence(&data);
    }
}
