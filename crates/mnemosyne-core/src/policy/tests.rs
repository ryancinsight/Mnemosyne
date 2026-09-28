use super::*;

#[test]
fn standard_policy_has_no_mitigations() {
    assert_eq!(StandardPolicy::MITIGATION_FLAGS, mitigations::NONE);
    assert_eq!(StandardPolicy::POLICY_NAME, "standard");
    assert_eq!(StandardPolicy::POLICY_FINGERPRINT & 0x1F, 0);
}

#[test]
fn hardened_policy_has_all_implemented_mitigations() {
    assert_eq!(HardenedPolicy::POLICY_NAME, "hardened");
    assert_ne!(HardenedPolicy::MITIGATION_FLAGS, mitigations::NONE);
    const _: () = assert!(HardenedPolicy::ENABLE_POISONING);
    const _: () = assert!(HardenedPolicy::ENABLE_FREE_LIST_ENCRYPTION);
}

#[test]
fn policy_fingerprints_are_distinct() {
    assert_ne!(
        StandardPolicy::POLICY_FINGERPRINT,
        SecurePolicy::POLICY_FINGERPRINT
    );
    assert_ne!(
        StandardPolicy::POLICY_FINGERPRINT,
        HardenedPolicy::POLICY_FINGERPRINT
    );
    assert_ne!(
        SecurePolicy::POLICY_FINGERPRINT,
        HardenedPolicy::POLICY_FINGERPRINT
    );
}

#[test]
fn policy_fingerprint_encodes_key_flags() {
    assert_eq!(
        StandardPolicy::POLICY_FINGERPRINT & 1,
        StandardPolicy::ENABLE_POISONING as u64
    );
    assert_eq!(
        HardenedPolicy::POLICY_FINGERPRINT & 1,
        HardenedPolicy::ENABLE_POISONING as u64
    );
}

#[test]
fn policy_marker_is_zero_sized() {
    assert_eq!(core::mem::size_of::<PolicyMarker<StandardPolicy>>(), 0);
    assert_eq!(core::mem::size_of::<PolicyMarker<HardenedPolicy>>(), 0);
}
