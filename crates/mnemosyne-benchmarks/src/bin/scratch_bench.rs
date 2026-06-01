use std::alloc::{GlobalAlloc, Layout};
use std::time::Instant;

fn main() {
    let layout_medium = Layout::from_size_align(1024, 8).unwrap();
    let layout_large = Layout::from_size_align(8192, 8).unwrap();

    println!("--- Medium 1024 Simulation ---");
    // Run setup and routine sequentially like Criterion with batching
    // Let's allocate N times, then free N times, to see if page count grows or if it behaves well.
    let n = 100;
    let mut ptrs = vec![std::ptr::null_mut(); n];

    // Reset stats
    let _ = mnemosyne::memory_stats();

    let start = Instant::now();
    for _ in 0..1000 {
        for slot in &mut ptrs {
            *slot = unsafe { mnemosyne::Mnemosyne.alloc(layout_medium) };
        }
        for &ptr in &ptrs {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_medium) };
        }
    }
    let elapsed = start.elapsed();
    println!("Time for 100,000 medium alloc/free: {:?}", elapsed);
    println!("{:#?}", mnemosyne::memory_stats());

    println!("--- Large 8192 Simulation ---");
    let mut ptrs_large = vec![std::ptr::null_mut(); n];
    let start = Instant::now();
    for _ in 0..1000 {
        for slot in &mut ptrs_large {
            *slot = unsafe { mnemosyne::Mnemosyne.alloc(layout_large) };
        }
        for &ptr in &ptrs_large {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_large) };
        }
    }
    let elapsed = start.elapsed();
    println!("Time for 100,000 large alloc/free: {:?}", elapsed);
    println!("{:#?}", mnemosyne::memory_stats());

    println!("--- Huge 2M Simulation ---");
    let layout_huge = Layout::from_size_align(2 * 1024 * 1024, 8).unwrap();
    // Use smaller count for huge to avoid virtual address space exhaustion in 32-bit (though we are 64-bit)
    let n_huge = 10;
    let mut ptrs_huge = vec![std::ptr::null_mut(); n_huge];
    let start = Instant::now();
    for _ in 0..100 {
        for slot in &mut ptrs_huge {
            *slot = unsafe { mnemosyne::Mnemosyne.alloc(layout_huge) };
        }
        for &ptr in &ptrs_huge {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_huge) };
        }
    }
    let elapsed = start.elapsed();
    println!("Time for 1,000 huge alloc/free: {:?}", elapsed);
    println!("{:#?}", mnemosyne::memory_stats());
}
