//! Show Mnemosyne's const-generated size-class routing.
//!
//! The example uses the canonical core mapper rather than reproducing its
//! class table. It verifies rounding and page density at representative class
//! boundaries, including the small-allocation ceiling.

extern crate mnemosyne_core;

use mnemosyne_core::{
    MAX_SMALL_ALLOC_SIZE, NUM_SIZE_CLASSES, PAGE_SIZE, class_to_max_blocks, class_to_size,
    round_up_size, size_to_class,
};

const REQUESTS: [usize; 7] = [16, 128, 512, 2048, 8192, 8193, MAX_SMALL_ALLOC_SIZE];

fn main() {
    println!("small ceiling: {MAX_SMALL_ALLOC_SIZE} B");
    println!("size classes: {NUM_SIZE_CLASSES}");

    for request in REQUESTS {
        let Some(class) = size_to_class(request) else {
            panic!("small example request must map to a class: {request} B");
        };
        let block_size = class_to_size(class);
        let blocks_per_page = class_to_max_blocks(class);

        assert!(block_size >= request);
        assert_eq!(round_up_size(request), Some(block_size));
        assert_eq!(blocks_per_page, PAGE_SIZE / block_size);

        println!(
            "request={request} B -> class={class}, block={block_size} B, blocks/page={blocks_per_page}"
        );
    }

    assert_eq!(size_to_class(MAX_SMALL_ALLOC_SIZE + 1), None);
    println!("above ceiling: large/huge route");
    println!("all size-class assertions passed");
}
