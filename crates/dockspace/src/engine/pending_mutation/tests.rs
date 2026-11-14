use super::*;
use crate::ids::EngineAuthorityDomainId;

fn emissions() -> [HostFrameKey; 3] {
    let mut ledger = PresentationLedger::new(EngineAuthorityDomainId::new_for_test(0xface));
    let host = ledger.create_host().unwrap();
    [SurfaceId::new(1), SurfaceId::new(1), SurfaceId::new(2)].map(|surface| {
        ledger
            .emit(
                host,
                surface,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .unwrap()
            .key()
    })
}

#[test]
fn failed_mutations_cannot_be_revived_by_late_outputs_or_presentations() {
    let [first, second, foreign] = emissions();
    for was_presented in [false, true] {
        let mut progress = PresentationProgress::waiting([first]);
        if was_presented {
            progress.observe_presented(first);
        }
        progress.fail();
        progress.record_output(second);
        progress.observe_presented(second);
        progress.retire_keys(&BTreeSet::from([first, second, foreign]));
        assert!(progress.is_failed());
        assert!(!progress.is_presented());
        assert!(!progress.awaiting_presentation());
        assert!(!progress.needs_paint());
        let mut retained = BTreeSet::new();
        progress.retain_emissions(&mut retained);
        assert!(retained.is_empty());
    }
}

#[test]
fn retiring_a_subset_cannot_fail_a_mutation_with_another_exact_output_pending() {
    let [first, second, foreign] = emissions();
    let mut progress = PresentationProgress::waiting([first, second]);
    progress.retire_keys(&BTreeSet::from([first, foreign]));
    assert!(progress.awaiting_presentation());
    assert!(!progress.needs_paint());
    let mut retained = BTreeSet::new();
    progress.retain_emissions(&mut retained);
    assert_eq!(retained, BTreeSet::from([second]));
    progress.observe_presented(second);
    progress.retire_keys(&BTreeSet::from([second]));
    assert!(progress.is_presented());
    assert!(!progress.is_failed());

    let mut progress = PresentationProgress::waiting([first, second]);
    progress.retire_keys(&BTreeSet::from([first]));
    progress.retire_keys(&BTreeSet::from([second]));
    assert!(progress.is_failed());
}

#[test]
fn an_unpainted_mutation_waits_for_new_output_instead_of_failing_on_unrelated_retirement() {
    let [first, second, foreign] = emissions();
    let mut progress = PresentationProgress::default();
    progress.retire_keys(&BTreeSet::from([foreign]));
    assert!(progress.needs_paint());
    progress.record_output(first);
    progress.record_output(second);
    assert!(!progress.needs_paint());
    progress.retire_keys(&BTreeSet::from([first]));
    assert!(progress.awaiting_presentation());
    progress.observe_presented(second);
    assert!(progress.is_presented());
}
