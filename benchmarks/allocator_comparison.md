# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | System (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs System | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator allocation latency/medium_1024 | 27.116 | 54.648 | 285.593 | 78.944 | N/A | 0.50x | 0.09x | 0.34x | N/A |
| allocator allocation latency/small_32 | 12.430 | 30.536 | 17.426 | 14.605 | N/A | 0.41x | 0.71x | 0.85x | N/A |
| allocator burst retention/large_8192 | 7246.274 | 12175.098 | 558314.062 | 21621.436 | N/A | 0.60x | 0.01x | 0.34x | N/A |
| allocator burst retention/medium_1024 | 2211.653 | 7992.914 | 101986.865 | 7684.222 | N/A | 0.28x | 0.02x | 0.29x | N/A |
| allocator burst retention/small_32 | 2195.262 | 7644.434 | 1293.334 | 3978.256 | N/A | 0.29x | 1.70x | 0.55x | N/A |
| allocator cycle latency/large_8192 | 7.009 | 20.611 | 16.905 | 17.255 | N/A | 0.34x | 0.41x | 0.41x | N/A |
| allocator cycle latency/medium_1024 | 7.066 | 22.034 | 5.682 | 16.876 | N/A | 0.32x | 1.24x | 0.42x | N/A |
| allocator cycle latency/small_32 | 6.968 | 22.881 | 2.824 | 16.289 | N/A | 0.30x | 2.47x | 0.43x | N/A |
| allocator deallocation latency/medium_1024 | 93.029 | 114.997 | 133.881 | 100.081 | N/A | 0.81x | 0.69x | 0.93x | N/A |
| allocator deallocation latency/small_32 | 4.717 | 21.295 | 6.614 | 9.745 | N/A | 0.22x | 0.71x | 0.48x | N/A |
| cross-thread free handoff/medium_1024 | 21168.188 | 38093.152 | 182569.434 | 41369.971 | N/A | 0.56x | 0.12x | 0.51x | N/A |
| cross-thread free handoff/small_32 | 20722.424 | 38668.628 | 7168.394 | 23262.134 | N/A | 0.54x | 2.89x | 0.89x | N/A |
| realloc latency/cross_class_32_to_64 | 15.774 | 44.298 | 8.268 | 32.789 | N/A | 0.36x | 1.91x | 0.48x | N/A |
| realloc latency/within_class_24_to_32 | 7.017 | 47.912 | 4.517 | 17.194 | N/A | 0.15x | 1.55x | 0.41x | N/A |
| segment cache eviction | 65683.398 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 76284.499 | 346191.423 | 60994.491 | 262260.247 | N/A | 0.22x | 1.25x | 0.29x | N/A |
| threaded small allocation cycles | 17372.615 | 31828.715 | 4683.371 | 24298.651 | N/A | 0.55x | 3.71x | 0.71x | N/A |
| usable size latency/medium_1024 | 6.862 | N/A | 5.995 | 16.836 | N/A | N/A | 1.14x | 0.41x | N/A |
| usable size latency/small_32 | 6.869 | N/A | 2.827 | 16.438 | N/A | N/A | 2.43x | 0.42x | N/A |
| usable size query latency/medium_1024 | 0.427 | N/A | 0.729 | 0.683 | N/A | N/A | 0.59x | 0.63x | N/A |
| usable size query latency/small_32 | 0.419 | N/A | 0.744 | 0.449 | N/A | N/A | 0.56x | 0.93x | N/A |
