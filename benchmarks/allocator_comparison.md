# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 26.024 | 75.493 | 290.351 | 93.565 | N/A | 0.34x | 0.09x | 0.28x | N/A |
| allocator allocation latency/small_32 | 12.350 | 29.948 | 18.615 | 15.082 | N/A | 0.41x | 0.66x | 0.82x | N/A |
| allocator burst retention/large_8192 | 5622.776 | 8919.487 | 410741.357 | 19644.174 | N/A | 0.63x | 0.01x | 0.29x | N/A |
| allocator burst retention/medium_1024 | 1504.114 | 6600.552 | 81580.083 | 7877.891 | N/A | 0.23x | 0.02x | 0.19x | N/A |
| allocator burst retention/small_32 | 503.747 | 6327.817 | 842.635 | 4252.063 | N/A | 0.08x | 0.60x | 0.12x | N/A |
| allocator cycle latency/large_8192 | 1.991 | 20.216 | 16.882 | 17.391 | N/A | 0.10x | 0.12x | 0.11x | N/A |
| allocator cycle latency/medium_1024 | 2.007 | 20.499 | 5.689 | 16.705 | N/A | 0.10x | 0.35x | 0.12x | N/A |
| allocator cycle latency/small_32 | 1.904 | 20.312 | 2.750 | 16.372 | N/A | 0.09x | 0.69x | 0.12x | N/A |
| allocator deallocation latency/medium_1024 | 25.166 | 33.127 | 79.168 | 46.063 | N/A | 0.76x | 0.32x | 0.55x | N/A |
| allocator deallocation latency/small_32 | 4.179 | 12.841 | 5.625 | 9.944 | N/A | 0.33x | 0.74x | 0.42x | N/A |
| cross-thread free handoff/medium_1024 | 18411.148 | 32430.069 | 150561.599 | 38220.083 | N/A | 0.57x | 0.12x | 0.48x | N/A |
| cross-thread free handoff/small_32 | 15455.481 | 28156.860 | 15983.409 | 19130.429 | N/A | 0.55x | 0.97x | 0.81x | N/A |
| realloc latency/cross_class_32_to_64 | 5.248 | 43.579 | 7.466 | 33.549 | N/A | 0.12x | 0.70x | 0.16x | N/A |
| realloc latency/huge_shrink_4m_to_2m | 25583.671 | 22846.408 | 1047789.013 | 1034248.028 | N/A | 1.12x | 0.02x | 0.02x | N/A |
| realloc latency/within_class_24_to_32 | 2.201 | 43.190 | 4.389 | 17.237 | N/A | 0.05x | 0.50x | 0.13x | N/A |
| segment cache eviction | 147232.377 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 39343.188 | 352463.639 | 64363.887 | 263359.388 | N/A | 0.11x | 0.61x | 0.15x | N/A |
| threaded small allocation cycles | 3723.482 | 29705.077 | 4841.033 | 27060.833 | N/A | 0.13x | 0.77x | 0.14x | N/A |
| usable size latency/medium_1024 | 2.163 | N/A | 6.023 | 17.687 | N/A | N/A | 0.36x | 0.12x | N/A |
| usable size latency/small_32 | 2.057 | N/A | 2.909 | 16.553 | N/A | N/A | 0.71x | 0.12x | N/A |
| usable size query latency/medium_1024 | 0.314 | N/A | 0.524 | 0.458 | N/A | N/A | 0.60x | 0.69x | N/A |
| usable size query latency/small_32 | 0.478 | N/A | 0.537 | 0.460 | N/A | N/A | 0.89x | 1.04x | N/A |
