use core::hash::{BuildHasher, Hasher};

#[derive(Default, Clone, Copy)]
pub(super) struct FastHasher(u64);

impl Hasher for FastHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.wrapping_mul(109) ^ (byte as u64);
        }
    }

    #[inline(always)]
    fn write_u8(&mut self, i: u8) {
        self.0 = self.0.wrapping_mul(109) ^ (i as u64);
    }

    #[inline(always)]
    fn write_usize(&mut self, i: usize) {
        // Construction: fmix64 (the MurmurHash3/SplitMix64 finalizer) gives a
        // full per-word avalanche, then a rotate-xor-multiply step chains the
        // mixed word into the accumulated state order-sensitively. Replacing
        // the state (`self.0 = x`) instead of chaining would make the slice
        // hash depend only on the *last* word written — and captured stacks
        // are innermost→outermost, so every stack would collapse onto the
        // shared thread-root frame, degenerating the interner map into one
        // collision chain.
        let mut x = i as u64;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58476d1ce4e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d049bb133111eb);
        x ^= x >> 31;
        self.0 = (self.0.rotate_left(29) ^ x).wrapping_mul(0x9e3779b97f4a7c15);
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct FastBuildHasher;

impl BuildHasher for FastBuildHasher {
    type Hasher = FastHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::BuildHasher;

    /// Hashes `words` exactly as the interner's `HashMap<Arc<[usize]>, _>` key
    /// does: `Hash for [usize]` emits the length prefix plus one
    /// `write_usize` per word.
    fn slice_hash(words: &[usize]) -> u64 {
        FastBuildHasher.hash_one(words)
    }

    #[test]
    fn fast_hasher_depends_on_every_word_position() {
        // Two-frame "stacks" sharing the last word model the real degenerate
        // case: distinct call sites all ending in the common thread root.
        let a = 0x7ff6_0000_1000_usize;
        let b = 0x7ff6_0000_2000_usize;
        let c = 0x7ff6_0000_3000_usize;

        let ab = slice_hash(&[a, b]);
        let ac = slice_hash(&[a, c]);
        let cb = slice_hash(&[c, b]);

        assert_ne!(ab, ac, "hash must depend on the last word");
        assert_ne!(
            ab, cb,
            "hash must depend on the first word, not only the last (interner degeneracy)"
        );
        assert_ne!(ac, cb, "hashes of distinct two-word slices must differ");
    }

    #[test]
    fn fast_hasher_is_order_sensitive() {
        let a = 0x7ff6_0000_1000_usize;
        let b = 0x7ff6_0000_2000_usize;
        assert_ne!(
            slice_hash(&[a, b]),
            slice_hash(&[b, a]),
            "permuted frame order must produce a different hash"
        );
    }

    #[test]
    fn fast_hasher_equal_input_yields_equal_hash() {
        let frames = [
            0x7ff6_0000_1000_usize,
            0x7ff6_0000_2000_usize,
            0x7ff6_0000_3000_usize,
        ];
        assert_eq!(
            slice_hash(&frames),
            slice_hash(&frames),
            "independent hashers over equal input must agree (HashMap contract)"
        );
    }
}
