//! Tests for the op-sequence interpreter: allocation/free bookkeeping
//! invariants and end-of-input cleanup.

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
