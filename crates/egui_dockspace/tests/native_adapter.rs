//! Public native-feature integration smoke.
//!
//! Provider conformance is a unit-test child of the real private adapter module,
//! so this target deliberately does not recompile private production source.

#![cfg(feature = "native")]

#[test]
fn native_feature_preserves_the_headless_contract_export() {
    assert_eq!(egui_dockspace::dockspace::CONTRACT_VERSION, 1);
}
