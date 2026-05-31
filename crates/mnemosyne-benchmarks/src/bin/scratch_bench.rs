use std::alloc::{Layout, GlobalAlloc};
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
        // Setup phase: allocate N blocks
        for i in 0..n {
            ptrs[i] = unsafe { mnemosyne::Mnemosyne.alloc(layout_medium) };
        }
        // Routine phase: free N blocks
        for i in 0..n {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptrs[i], layout_medium) };
        }
    }
    let elapsed = start.elapsed();
    println!("Time for 100,000 medium alloc/free: {:?}", elapsed);
    println!("{:#?}", mnemosyne::memory_stats());

    println!("--- Large 8192 Simulation ---");
    let mut ptrs_large = vec![std::ptr::null_mut(); n];
    let start = Instant::now();
    for _ in 0..1000 {
        // Setup phase: allocate N blocks
        for i in 0..n {
            ptrs_large[i] = unsafe { mnemosyne::Mnemosyne.alloc(layout_large) };
        }
        // Routine phase: free N blocks
        for i in 0..n {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptrs_large[i], layout_large) };
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
        // Setup phase: allocate N blocks
        for i in 0..n_huge {
            ptrs_huge[i] = unsafe { mnemosyne::Mnemosyne.alloc(layout_huge) };
        }
        // Routine phase: free N blocks
        for i in 0..n_huge {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptrs_huge[i], layout_huge) };
        }
    }
    let elapsed = start.elapsed();
    println!("Time for 1,000 huge alloc/free: {:?}", elapsed);
    println!("{:#?}", mnemosyne::memory_stats());
}

