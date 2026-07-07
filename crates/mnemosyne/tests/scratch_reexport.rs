#![cfg(feature = "eunomia")]

use mnemosyne::scratch::{DEFAULT_SCRATCH_ALIGN, ScratchPool};

#[test]
fn scratch_reexport_accepts_eunomia_complex() {
    let pool = ScratchPool::<eunomia::Complex<f64>>::new();

    pool.with_scratch(2, |scratch| {
        assert_eq!(scratch.len(), 2);
        assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
        assert_eq!(scratch[0], eunomia::Complex::new(0.0, 0.0));
        scratch[0] = eunomia::Complex::new(2.0, -3.0);
        scratch[1] = eunomia::Complex::new(-5.0, 7.5);
    });

    pool.with_scratch(2, |scratch| {
        assert_eq!(scratch[0], eunomia::Complex::new(2.0, -3.0));
        assert_eq!(scratch[1], eunomia::Complex::new(-5.0, 7.5));
    });
}
