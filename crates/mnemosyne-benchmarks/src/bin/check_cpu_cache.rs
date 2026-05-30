use std::time::Instant;
use std::ptr::NonNull;

struct Page {
    free: Option<NonNull<u8>>,
    alloc_count: usize,
    max_blocks: usize,
}

#[derive(Clone, Copy)]
struct BitmapLayout {
    num_blocks: usize,
    num_u64s: usize,
    reserved_blocks: usize,
    block_size: usize,
}

// 1. Linear scan allocation
unsafe fn alloc_linear(page: &mut Page, page_start: *mut u8, layout: &BitmapLayout) -> *mut u8 {
    let bitmap_ptr = page_start as *mut u64;
    for i in 0..layout.num_u64s {
        let val = *bitmap_ptr.add(i);
        if val != 0 {
            let bit_idx = val.trailing_zeros() as usize;
            let block_idx = i * 64 + bit_idx;
            bitmap_ptr.add(i).write(val & !(1u64 << bit_idx));
            page.alloc_count += 1;
            if page.alloc_count == page.max_blocks {
                page.free = None;
            }
            return page_start.add(block_idx * layout.block_size);
        }
    }
    std::ptr::null_mut()
}

unsafe fn free_linear(page: &mut Page, page_start: *mut u8, layout: &BitmapLayout, block: *mut u8) {
    let block_idx = (block as usize - page_start as usize) / layout.block_size;
    let bitmap_ptr = page_start as *mut u64;
    let u64_idx = block_idx / 64;
    let bit_idx = block_idx % 64;
    let bit_mask = 1u64 << bit_idx;
    let old_val = bitmap_ptr.add(u64_idx).read();
    bitmap_ptr.add(u64_idx).write(old_val | bit_mask);
    page.alloc_count -= 1;
    if page.free.is_none() {
        page.free = Some(NonNull::dangling());
    }
}

// 2. Hint-based allocation
unsafe fn alloc_hint(page: &mut Page, page_start: *mut u8, layout: &BitmapLayout) -> *mut u8 {
    let page_start_u64 = page_start as *mut u64;
    let hint_ptr = page.free.unwrap().as_ptr() as *mut u64;
    let offset = hint_ptr as usize - page_start as usize;
    let mut hint_idx = offset / 8;
    
    if hint_idx >= layout.num_u64s {
        hint_idx = 0;
    }

    for i in hint_idx..layout.num_u64s {
        let val = *page_start_u64.add(i);
        if val != 0 {
            let bit_idx = val.trailing_zeros() as usize;
            let block_idx = i * 64 + bit_idx;
            let new_val = val & !(1u64 << bit_idx);
            page_start_u64.add(i).write(new_val);
            
            if new_val == 0 {
                let mut next_hint = i + 1;
                while next_hint < layout.num_u64s && *page_start_u64.add(next_hint) == 0 {
                    next_hint += 1;
                }
                if next_hint < layout.num_u64s {
                    page.free = Some(NonNull::new_unchecked(page_start_u64.add(next_hint) as *mut u8));
                } else {
                    page.free = Some(NonNull::dangling());
                }
            } else {
                page.free = Some(NonNull::new_unchecked(page_start_u64.add(i) as *mut u8));
            }
            
            page.alloc_count += 1;
            if page.alloc_count == page.max_blocks {
                page.free = None;
            }
            return page_start.add(block_idx * layout.block_size);
        }
    }
    std::ptr::null_mut()
}

unsafe fn free_hint(page: &mut Page, page_start: *mut u8, layout: &BitmapLayout, block: *mut u8) {
    let block_idx = (block as usize - page_start as usize) / layout.block_size;
    let page_start_u64 = page_start as *mut u64;
    let u64_idx = block_idx / 64;
    let bit_idx = block_idx % 64;
    let bit_mask = 1u64 << bit_idx;
    let old_val = page_start_u64.add(u64_idx).read();
    page_start_u64.add(u64_idx).write(old_val | bit_mask);
    
    let was_full = page.free.is_none();
    page.alloc_count -= 1;
    
    if was_full {
        page.free = Some(NonNull::new_unchecked(page_start_u64.add(u64_idx) as *mut u8));
    } else if let Some(current_hint) = page.free {
        if current_hint != NonNull::dangling() {
            let hint_offset = current_hint.as_ptr() as usize - page_start as usize;
            let current_hint_idx = hint_offset / 8;
            if u64_idx < current_hint_idx {
                page.free = Some(NonNull::new_unchecked(page_start_u64.add(u64_idx) as *mut u8));
            }
        } else {
            page.free = Some(NonNull::new_unchecked(page_start_u64.add(u64_idx) as *mut u8));
        }
    }
}

fn main() {
    let layout = BitmapLayout {
        num_blocks: 2048, // 32 B class
        num_u64s: 32,
        reserved_blocks: 8,
        block_size: 32,
    };
    
    let mut storage = vec![0u8; 65536];
    let page_start = storage.as_mut_ptr();
    
    // Initialize storage: set bitmap bits to 1 (except reserved)
    let bitmap_ptr = page_start as *mut u64;
    unsafe {
        bitmap_ptr.write(!((1u64 << layout.reserved_blocks) - 1));
        for i in 1..layout.num_u64s {
            bitmap_ptr.add(i).write(u64::MAX);
        }
    }
    
    let mut page = Page {
        free: Some(NonNull::dangling()),
        alloc_count: 0,
        max_blocks: layout.num_blocks - layout.reserved_blocks,
    };

    // Benchmark linear
    let start = Instant::now();
    let n = 10_000_000;
    for _ in 0..n {
        unsafe {
            let ptr = alloc_linear(&mut page, page_start, &layout);
            free_linear(&mut page, page_start, &layout, ptr);
        }
    }
    let elapsed_linear = start.elapsed();
    println!("Linear scan: {:.3} ns/cycle", elapsed_linear.as_nanos() as f64 / n as f64);

    // Re-initialize storage
    unsafe {
        bitmap_ptr.write(!((1u64 << layout.reserved_blocks) - 1));
        for i in 1..layout.num_u64s {
            bitmap_ptr.add(i).write(u64::MAX);
        }
    }
    
    page = Page {
        free: unsafe { Some(NonNull::new_unchecked(page_start as *mut u8)) },
        alloc_count: 0,
        max_blocks: layout.num_blocks - layout.reserved_blocks,
    };

    // Benchmark hint-based
    let start = Instant::now();
    for _ in 0..n {
        unsafe {
            let ptr = alloc_hint(&mut page, page_start, &layout);
            free_hint(&mut page, page_start, &layout, ptr);
        }
    }
    let elapsed_hint = start.elapsed();
    println!("Hint-based: {:.3} ns/cycle", elapsed_hint.as_nanos() as f64 / n as f64);
}
