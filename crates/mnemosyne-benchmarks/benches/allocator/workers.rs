use core::alloc::{GlobalAlloc, Layout};
use criterion::black_box;
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;

use super::constants::{
    CROSS_THREAD_ALLOCS, CROSS_THREAD_QUEUE_BOUND, THREAD_ALLOCS, THREAD_WORK_QUEUE_BOUND, THREADS,
};
use super::helpers::{alloc_dealloc, benchmark_failure, require_allocated};

pub struct HandoffBatch {
    pub layout: Layout,
    pub count: usize,
}

pub struct HandoffBuffer {
    slots: UnsafeCell<[usize; CROSS_THREAD_ALLOCS]>,
}

// Safety: the `slots` cell is never accessed concurrently. The producer writes
// exactly `batch.count` slots, then `send`s the batch over the `sync_channel`;
// the worker only `read`s after `recv` returns that batch, and the producer
// blocks on `done.recv()` before writing again. The channel send/recv pair
// establishes a happens-before edge in each direction, so producer writes and
// worker reads are strictly ordered and never overlap despite the shared `&`.
unsafe impl Sync for HandoffBuffer {}

impl HandoffBuffer {
    #[inline]
    pub fn new() -> Self {
        Self {
            slots: UnsafeCell::new([0; CROSS_THREAD_ALLOCS]),
        }
    }

    #[inline(always)]
    pub unsafe fn write(&self, index: usize, ptr: usize) {
        debug_assert!(index < CROSS_THREAD_ALLOCS);
        unsafe {
            (*self.slots.get())[index] = ptr;
        }
    }

    #[inline(always)]
    pub unsafe fn read(&self, index: usize) -> usize {
        debug_assert!(index < CROSS_THREAD_ALLOCS);
        unsafe { (*self.slots.get())[index] }
    }
}

pub struct HandoffWorker<A: GlobalAlloc + Send + Sync + 'static> {
    pub allocator: &'static A,
    pub buffer: Arc<HandoffBuffer>,
    pub sender: SyncSender<Option<HandoffBatch>>,
    pub done: Receiver<()>,
    pub handle: Option<thread::JoinHandle<()>>,
}

impl<A: GlobalAlloc + Send + Sync + 'static> HandoffWorker<A> {
    pub fn new(allocator: &'static A) -> Self {
        let buffer = Arc::new(HandoffBuffer::new());
        let worker_buffer = Arc::clone(&buffer);
        let (sender, receiver) = sync_channel::<Option<HandoffBatch>>(CROSS_THREAD_QUEUE_BOUND);
        let (done_sender, done) = sync_channel::<()>(CROSS_THREAD_QUEUE_BOUND);
        let handle = thread::spawn(move || {
            while let Ok(Some(batch)) = receiver.recv() {
                for index in 0..batch.count {
                    // Safety: the producer writes exactly `batch.count`
                    // initialized slots before sending this command, and waits
                    // for `done` before reusing the buffer.
                    let ptr = unsafe { worker_buffer.read(index) };
                    // Safety: each pointer in the batch was allocated by the
                    // same allocator with `batch.layout` before handoff.
                    unsafe {
                        allocator.dealloc(ptr as *mut u8, batch.layout);
                    }
                }
                if done_sender.send(()).is_err() {
                    return;
                }
            }
        });

        Self {
            allocator,
            buffer,
            sender,
            done,
            handle: Some(handle),
        }
    }

    pub fn alloc_then_handoff(&self, layout: Layout, count: usize) {
        debug_assert!(count <= CROSS_THREAD_ALLOCS);
        for index in 0..count {
            // Safety: `layout` is one of the static benchmark layouts.
            let allocated = unsafe { self.allocator.alloc(black_box(layout)) };
            require_allocated(allocated, "cross-thread handoff allocation");
            // Safety: `index < count <= CROSS_THREAD_ALLOCS`, and the worker
            // cannot read this buffer until the command is sent below.
            unsafe { self.buffer.write(index, allocated as usize) };
        }
        black_box(&self.buffer);
        if self
            .sender
            .send(Some(HandoffBatch { layout, count }))
            .is_err()
        {
            benchmark_failure("cross-thread handoff", "worker command channel closed");
        }
        if self.done.recv().is_err() {
            benchmark_failure("cross-thread handoff", "worker completion channel closed");
        }
    }
}

impl<A: GlobalAlloc + Send + Sync + 'static> Drop for HandoffWorker<A> {
    fn drop(&mut self) {
        let _ = self.sender.send(None);
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            eprintln!(
                "benchmark failure: cross-thread handoff worker panicked during shutdown"
            );
        }
    }
}

pub struct ThreadCycleWorker {
    pub sender: SyncSender<Option<usize>>,
    pub handle: Option<thread::JoinHandle<()>>,
}

pub struct ThreadCycleWorkers<A: GlobalAlloc + Send + Sync + 'static> {
    pub workers: [ThreadCycleWorker; THREADS],
    pub done: Receiver<()>,
    pub _allocator: &'static A,
}

impl<A: GlobalAlloc + Send + Sync + 'static> ThreadCycleWorkers<A> {
    pub fn new(allocator: &'static A, layout: Layout) -> Self {
        let (done_sender, done) = sync_channel::<()>(THREAD_WORK_QUEUE_BOUND);
        let workers = std::array::from_fn(|_| {
            let (sender, receiver) = sync_channel::<Option<usize>>(THREAD_WORK_QUEUE_BOUND);
            let worker_done = done_sender.clone();
            let handle_allocator = allocator;
            let handle = thread::spawn(move || {
                while let Ok(Some(iterations)) = receiver.recv() {
                    for _ in 0..iterations {
                        // Safety: `layout` is a valid static layout and
                        // `alloc_dealloc` validates non-null allocations.
                        unsafe {
                            alloc_dealloc(handle_allocator, layout);
                        }
                    }
                    if worker_done.send(()).is_err() {
                        return;
                    }
                }
            });
            ThreadCycleWorker {
                sender,
                handle: Some(handle),
            }
        });

        Self {
            workers,
            done,
            _allocator: allocator,
        }
    }

    pub fn run(&self) {
        self.run_with_iterations(THREAD_ALLOCS);
    }

    pub fn run_with_iterations(&self, iterations: usize) {
        for worker in &self.workers {
            if worker.sender.send(Some(iterations)).is_err() {
                benchmark_failure("threaded allocation cycle", "worker command channel closed");
            }
        }
        for _ in 0..THREADS {
            if self.done.recv().is_err() {
                benchmark_failure(
                    "threaded allocation cycle",
                    "worker completion channel closed",
                );
            }
        }
    }
}

impl<A: GlobalAlloc + Send + Sync + 'static> Drop for ThreadCycleWorkers<A> {
    fn drop(&mut self) {
        for worker in &self.workers {
            let _ = worker.sender.send(None);
        }
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take()
                && handle.join().is_err()
            {
                eprintln!(
                    "benchmark failure: allocation-cycle worker panicked during shutdown"
                );
            }
        }
    }
}
