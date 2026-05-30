# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 10.452 | 171.365 | 258.754 | 76.165 | 27.836 | 0.06x | 0.04x | 0.14x | 0.38x |
| allocator allocation latency/small_32 | 10.657 | 26.019 | 15.505 | 14.261 | 12.718 | 0.41x | 0.69x | 0.75x | 0.84x |
| allocator burst retention/large_8192 | 3799.619 | 9051.818 | 388868.033 | 19700.999 | 26607.355 | 0.42x | 0.01x | 0.19x | 0.14x |
| allocator burst retention/medium_1024 | 1295.636 | 6713.872 | 80462.807 | 7329.087 | 9248.398 | 0.19x | 0.02x | 0.18x | 0.14x |
| allocator burst retention/small_32 | 485.195 | 6378.425 | 917.502 | 4232.338 | 2677.250 | 0.08x | 0.53x | 0.11x | 0.18x |
| allocator cycle latency/large_8192 | 2.445 | 25.631 | 19.176 | 19.815 | 15.953 | 0.10x | 0.13x | 0.12x | 0.15x |
| allocator cycle latency/medium_1024 | 2.099 | 22.123 | 5.634 | 16.499 | 7.280 | 0.09x | 0.37x | 0.13x | 0.29x |
| allocator cycle latency/small_32 | 2.218 | 25.645 | 3.429 | 18.822 | 7.146 | 0.09x | 0.65x | 0.12x | 0.31x |
| allocator deallocation latency/medium_1024 | 36.193 | 64.311 | 98.592 | 70.683 | 18.466 | 0.56x | 0.37x | 0.51x | 1.96x |
| allocator deallocation latency/small_32 | 2.858 | 10.031 | 5.471 | 9.616 | 6.598 | 0.28x | 0.52x | 0.30x | 0.43x |
| cross-thread free handoff/medium_1024 | 10332.099 | 37471.467 | 173153.754 | 34378.992 | 49720.499 | 0.28x | 0.06x | 0.30x | 0.21x |
| cross-thread free handoff/small_32 | 14547.614 | 28120.049 | 15775.536 | 18478.729 | 28923.781 | 0.52x | 0.92x | 0.79x | 0.50x |
| realloc latency/cross_class_32_to_64 | 5.048 | 43.424 | 7.453 | 33.230 | 17.393 | 0.12x | 0.68x | 0.15x | 0.29x |
| realloc latency/huge_shrink_4m_to_2m | 31.445 | 18410.089 | 927149.630 | 1042452.930 | 245.676 | 0.00x | 0.00x | 0.00x | 0.13x |
| realloc latency/within_class_24_to_32 | 2.142 | 42.759 | 4.385 | 17.300 | 18.923 | 0.05x | 0.49x | 0.12x | 0.11x |
| segment cache eviction | 138936.262 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 37874.164 | 347853.535 | 61876.363 | 265137.768 | 144065.638 | 0.11x | 0.61x | 0.14x | 0.26x |
| threaded small allocation cycles | 3613.241 | 30872.928 | 5001.716 | 24071.795 | 15893.986 | 0.12x | 0.72x | 0.15x | 0.23x |
| usable size latency/medium_1024 | 2.259 | N/A | 6.039 | 16.805 | 13.004 | N/A | 0.37x | 0.13x | 0.17x |
| usable size latency/small_32 | 1.952 | N/A | 2.889 | 16.530 | 12.707 | N/A | 0.68x | 0.12x | 0.15x |
| usable size query latency/medium_1024 | 0.404 | N/A | 0.654 | 0.550 | 3.784 | N/A | 0.62x | 0.73x | 0.11x |
| usable size query latency/small_32 | 0.317 | N/A | 0.526 | 0.454 | 3.385 | N/A | 0.60x | 0.70x | 0.09x |
