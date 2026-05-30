use std::alloc::{Layout, GlobalAlloc};
use std::time::Instant;

fn main() {
    let layout_small = Layout::from_size_align(32, 8).unwrap();
    let layout_medium = Layout::from_size_align(1024, 8).unwrap();

    // Warm up
    for _ in 0..10_000 {
        let ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout_small) };
        unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_small) };
    }

    // Time small allocation/deallocation round trips
    let start = Instant::now();
    for _ in 0..100_000 {
        let ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout_small) };
        unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_small) };
    }
    let elapsed = start.elapsed();
    println!("100,000 small alloc/free round trips: {:?}", elapsed);

    // Time medium allocation/deallocation round trips
    let start = Instant::now();
    for _ in 0..100_000 {
        let ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout_medium) };
        unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_medium) };
    }
    let elapsed = start.elapsed();
    println!("100,000 medium alloc/free round trips: {:?}", elapsed);

    // Batch small (100)
    let mut ptrs = [std::ptr::null_mut(); 100];
    let start = Instant::now();
    for _ in 0..1_000 {
        for ptr in &mut ptrs {
            *ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout_small) };
        }
        for &ptr in &ptrs {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_small) };
        }
    }
    let elapsed = start.elapsed();
    println!("Batch small (1,000 * 100): {:?}", elapsed);

    // Batch medium (100)
    let mut ptrs = [std::ptr::null_mut(); 100];
    let start = Instant::now();
    for _ in 0..1_000 {
        for ptr in &mut ptrs {
            *ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout_medium) };
        }
        for &ptr in &ptrs {
            unsafe { mnemosyne::Mnemosyne.dealloc(ptr, layout_medium) };
        }
    }
    let elapsed = start.elapsed();
    println!("Batch medium (1,000 * 100): {:?}", elapsed);
}
