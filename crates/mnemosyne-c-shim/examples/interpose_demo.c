#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include "../include/mnemosyne.h"

int main(void) {
    printf("=== Mnemosyne C ABI Interposition Demo ===\n");

    // 1. Perform standard malloc and check alignment
    size_t size = 24;
    void *ptr1 = malloc(size);
    if (!ptr1) {
        fprintf(stderr, "Error: malloc(%zu) returned NULL\n", size);
        return 1;
    }
    printf("[malloc] Allocated %zu bytes at address: %p\n", size, ptr1);

    // Verify alignment to 16 bytes (fundamental alignment / MALLOC_ALIGN)
    if ((size_t)ptr1 % 16 == 0) {
        printf("[alignment] Pointer is correctly 16-byte aligned.\n");
    } else {
        fprintf(stderr, "Error: Pointer is not 16-byte aligned!\n");
        free(ptr1);
        return 1;
    }

    // Write to the memory to verify it is writable
    memset(ptr1, 0xAA, size);
    printf("[write] Successfully wrote sentinel patterns to allocated block.\n");

    // 2. Query usable size (consulting Mnemosyne's size-class rounding)
    size_t usable = malloc_usable_size(ptr1);
    printf("[usable_size] malloc_usable_size(ptr) reported: %zu bytes\n", usable);
    if (usable >= size) {
        printf("[usable_size] Usable size is at least requested size.\n");
    } else {
        fprintf(stderr, "Error: Usable size is smaller than requested!\n");
        free(ptr1);
        return 1;
    }

    // 3. Perform realloc grow and check data preservation
    size_t new_size = 48;
    void *ptr2 = realloc(ptr1, new_size);
    if (!ptr2) {
        fprintf(stderr, "Error: realloc to %zu failed\n", new_size);
        free(ptr1);
        return 1;
    }
    printf("[realloc] Reallocated to %zu bytes at address: %p\n", new_size, ptr2);

    // Verify that the data was preserved (our 0xAA bytes)
    for (size_t i = 0; i < size; i++) {
        if (((unsigned char*)ptr2)[i] != 0xAA) {
            fprintf(stderr, "Error: Data corruption at index %zu during realloc!\n", i);
            free(ptr2);
            return 1;
        }
    }
    printf("[realloc] Realloc successfully preserved old data.\n");

    // Write new data
    memset(ptr2, 0xBB, new_size);

    // 4. Allocate with aligned_alloc (C11)
    size_t alignment = 64;
    size_t aligned_size = 128;
    void *ptr3 = aligned_alloc(alignment, aligned_size);
    if (!ptr3) {
        fprintf(stderr, "Error: aligned_alloc(%zu, %zu) failed\n", alignment, aligned_size);
        free(ptr2);
        return 1;
    }
    printf("[aligned_alloc] Allocated %zu bytes aligned to %zu at address: %p\n", aligned_size, alignment, ptr3);
    if ((size_t)ptr3 % alignment == 0) {
        printf("[aligned_alloc] Pointer is correctly aligned to %zu bytes.\n", alignment);
    } else {
        fprintf(stderr, "Error: Pointer is not %zu-byte aligned!\n", alignment);
        free(ptr2);
        free(ptr3);
        return 1;
    }

    // 5. Clean up
    free(ptr2);
    free(ptr3);
    printf("[free] Successfully released all allocations.\n");

    printf("=== Demo completed successfully! Mnemosyne is interposing correctly. ===\n");
    return 0;
}
