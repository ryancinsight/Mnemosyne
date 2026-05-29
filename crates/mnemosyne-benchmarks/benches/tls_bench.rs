use core::sync::atomic::AtomicU32;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_local::tls::{
    AsmTls, CachedCellTls, NativeOsTls, StandardTls, TlsProvider, TlsSlotAccess,
};
use mnemosyne_local::LocalAllocatorSlot;

std::thread_local! {
    static DUMMY_SLOT: LocalAllocatorSlot<MemoryBackendWrapper> = const { LocalAllocatorSlot::new() };
    static DUMMY_CACHE_CELL: core::cell::Cell<*mut core::ffi::c_void> = const { core::cell::Cell::new(core::ptr::null_mut()) };
}

static DUMMY_OS_TLS_KEY: AtomicU32 = AtomicU32::new(u32::MAX);

struct BenchSlotAccess;

impl TlsSlotAccess<MemoryBackendWrapper> for BenchSlotAccess {
    #[inline(always)]
    fn get_slot_standard<R>(f: impl FnOnce(&LocalAllocatorSlot<MemoryBackendWrapper>) -> R) -> R {
        DUMMY_SLOT.with(f)
    }

    #[inline(always)]
    fn get_cached_cell<R>(f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R) -> R {
        DUMMY_CACHE_CELL.with(f)
    }

    #[inline(always)]
    fn arm_thread_exit(_slot: &LocalAllocatorSlot<MemoryBackendWrapper>) {
        // No-op for benchmarking standard path
    }

    #[inline(always)]
    fn get_os_tls_key() -> &'static AtomicU32 {
        &DUMMY_OS_TLS_KEY
    }
}

type BenchStandard = StandardTls<MemoryBackendWrapper, BenchSlotAccess>;
type BenchCachedCell = CachedCellTls<MemoryBackendWrapper, BenchSlotAccess>;
type BenchNativeOs = NativeOsTls<MemoryBackendWrapper, BenchSlotAccess>;
type BenchAsm = AsmTls<MemoryBackendWrapper, BenchSlotAccess>;

fn bench_tls_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("TLS Lookup Overhead");

    group.bench_function("StandardTls (std::thread_local!)", |b| {
        b.iter(|| {
            let ptr = <BenchStandard as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
            black_box(ptr);
        })
    });

    group.bench_function("CachedCellTls (thread_local! Cell)", |b| {
        // Warm up and cache the pointer
        let _ = <BenchCachedCell as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
        b.iter(|| {
            let ptr = <BenchCachedCell as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
            black_box(ptr);
        })
    });

    group.bench_function("NativeOsTls (TlsGetValue)", |b| {
        // Warm up and cache the pointer
        let _ = <BenchNativeOs as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
        b.iter(|| {
            let ptr = <BenchNativeOs as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
            black_box(ptr);
        })
    });

    group.bench_function("AsmTls (Direct TEB gs:[0x30] access)", |b| {
        // Warm up and cache the pointer
        let _ = <BenchAsm as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
        b.iter(|| {
            let ptr = <BenchAsm as TlsProvider<MemoryBackendWrapper>>::get_allocator_ptr();
            black_box(ptr);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_tls_lookup);
criterion_main!(benches);
