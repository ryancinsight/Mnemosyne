# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc |
| :--- | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 9661.456 | 496543.750 | 19221.667 | 0.02x | 0.50x |
| allocator burst retention/medium_1024 | 3546.796 | 88351.831 | 7641.486 | 0.04x | 0.46x |
| allocator burst retention/small_32 | 3301.404 | 905.230 | 4586.560 | 3.65x | 0.72x |
| allocator cycle latency/large_8192 | 13.682 | 16.807 | 18.783 | 0.81x | 0.73x |
| allocator cycle latency/medium_1024 | 13.687 | 5.641 | 17.946 | 2.43x | 0.76x |
| allocator cycle latency/small_32 | 13.376 | 2.759 | 16.855 | 4.85x | 0.79x |
| cross-thread free handoff/medium_1024 | 28150.049 | 177169.824 | 32876.782 | 0.16x | 0.86x |
| cross-thread free handoff/small_32 | 21480.640 | 18888.708 | 21158.472 | 1.14x | 1.02x |
| segment cache eviction | 61038.379 | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 237034.180 | 75996.143 | 275206.641 | 3.12x | 0.86x |
| threaded small allocation cycles | 23571.741 | 5012.689 | 25356.177 | 4.70x | 0.93x |
