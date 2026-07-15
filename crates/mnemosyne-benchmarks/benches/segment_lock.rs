#[path = "../../mnemosyne-arena/src/segment/pool/cache_aligned.rs"]
mod cache_aligned;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use cache_aligned::{CacheAlignedAtomicPtr, CacheAlignedAtomicUsize, CacheAlignedSegmentLock};
use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

const WORKERS: usize = 4;
const OPERATIONS: usize = 1_000;
const QUEUE_BOUND: usize = WORKERS;

struct LockWorker {
    sender: SyncSender<Option<usize>>,
    handle: Option<JoinHandle<()>>,
}

struct LockWorkers {
    workers: [LockWorker; WORKERS],
    done: Receiver<()>,
}

impl LockWorkers {
    fn new(lock: Arc<CacheAlignedSegmentLock>) -> Self {
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
        Self { workers, done }
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

impl Drop for LockWorkers {
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
    let pointer = CacheAlignedAtomicPtr::new();
    let state = pointer.load(Ordering::Relaxed);
    let address = CacheAlignedAtomicPtr::ptr(state);
    let successor = CacheAlignedAtomicPtr::tagged_successor(address, state);
    let _ = pointer.compare_exchange_weak(state, successor, Ordering::AcqRel, Ordering::Acquire);
    let _ = pointer.swap_null(Ordering::AcqRel);

    let counter = CacheAlignedAtomicUsize::new(0);
    black_box(counter.value.load(Ordering::Relaxed));
}

pub fn bench_segment_lock(c: &mut Criterion) {
    validate_included_canonical_surface();
    let mut group = c.benchmark_group("Segment lock");
    group.throughput(Throughput::Elements(1));

    let uncontended_lock = CacheAlignedSegmentLock::new();
    group.bench_function("Uncontended", |b| {
        b.iter(|| {
            let guard = uncontended_lock.lock();
            black_box(&guard);
        })
    });

    group.throughput(Throughput::Elements((WORKERS * OPERATIONS) as u64));
    let contended_workers = LockWorkers::new(Arc::new(CacheAlignedSegmentLock::new()));
    group.bench_function("Contended", |b| b.iter(|| contended_workers.run()));
    group.finish();
}

criterion_group!(benches, bench_segment_lock);
criterion_main!(benches);
