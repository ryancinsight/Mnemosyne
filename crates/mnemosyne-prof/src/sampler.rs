use core::ffi::c_void;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::{ACTIVE_SAMPLES_COUNT, SAMPLE_INTERVAL};

use core::hash::{BuildHasher, Hasher};

#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

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
pub struct FastBuildHasher;

impl BuildHasher for FastBuildHasher {
    type Hasher = FastHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0)
    }
}

/// Maximum captured stack depth (instruction pointers per sample).
pub(crate) const MAX_STACK_FRAMES: usize = 32;

/// Interned identity of a captured stack trace.
///
/// Samples store this `u32` handle instead of an owned `Box<[usize]>`, so the
/// per-live-allocation metadata is a fixed 4 bytes regardless of stack depth and
/// the actual frame arrays are deduplicated: the leak detector's retained memory
/// scales with the number of *distinct call sites*, not the number of live
/// allocations (which can differ by orders of magnitude).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StackId(u32);

const STACK_INTERNER_SHARDS: usize = 64;
const STACK_INTERNER_SHARD_BITS: u32 = STACK_INTERNER_SHARDS.trailing_zeros();
const STACK_ID_LOCAL_BITS: u32 = u32::BITS - STACK_INTERNER_SHARD_BITS;
const STACK_ID_LOCAL_MASK: u32 = (1u32 << STACK_ID_LOCAL_BITS) - 1;
const _: () = assert!(STACK_INTERNER_SHARDS.is_power_of_two());

impl StackId {
    #[inline]
    fn new(shard: usize, local_id: u32) -> Self {
        debug_assert!(shard < STACK_INTERNER_SHARDS);
        debug_assert!(local_id <= STACK_ID_LOCAL_MASK);
        Self(((shard as u32) << STACK_ID_LOCAL_BITS) | local_id)
    }

    #[inline]
    fn shard(self) -> usize {
        (self.0 >> STACK_ID_LOCAL_BITS) as usize
    }

    #[inline]
    fn local_index(self) -> usize {
        (self.0 & STACK_ID_LOCAL_MASK) as usize
    }

    #[inline]
    fn local_id(self) -> u32 {
        self.0 & STACK_ID_LOCAL_MASK
    }
}

/// Representation of a sampled memory allocation.
#[derive(Clone, Copy)]
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Interned identity of the retained stack trace (resolve via the interner).
    pub stack: StackId,
}

#[derive(Clone)]
struct ActiveSample {
    ptr: usize,
    size: usize,
    stack: Arc<[usize]>,
}

/// Global stack-trace interner shared by every sampled allocation.
///
/// Each distinct live frame sequence is stored exactly once as an `Arc<[usize]>`.
/// Repeat call sites increment a reference count without allocating; the last
/// free removes the content-keyed entry and recycles the id slot.
struct StackInternerShard {
    forward: HashMap<Arc<[usize]>, StackId, FastBuildHasher>,
    entries: Vec<Option<StackEntry>>,
    free_ids: Vec<u32>,
}

struct StackEntry {
    frames: Arc<[usize]>,
    refs: usize,
}

#[repr(align(64))]
struct InternerShard {
    mutex: Mutex<Option<StackInternerShard>>,
}

static STACK_INTERNER: [InternerShard; STACK_INTERNER_SHARDS] = [const {
    InternerShard {
        mutex: Mutex::new(None),
    }
}; STACK_INTERNER_SHARDS];

fn stack_interner_shard(frames: &[usize]) -> usize {
    (FastBuildHasher.hash_one(frames) as usize) & (STACK_INTERNER_SHARDS - 1)
}

fn get_stack_interner(shard: usize) -> std::sync::MutexGuard<'static, Option<StackInternerShard>> {
    let mut lock = STACK_INTERNER[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if lock.is_none() {
        *lock = Some(StackInternerShard {
            forward: HashMap::with_hasher(FastBuildHasher),
            entries: Vec::new(),
            free_ids: Vec::new(),
        });
    }
    lock
}

fn resolve_stack(id: StackId) -> Option<Arc<[usize]>> {
    let guard = STACK_INTERNER[id.shard()]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_ref().and_then(|interner| interner.resolve(id))
}

/// Interns `frames`, returning its stable [`StackId`]. Allocates only on a
/// first-seen call site; repeat sites are a hash lookup with no allocation.
fn intern_stack(frames: &[usize]) -> StackId {
    let shard = stack_interner_shard(frames);
    {
        let mut guard = get_stack_interner(shard);
        let interner = guard
            .as_mut()
            .expect("stack interner shard must be initialized");
        if let Some(&id) = interner.forward.get(frames) {
            return interner.retain(id);
        }
    }

    let arc: Arc<[usize]> = Arc::from(frames);
    let mut guard = get_stack_interner(shard);
    let interner = guard
        .as_mut()
        .expect("stack interner shard must be initialized");
    if let Some(&id) = interner.forward.get(arc.as_ref()) {
        return interner.retain(id);
    }
    let id = if let Some(local_id) = interner.free_ids.pop() {
        let id = StackId::new(shard, local_id);
        interner.entries[local_id as usize] = Some(StackEntry {
            frames: Arc::clone(&arc),
            refs: 1,
        });
        id
    } else {
        assert!(
            interner.entries.len() <= STACK_ID_LOCAL_MASK as usize,
            "invariant: stack interner shard id count exceeds its bit budget"
        );
        let local_id = u32::try_from(interner.entries.len())
            .expect("invariant: stack interner shard id count exceeds u32::MAX");
        let id = StackId::new(shard, local_id);
        interner.entries.push(Some(StackEntry {
            frames: Arc::clone(&arc),
            refs: 1,
        }));
        id
    };
    interner.forward.insert(arc, id);
    id
}

fn release_stack(id: StackId) {
    let mut guard = STACK_INTERNER[id.shard()]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(interner) = guard.as_mut() else {
        return;
    };
    interner.release(id);
}

impl StackInternerShard {
    fn retain(&mut self, id: StackId) -> StackId {
        let entry = self
            .entries
            .get_mut(id.local_index())
            .and_then(Option::as_mut)
            .expect("invariant: stack interner forward map points at a live entry");
        entry.refs = entry
            .refs
            .checked_add(1)
            .expect("invariant: stack interner reference count overflow");
        id
    }

    fn resolve(&self, id: StackId) -> Option<Arc<[usize]>> {
        self.entries
            .get(id.local_index())
            .and_then(Option::as_ref)
            .map(|entry| Arc::clone(&entry.frames))
    }

    fn release(&mut self, id: StackId) {
        let Some(entry_slot) = self.entries.get_mut(id.local_index()) else {
            return;
        };
        let Some(entry) = entry_slot.as_mut() else {
            return;
        };
        if entry.refs > 1 {
            entry.refs -= 1;
            return;
        }

        let frames = Arc::clone(&entry.frames);
        *entry_slot = None;
        self.forward.remove(frames.as_ref());
        self.free_ids.push(id.local_id());
    }
}

const SHARDS: usize = 64;

#[repr(align(64))]
struct Shard {
    mutex: Mutex<Option<HashMap<usize, Sample, FastBuildHasher>>>,
}

static ACTIVE_SAMPLES: [Shard; SHARDS] = [const {
    Shard {
        mutex: Mutex::new(None),
    }
}; SHARDS];

fn get_map(
    shard: usize,
) -> std::sync::MutexGuard<'static, Option<HashMap<usize, Sample, FastBuildHasher>>> {
    let mut lock = ACTIVE_SAMPLES[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if lock.is_none() {
        *lock = Some(HashMap::with_hasher(FastBuildHasher));
    }
    lock
}

/// Reset the sampler state (active samples).
pub(crate) fn reset_sampler_state() {
    for shard in &ACTIVE_SAMPLES {
        let mut lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *lock = None;
    }
    for shard in &STACK_INTERNER {
        let mut lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *lock = None;
    }
}

pub(crate) fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    let debit = crate::sample_debit(size);

    #[cfg(nightly_tls_active)]
    {
        let mut val = crate::get_bytes_until_sample();
        maybe_record_sample(ptr, size, leak_active, debit, &mut val);
        crate::set_bytes_until_sample(val);
    }

    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // `ThreadState`; the `&mut` is exclusive (thread-local) and this runs inside
    // the `enter_hook`/`exit_hook` re-entrancy guard, so no nested `&mut` to the
    // same state can be live.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        let state = &mut *crate::get_profiler_state();
        maybe_record_sample(ptr, size, leak_active, debit, &mut state.bytes_until_sample);
    }
}

fn maybe_record_sample(
    ptr: *mut u8,
    size: usize,
    leak_active: bool,
    debit: isize,
    bytes_until_sample: &mut isize,
) {
    if leak_active || *bytes_until_sample <= debit {
        if !leak_active {
            let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
            *bytes_until_sample = next_sample_interval(mean) as isize;
        }

        let stack = capture_stack();
        let shard = sample_shard(ptr as usize);
        let replaced = {
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                let replaced = map.insert(ptr as usize, Sample { size, stack });
                if replaced.is_none() {
                    ACTIVE_SAMPLES_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                replaced
            } else {
                None
            }
        };
        if let Some(replaced) = replaced {
            release_stack(replaced.stack);
        }
    }

    if !leak_active {
        *bytes_until_sample = (*bytes_until_sample).saturating_sub(debit);
    }
}

pub(crate) fn capture_stack() -> StackId {
    let (frames, len) = capture_stack_frames();
    intern_stack(&frames[..len])
}

fn capture_stack_frames() -> ([usize; MAX_STACK_FRAMES], usize) {
    let mut frames = [0usize; MAX_STACK_FRAMES];
    let mut len = 0usize;

    backtrace::trace(|frame| {
        let ip = frame.ip() as usize;
        if ip != 0 {
            if len == MAX_STACK_FRAMES {
                return false;
            }
            frames[len] = ip;
            len += 1;
        }
        len < MAX_STACK_FRAMES
    });

    (frames, len)
}

pub(crate) fn sample_free_inner(ptr: *mut u8) {
    let shard = sample_shard(ptr as usize);
    let removed = {
        let mut lock = get_map(shard);
        if let Some(ref mut map) = *lock {
            let removed = map.remove(&(ptr as usize));
            if removed.is_some() {
                ACTIVE_SAMPLES_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            removed
        } else {
            None
        }
    };
    if let Some(sample) = removed {
        release_stack(sample.stack);
    }
}

#[inline]
fn sample_shard(ptr: usize) -> usize {
    (ptr >> 6) % SHARDS
}

fn next_sample_interval(mean: usize) -> usize {
    std::thread_local! {
        static RNG: core::cell::Cell<u64> = const { core::cell::Cell::new(0x123456789abcdef) };
    }
    RNG.with(|rng_state| {
        let mut state = rng_state.get();
        if state == 0 {
            state = 0x123456789abcdef
                ^ (std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64);
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        rng_state.set(state);

        let u = (state as f64) / (u64::MAX as f64);
        let u = if u < 1e-9 { 1e-9 } else { u };
        (-u.ln() * mean as f64) as usize
    })
}

/// Dumps a folded stack profile of active memory allocations to a file.
///
/// The output uses the standard collapsed stack format:
/// `func1;func2;func3 <bytes>`
pub fn dump_profile(path: &str) -> std::io::Result<()> {
    let in_hook = crate::enter_hook();
    if in_hook {
        return Ok(());
    }

    let result = dump_profile_inner(path);

    crate::exit_hook();
    result
}

fn dump_profile_inner(path: &str) -> std::io::Result<()> {
    let mut folded: HashMap<String, usize> = HashMap::new();
    let samples = active_sample_snapshot();
    for sample in &samples {
        fold_sample_stack(sample, &mut folded);
    }

    let mut file = std::fs::File::create(path)?;
    for (stack, bytes) in folded {
        writeln!(file, "{} {}", stack, bytes)?;
    }

    Ok(())
}

fn fold_sample_stack(sample: &ActiveSample, folded: &mut HashMap<String, usize>) {
    let mut stack = String::new();
    for &ip in sample.stack.iter().rev() {
        let mut symbol = String::new();
        backtrace::resolve(ip as *mut c_void, |resolved| {
            if let Some(name) = resolved.name() {
                let _ = write!(symbol, "{name}");
            }
        });
        if symbol.is_empty() {
            let _ = write!(symbol, "{ip:#x}");
        }
        if is_profiler_internal_symbol(&symbol) {
            continue;
        }
        if !stack.is_empty() {
            stack.push(';');
        }
        stack.push_str(&symbol);
    }

    if !stack.is_empty() {
        *folded.entry(stack).or_insert(0) += sample.size;
    }
}

#[inline]
fn is_profiler_internal_symbol(symbol: &str) -> bool {
    symbol.contains("mnemosyne_prof")
        || symbol.contains("sample_alloc")
        || symbol.contains("on_alloc")
        || symbol.contains("thread_alloc")
        || symbol.contains("mnemosyne_heap::heap::Heap")
        || symbol.contains("backtrace::")
}

/// Dumps all active allocations (representing leaks) with their resolved stacks to a file.
///
/// Returns the number of leaked blocks.
pub fn dump_leaks(path: &str) -> std::io::Result<usize> {
    let in_hook = crate::enter_hook();
    if in_hook {
        return Ok(0);
    }

    let result = dump_leaks_inner(path);

    crate::exit_hook();
    result
}

fn dump_leaks_inner(path: &str) -> std::io::Result<usize> {
    let mut file = None;
    let samples = active_sample_snapshot();
    for sample in &samples {
        let file = leak_report_file(path, &mut file)?;
        write_leak_sample(file, sample)?;
    }

    Ok(samples.len())
}

fn active_sample_snapshot() -> Vec<ActiveSample> {
    let total = ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed);
    let mut samples = Vec::with_capacity(total);
    for shard in &ACTIVE_SAMPLES {
        let lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(ref map) = *lock {
            samples.extend(map.iter().filter_map(|(&ptr, sample)| {
                resolve_stack(sample.stack).map(|stack| ActiveSample {
                    ptr,
                    size: sample.size,
                    stack,
                })
            }));
        }
    }
    samples
}

fn leak_report_file<'a>(
    path: &str,
    file: &'a mut Option<std::fs::File>,
) -> std::io::Result<&'a mut std::fs::File> {
    if file.is_none() {
        let mut created = std::fs::File::create(path)?;
        write_leak_report_header(&mut created)?;
        *file = Some(created);
    }
    match file.as_mut() {
        Some(file) => Ok(file),
        None => Err(std::io::Error::other(
            "leak report file was not initialized",
        )),
    }
}

fn write_leak_report_header(file: &mut std::fs::File) -> std::io::Result<()> {
    writeln!(file, "Mnemosyne Leak Report:")?;
    writeln!(file, "======================")
}

fn write_leak_sample(file: &mut std::fs::File, sample: &ActiveSample) -> std::io::Result<()> {
    writeln!(
        file,
        "\nLeak of {} bytes at {:#x}:",
        sample.size, sample.ptr
    )?;
    for (idx, &ip) in sample.stack.iter().enumerate() {
        let mut name = String::new();
        let mut file_path = String::new();
        let mut line_opt = None;
        backtrace::resolve(ip as *mut c_void, |symbol| {
            if let Some(symbol_name) = symbol.name() {
                let _ = write!(name, "{symbol_name}");
            }
            if let Some(path_buf) = symbol.filename() {
                let _ = write!(file_path, "{}", path_buf.to_string_lossy());
            }
            line_opt = symbol.lineno();
        });
        if name.is_empty() {
            let _ = write!(name, "{ip:#x}");
        }

        match (file_path.is_empty(), line_opt) {
            (false, Some(line)) => writeln!(file, "  #{}: {} ({}:{})", idx, name, file_path, line)?,
            (false, None) => writeln!(file, "  #{}: {} ({})", idx, name, file_path)?,
            (true, _) => writeln!(file, "  #{}: {}", idx, name)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn frames_for_shards<const N: usize>() -> [(usize, [usize; 2]); N] {
        let mut frames = [(usize::MAX, [0usize; 2]); N];
        let mut found = 0usize;
        for word in 1..16_384usize {
            let stack = [0x7ff6_0000_0000usize | word, 0x7ff6_ffff_ffffusize];
            let shard = stack_interner_shard(&stack);
            if frames[..found].iter().all(|(seen, _)| *seen != shard) {
                frames[found] = (shard, stack);
                found += 1;
                if found == N {
                    return frames;
                }
            }
        }
        panic!("invariant: deterministic stack hash did not cover {N} distinct shards");
    }

    fn distinct_frames_for_shard(shard: usize, excluded: &[usize]) -> [usize; 2] {
        for word in 1..16_384usize {
            let stack = [0x7ff6_1000_0000usize | word, 0x7ff6_ffff_ffffusize];
            if stack_interner_shard(&stack) == shard && stack.as_slice() != excluded {
                return stack;
            }
        }
        panic!("invariant: deterministic stack hash did not find a distinct same-shard stack");
    }

    #[test]
    fn stack_interner_hash_covers_all_shards() {
        let frames = frames_for_shards::<STACK_INTERNER_SHARDS>();
        let mut seen = [false; STACK_INTERNER_SHARDS];
        for (shard, stack) in frames {
            assert_eq!(
                stack_interner_shard(&stack),
                shard,
                "fixture must route to its recorded shard"
            );
            seen[shard] = true;
        }
        assert!(
            seen.into_iter().all(|covered| covered),
            "deterministic stack fixtures must cover every interner shard"
        );
    }

    #[test]
    fn stack_interner_encodes_shard_and_local_id() {
        crate::reset_profiler_for_testing();

        let [(first_shard, first_stack), (second_shard, second_stack)] = frames_for_shards::<2>();
        let first = intern_stack(&first_stack);
        let second = intern_stack(&second_stack);

        assert_eq!(first.shard(), first_shard);
        assert_eq!(second.shard(), second_shard);
        assert_eq!(first.local_id(), 0);
        assert_eq!(second.local_id(), 0);
        assert_ne!(
            first, second,
            "equal local ids in distinct shards must still form distinct StackIds"
        );

        release_stack(first);
        release_stack(second);
        crate::reset_profiler_for_testing();
    }

    #[test]
    fn stack_interner_reuses_ids_and_releases_last_reference() {
        crate::reset_profiler_for_testing();

        let first = intern_stack(&[1, 2, 3]);
        let repeat = intern_stack(&[1, 2, 3]);
        assert_eq!(first, repeat);

        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard.as_ref().expect("stack interner must be initialized");
            let entry = interner.entries[first.local_index()]
                .as_ref()
                .expect("interned stack id must point to a live entry");
            assert_eq!(entry.refs, 2);
            assert_eq!(interner.forward.len(), 1);
        }

        release_stack(first);
        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard
                .as_ref()
                .expect("stack interner must stay initialized");
            let entry = interner.entries[first.local_index()]
                .as_ref()
                .expect("one remaining reference must keep the entry live");
            assert_eq!(entry.refs, 1);
            assert_eq!(interner.forward.len(), 1);
        }

        release_stack(repeat);
        {
            let guard = STACK_INTERNER[first.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard
                .as_ref()
                .expect("stack interner must stay initialized");
            assert!(interner.entries[first.local_index()].is_none());
            assert!(interner.forward.is_empty());
            assert_eq!(interner.free_ids.as_slice(), &[first.local_id()]);
        }

        let same_shard = distinct_frames_for_shard(first.shard(), &[1, 2, 3]);
        let reused = intern_stack(&same_shard);
        assert_eq!(
            reused, first,
            "released same-shard stack ids should be recycled instead of growing the table"
        );

        crate::reset_profiler_for_testing();
    }

    #[test]
    fn stack_interner_interns_distinct_shards_concurrently() {
        crate::reset_profiler_for_testing();

        let frames = frames_for_shards::<STACK_INTERNER_SHARDS>();
        let barrier = Arc::new(std::sync::Barrier::new(STACK_INTERNER_SHARDS));
        let mut workers = Vec::with_capacity(STACK_INTERNER_SHARDS);
        for (expected_shard, stack) in frames {
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let id = intern_stack(&stack);
                assert_eq!(id.shard(), expected_shard);
                id
            }));
        }

        let ids: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("interner worker must not panic"))
            .collect();
        assert_eq!(ids.len(), STACK_INTERNER_SHARDS);
        for id in &ids {
            let guard = STACK_INTERNER[id.shard()]
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let interner = guard.as_ref().expect("stack interner must be initialized");
            let entry = interner.entries[id.local_index()]
                .as_ref()
                .expect("worker interned stack id must point to a live entry");
            assert_eq!(entry.refs, 1);
        }
        for id in ids {
            release_stack(id);
        }

        crate::reset_profiler_for_testing();
    }

    #[test]
    fn active_sample_snapshot_is_detached_from_live_shards() {
        crate::reset_profiler_for_testing();

        let ptr = 0x1000usize as *mut u8;
        sample_alloc_inner(ptr, 64, true);

        let snapshot = active_sample_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].ptr, ptr as usize);
        assert_eq!(snapshot[0].size, 64);
        assert!(
            !snapshot[0].stack.is_empty(),
            "snapshot must retain resolved stack frames"
        );

        sample_free_inner(ptr);
        assert!(
            active_sample_snapshot().is_empty(),
            "live shard map must be empty after freeing the sampled pointer"
        );
        assert_eq!(
            snapshot[0].size, 64,
            "snapshot must retain a value copy after the live sample is removed"
        );

        crate::reset_profiler_for_testing();
    }
}
