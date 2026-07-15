use super::StackId;
use super::stack_interner::intern_stack;

/// Maximum captured stack depth (instruction pointers per sample).
pub(super) const MAX_STACK_FRAMES: usize = 32;

pub(super) fn capture_stack() -> StackId {
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

pub(super) fn next_sample_interval(mean: usize) -> usize {
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
