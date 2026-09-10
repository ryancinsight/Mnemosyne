#![cfg(feature = "eunomia")]

use mnemosyne::scratch::{ScratchBank as ModuleScratchBank, ScratchPool as ModuleScratchPool};
use mnemosyne::{DEFAULT_SCRATCH_ALIGN, ScratchBank, ScratchPool};

#[test]
fn scratch_reexport_accepts_eunomia_complex() {
    let pool = ModuleScratchPool::<eunomia::Complex<f64>>::new();
    let bank = ModuleScratchBank::<eunomia::Complex<f64>, 2>::new();
    let root_pool = ScratchPool::<eunomia::Complex<f64>>::new();
    let root_bank = ScratchBank::<eunomia::Complex<f64>, 2>::new();

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

    bank.with_scratch::<0, _>(2, |scratch| {
        assert_eq!(scratch.len(), 2);
        assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
        scratch[0] = eunomia::Complex::new(4.0, 1.0);
        scratch[1] = eunomia::Complex::new(-1.0, 8.0);
    });

    root_pool.with_scratch(2, |scratch| {
        assert_eq!(scratch[0], eunomia::Complex::new(0.0, 0.0));
        scratch[0] = eunomia::Complex::new(12.0, 1.0);
        scratch[1] = eunomia::Complex::new(-7.0, 2.5);
    });

    root_bank.with_scratch::<1, _>(2, |scratch| {
        assert_eq!(scratch.len(), 2);
        scratch[0] = eunomia::Complex::new(8.0, -2.0);
        scratch[1] = eunomia::Complex::new(-4.0, 3.0);
    });
}
