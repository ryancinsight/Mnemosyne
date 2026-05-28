# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8248.501 | 430304.320 | 19389.191 | 0.02x | 0.43x |
| allocator burst retention/medium_1024 | 3627.403 | 77976.429 | 7380.772 | 0.05x | 0.49x |
| allocator burst retention/small_32 | 3426.597 | 927.354 | 4097.597 | 3.70x | 0.84x |
| allocator cycle latency/large_8192 | 13.579 | 16.287 | 17.309 | 0.83x | 0.78x |
| allocator cycle latency/medium_1024 | 13.508 | 5.901 | 16.500 | 2.29x | 0.82x |
| allocator cycle latency/small_32 | 13.150 | 2.744 | 16.226 | 4.79x | 0.81x |
| cross-thread free handoff/medium_1024 | 27199.845 | 184255.616 | 37552.934 | 0.15x | 0.72x |
| cross-thread free handoff/small_32 | 22931.878 | 6666.087 | 22549.239 | 3.44x | 1.02x |
| segment cache eviction | 66081.935 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 212409.178 | 76664.478 | 266514.556 | 2.77x | 0.80x |
| threaded small allocation cycles | 21256.847 | 6211.866 | 25032.966 | 3.42x | 0.85x |
