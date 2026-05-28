# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 7742.619 | 462496.628 | 18227.238 | 0.02x | 0.42x |
| allocator burst retention/medium_1024 | 3424.632 | 78852.993 | 7309.833 | 0.04x | 0.47x |
| allocator burst retention/small_32 | 3198.899 | 895.707 | 4268.775 | 3.57x | 0.75x |
| allocator cycle latency/large_8192 | 13.268 | 16.947 | 17.385 | 0.78x | 0.76x |
| allocator cycle latency/medium_1024 | 13.240 | 5.958 | 16.752 | 2.22x | 0.79x |
| allocator cycle latency/small_32 | 12.828 | 2.753 | 16.286 | 4.66x | 0.79x |
| cross-thread free handoff/medium_1024 | 22138.866 | 141860.861 | 35268.820 | 0.16x | 0.63x |
| cross-thread free handoff/small_32 | 18159.403 | 15722.105 | 19621.558 | 1.16x | 0.93x |
| segment cache eviction | 57928.962 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 202808.398 | 59430.713 | 268690.430 | 3.41x | 0.75x |
| threaded small allocation cycles | 18359.716 | 4494.536 | 29740.542 | 4.08x | 0.62x |
