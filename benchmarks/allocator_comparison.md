# Allocator Performance Comparison

| Benchmark | Mnemosyne (ns) | MiMalloc (ns) | SnMalloc (ns) | Jemalloc (ns) | Mnemosyne vs MiMalloc | Mnemosyne vs SnMalloc | Mnemosyne vs Jemalloc |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| allocator burst retention/large_8192 | 8298.115 | 498982.289 | 19293.517 | N/A | 0.02x | 0.43x | N/A |
| allocator burst retention/medium_1024 | 3638.792 | 91339.474 | 7141.375 | N/A | 0.04x | 0.51x | N/A |
| allocator burst retention/small_32 | 3457.070 | 1298.791 | 4051.253 | N/A | 2.66x | 0.85x | N/A |
| allocator cycle latency/large_8192 | 14.066 | 14.935 | 17.290 | N/A | 0.94x | 0.81x | N/A |
| allocator cycle latency/medium_1024 | 13.881 | 6.310 | 16.419 | N/A | 2.20x | 0.85x | N/A |
| allocator cycle latency/small_32 | 13.092 | 2.774 | 16.326 | N/A | 4.72x | 0.80x | N/A |
| cross-thread free handoff/medium_1024 | 26563.741 | 196390.907 | 40184.212 | N/A | 0.14x | 0.66x | N/A |
| cross-thread free handoff/small_32 | 23476.605 | 8076.841 | 23956.284 | N/A | 2.91x | 0.98x | N/A |
| segment cache eviction | 66318.325 | N/A | N/A | N/A | N/A | N/A | N/A |
| threaded saturated small allocation cycles | 210170.422 | 78921.266 | 272494.096 | N/A | 2.66x | 0.77x | N/A |
| threaded small allocation cycles | 22909.503 | 6207.192 | 26218.621 | N/A | 3.69x | 0.87x | N/A |
