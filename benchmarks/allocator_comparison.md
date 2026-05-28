# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 27.116 | 54.648 | 285.593 | 78.944 | N/A | 0.50x | 0.09x | 0.34x | N/A |
| allocator allocation latency/small_32 | 12.430 | 30.536 | 17.426 | 14.605 | N/A | 0.41x | 0.71x | 0.85x | N/A |
| allocator burst retention/large_8192 | 7246.274 | 12175.098 | 558314.062 | 21621.436 | N/A | 0.60x | 0.01x | 0.34x | N/A |
| allocator burst retention/medium_1024 | 2211.653 | 7992.914 | 101986.865 | 7684.222 | N/A | 0.28x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 2195.262 | 7644.434 | 1293.334 | 3978.256 | N/A | 0.29x | 1.70x | 0.55x | N/A |
| allocator cycle latency/large_8192 | 7.443 | 21.494 | 15.276 | 17.479 | N/A | 0.35x | 0.49x | 0.43x | N/A |
| allocator cycle latency/medium_1024 | 6.992 | 21.433 | 6.186 | 16.847 | N/A | 0.33x | 1.13x | 0.42x | N/A |
| allocator cycle latency/small_32 | 6.880 | 20.367 | 2.766 | 16.274 | N/A | 0.34x | 2.49x | 0.42x | N/A |
| allocator deallocation latency/medium_1024 | 22.458 | 31.095 | 129.086 | 64.022 | N/A | 0.72x | 0.17x | 0.35x | N/A |
| allocator deallocation latency/small_32 | 4.231 | 19.284 | 7.548 | 9.845 | N/A | 0.22x | 0.56x | 0.43x | N/A |
| cross-thread free handoff/medium_1024 | 21168.188 | 38093.152 | 182569.434 | 41369.971 | N/A | 0.56x | 0.12x | 0.51x | N/A |
| cross-thread free handoff/small_32 | 20722.424 | 38668.628 | 7168.394 | 23262.134 | N/A | 0.54x | 2.89x | 0.89x | N/A |
| realloc latency/cross_class_32_to_64 | 15.774 | 44.298 | 8.268 | 32.789 | N/A | 0.36x | 1.91x | 0.48x | N/A |
| realloc latency/within_class_24_to_32 | 7.017 | 47.912 | 4.517 | 17.194 | N/A | 0.15x | 1.55x | 0.41x | N/A |
| segment cache eviction | 65683.398 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 109288.153 | 399073.273 | 78954.164 | 273868.558 | N/A | 0.27x | 1.38x | 0.40x | N/A |
| threaded small allocation cycles | 16770.916 | 35870.386 | 6287.696 | 25923.289 | N/A | 0.47x | 2.67x | 0.65x | N/A |
| usable size latency/medium_1024 | 6.862 | N/A | 5.995 | 16.836 | N/A | N/A | 1.14x | 0.41x | N/A |
| usable size latency/small_32 | 6.869 | N/A | 2.827 | 16.438 | N/A | N/A | 2.43x | 0.42x | N/A |
| usable size query latency/medium_1024 | 0.427 | N/A | 0.729 | 0.683 | N/A | N/A | 0.59x | 0.63x | N/A |
| usable size query latency/small_32 | 0.419 | N/A | 0.744 | 0.449 | N/A | N/A | 0.56x | 0.93x | N/A |
