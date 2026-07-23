#[path = "../../mnemosyne-arena/src/segment/pool/cache_aligned.rs"]
mod cache_aligned;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use cache_aligned::{
    CacheAlignedAtomicUsize, CacheAlignedSegmentLock, TaggedHead, TaggedStackState,
};
use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

const WORKERS: usize = 4;
const OPERATIONS: usize = 1_000;
const QUEUE_BOUND: usize = WORKERS;

trait LockStrategy: Send + Sync + 'static {
    type Guard<'a>: 'a
    where
        Self: 'a;

    fn lock(&self) -> Self::Guard<'_>;
}

impl LockStrategy for CacheAlignedSegmentLock {
    type Guard<'a> = cache_aligned::SegmentLockGuard<'a>;

    fn lock(&self) -> Self::Guard<'_> {
        CacheAlignedSegmentLock::lock(self)
    }
}

struct Unlocked;
struct UnlockedGuard;

impl LockStrategy for Unlocked {
    type Guard<'a> = UnlockedGuard;

    fn lock(&self) -> Self::Guard<'_> {
        UnlockedGuard
    }
}

struct LockWorker {
    sender: SyncSender<Option<usize>>,
    handle: Option<JoinHandle<()>>,
}

struct LockWorkers<S: LockStrategy> {
    workers: [LockWorker; WORKERS],
    done: Receiver<()>,
    _strategy: std::marker::PhantomData<S>,
}

impl<S: LockStrategy> LockWorkers<S> {
    fn new(lock: Arc<S>) -> Self {
        let (done_sender, done) = sync_channel::<()>(QUEUE_BOUND);
        let workers = std::array::from_fn(|_| {
            let (sender, receiver) = sync_channel::<Option<usize>>(QUEUE_BOUND);
            let worker_done = done_sender.clone();
            let worker_lock = Arc::clone(&lock);
            let handle = thread::spawn(move || {
                while let Ok(Some(iterations)) = receiver.recv() {
                    for _ in 0..iterations {
                        let guard = worker_lock.lock();
                        black_box(&guard);
                    }
                    if worker_done.send(()).is_err() {
                        return;
                    }
                }
            });
            LockWorker {
                sender,
                handle: Some(handle),
            }
        });
        Self {
            workers,
            done,
            _strategy: std::marker::PhantomData,
        }
    }

    fn run(&self) {
        for worker in &self.workers {
            worker
                .sender
                .send(Some(OPERATIONS))
                .expect("lock benchmark worker command channel must remain open");
        }
        for _ in 0..WORKERS {
            self.done
                .recv()
                .expect("lock benchmark worker completion channel must remain open");
        }
    }
}

impl<S: LockStrategy> Drop for LockWorkers<S> {
    fn drop(&mut self) {
        for worker in &self.workers {
            worker
                .sender
                .send(None)
                .expect("lock benchmark worker command channel must remain open");
        }
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                handle
                    .join()
                    .expect("lock benchmark worker must exit without panic");
            }
        }
    }
}

fn validate_included_canonical_surface() {
    let pointer = TaggedHead::new();
    let state = pointer.load(Ordering::Relaxed);
    let address = TaggedHead::ptr(state);
    let successor = TaggedHead::tagged_successor(address, state);
    match pointer.compare_exchange_weak(state, successor, Ordering::AcqRel, Ordering::Acquire) {
        Ok(value) | Err(value) => black_box(value),
    };
    black_box(pointer.swap_null(Ordering::AcqRel));

    let packed = TaggedStackState::new();
    assert_eq!(
        std::mem::size_of_val(&packed),
        std::mem::align_of_val(&packed)
    );
    black_box(packed.len());

    let counter = CacheAlignedAtomicUsize::new(0);
    black_box(counter.value.load(Ordering::Relaxed));
}

pub fn bench_segment_lock(c: &mut Criterion) {
    validate_included_canonical_surface();
    let mut group = c.benchmark_group("Segment lock");
    group.throughput(Throughput::Elements(1));

    let reference_uncontended = Unlocked;
    group.bench_function("Reference/Uncontended", |b| {
        b.iter(|| black_box(reference_uncontended.lock()))
    });

    let lifetime_lock = CacheAlignedSegmentLock::new();
    group.bench_function("LifetimeLock/Uncontended", |b| {
        b.iter(|| black_box(lifetime_lock.lock()))
    });

    group.throughput(Throughput::Elements((WORKERS * OPERATIONS) as u64));
    let reference_workers = LockWorkers::new(Arc::new(Unlocked));
    group.bench_function("Reference/Contended", |b| {
        b.iter(|| reference_workers.run())
    });

    let lifetime_workers = LockWorkers::new(Arc::new(CacheAlignedSegmentLock::new()));
    group.bench_function("LifetimeLock/Contended", |b| {
        b.iter(|| lifetime_workers.run())
    });
    group.finish();
}

criterion_group!(benches, bench_segment_lock);
criterion_main!(benches);
