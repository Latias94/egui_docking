//! Independent transferred children must not block each other's explicit recovery.

use super::*;
use crate::frame::{BindingRetirementOrigin, NativeCreatePhase};
use crate::runtime::{
    HostInputOutcome, NativeCreateRecoveryBlockedReason, NativeCreateRecoveryStep,
};
use crate::viewport_registry::ViewportAdmission;

const SECOND_HOST: SurfaceId = SurfaceId::new(2);
const SECOND_ROOT: RootId = RootId::new(3);
const FIRST_MAIN: RootId = RootId::new(2);
const SECOND_MAIN: RootId = RootId::new(4);
const FIRST_MAIN_ITEM: ItemId = ItemId::new(3);
const THIRD_ITEM: ItemId = ItemId::new(4);
const FOURTH_ITEM: ItemId = ItemId::new(5);
const SECOND_MAIN_ITEM: ItemId = ItemId::new(6);

fn second_host_facts() -> NativeWindowFacts {
    root_window_facts()
        .with_content_bounds(PhysicalRect::new(800.0, 80.0, 640.0, 480.0).unwrap())
        .with_outer_bounds(PhysicalRect::new(792.0, 50.0, 656.0, 518.0).unwrap())
}

#[inline(never)]
fn observe_all(
    session: &mut DockspaceSession,
    hosts: [NativeSurfaceBinding; 2],
    first_host: NativeWindowFacts,
    children: &[(NativeSurfaceBinding, NativeWindowFacts)],
) {
    session
        .report_managed_native_snapshot(
            [(hosts[0], first_host), (hosts[1], second_host_facts())]
                .into_iter()
                .chain(children.iter().copied()),
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the complete two-host inventory records");
    let mut report = commit_managed_frame_report(session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
}

#[inline(never)]
fn present_both_staging(
    session: &mut DockspaceSession,
    children: [NativeSurfaceBinding; 2],
    phase: NativeStagingPresentationPhase,
) -> HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let requests = frame.native_staging_paints().collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert!(children.contains(&request.binding()));
        assert_eq!(request.phase(), phase);
        frame.confirm_native_staging_painted(request).unwrap();
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    let outputs = report.take_painted_native_staging_outputs();
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        session
            .report_native_staging_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let report = commit_managed_frame_report(session);
    assert!(report.native_admissions().is_empty());
    report
}

#[inline(never)]
fn multiple_transferred_children() -> (
    DockspaceSession,
    [NativeSurfaceBinding; 2],
    [(NativeSurfaceBinding, NativeWindowFacts); 2],
) {
    let mut builder = Workspace::builder();
    for (root, items) in [
        (ROOT, vec![ITEM, SECOND_ITEM]),
        (FIRST_MAIN, vec![FIRST_MAIN_ITEM]),
        (SECOND_ROOT, vec![THIRD_ITEM, FOURTH_ITEM]),
        (SECOND_MAIN, vec![SECOND_MAIN_ITEM]),
    ] {
        let tabs = builder.insert_node(Node::tabs(items));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    }
    for (surface, main, root, floating) in [
        (SURFACE, FIRST_MAIN, ROOT, FloatingPresentationId::new(1)),
        (
            SECOND_HOST,
            SECOND_MAIN,
            SECOND_ROOT,
            FloatingPresentationId::new(2),
        ),
    ] {
        builder.set_surface(
            surface,
            SurfacePresentation {
                main_root: Some(main),
                contained: vec![floating],
            },
        );
        builder.set_contained_floating(
            floating,
            ContainedFloating::new(root, LogicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap()),
        );
    }
    let (mut session, first_host) = managed_session_from_workspace(builder.build().unwrap());
    session
        .register_native_root(SECOND_HOST, HostWindowToken::new(42))
        .unwrap();
    let report = commit_managed_frame_report(&mut session);
    let [
        HostInputOutcome::NativeSurfaceRegistered {
            binding: second_host,
        },
    ] = report.inputs()
    else {
        panic!(
            "the second independent native root must register: {:?}",
            report.inputs()
        );
    };
    let hosts = [first_host, *second_host];
    observe_all(&mut session, hosts, root_window_facts(), &[]);
    let report = paint_and_present_all_native_surfaces(&mut session);
    assert!(report.native_admissions().is_empty());

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    for (root, x) in [(ROOT, 200.0), (SECOND_ROOT, 1000.0)] {
        frame
            .move_root_to_new_window_current(
                root,
                NativeWindowPlacement::new(PhysicalRect::new(x, 620.0, 420.0, 320.0).unwrap()),
            )
            .unwrap();
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    let creates = report.take_native_effects();
    assert_eq!(creates.len(), 2, "tear-off outcomes: {:?}", report.inputs());
    let mut observed = Vec::new();
    for create in creates {
        let NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } = create.operation()
        else {
            panic!("each public tear-off must request a real native child");
        };
        let (binding, placement) = (*binding, *placement);
        let Some(NativeEffectAcknowledgement::Presentation(created)) = create.accepted() else {
            panic!("each create must return its exact acknowledgement");
        };
        observed.push((
            binding,
            NativeWindowFacts::live()
                .with_content_bounds(placement)
                .with_outer_bounds(placement)
                .with_native_scale_factor(native_scale())
                .with_presentation_scale_factor(native_scale())
                .with_input(NativeWindowInputState::ReceivesInput, None)
                .with_close(NativeCloseState::Clear, None)
                .with_presentation(NativeWindowPresentationState::Hidden, Some(created)),
        ));
    }
    let mut observed: [_; 2] = observed.try_into().unwrap();
    let children = observed.map(|(binding, _)| binding);
    observe_all(&mut session, hosts, root_window_facts(), &observed);
    let mut report = present_both_staging(
        &mut session,
        children,
        NativeStagingPresentationPhase::PreShow,
    );
    let shows = report.take_native_effects();
    assert_eq!(shows.len(), 2);
    for show in shows {
        let NativeEffectOperation::ShowWindow { binding } = show.operation() else {
            panic!("both real pre-show receipts must request ShowWindow");
        };
        let index = children.iter().position(|child| child == binding).unwrap();
        let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
            panic!("each show must return its exact acknowledgement");
        };
        observed[index].1 = observed[index]
            .1
            .with_presentation(NativeWindowPresentationState::Hidden, Some(shown));
    }
    // Exact show acknowledgements precede the later visibility boundary.
    observe_all(&mut session, hosts, root_window_facts(), &observed);
    for (_, facts) in &mut observed {
        *facts = facts.with_presentation(NativeWindowPresentationState::Visible, None);
    }
    observe_all(&mut session, hosts, root_window_facts(), &observed);
    let mut report = present_both_staging(
        &mut session,
        children,
        NativeStagingPresentationPhase::PostShow,
    );
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 2);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        2
    );
    for (index, items) in [[ITEM, SECOND_ITEM], [THIRD_ITEM, FOURTH_ITEM]]
        .into_iter()
        .enumerate()
    {
        let child = children[index];
        let saga = session
            .engine
            .viewport()
            .native_create_sagas()
            .find_map(|(_, saga)| (saga.binding() == child.binding).then_some(saga))
            .unwrap();
        assert!(matches!(
            saga.phase(),
            NativeCreatePhase::AwaitingFirstLivePresentation { .. }
        ));
        assert_eq!(
            saga.prepared()
                .recovery_obligation()
                .target()
                .host_surface(),
            hosts[index].surface()
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .viewport(child.surface())
                .unwrap()
                .admission(),
            ViewportAdmission::Pending
        );
        for item in items {
            assert_eq!(
                session.view().item(item).unwrap().surface(),
                child.surface()
            );
        }
    }
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
    (session, hosts, observed)
}

fn assert_applied(
    step: NativeCreateRecoveryStep,
    child: NativeSurfaceBinding,
    host: SurfaceId,
    report: &HostFrameReport,
) {
    let NativeCreateRecoveryStep::Applied {
        submitted,
        surface,
        host: actual_host,
    } = step
    else {
        panic!("expected exact child recovery, got {step:?}");
    };
    assert_eq!((surface, actual_host), (child.surface(), host));
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeCreateFirstLiveRecovered { surface, host })
    );
    assert!(report.native_admissions().is_empty());
}

fn assert_cleanup(
    session: &DockspaceSession,
    report: &mut HostFrameReport,
    children: &[NativeSurfaceBinding],
    delivered_children: &[NativeSurfaceBinding],
) {
    let mut retired = session
        .engine
        .viewport()
        .binding_retirements()
        .map(|(binding, retirement)| {
            assert_eq!(retirement.origin(), BindingRetirementOrigin::SurfaceVacated);
            binding
        })
        .collect::<Vec<_>>();
    let mut expected = children
        .iter()
        .map(|child| child.binding)
        .collect::<Vec<_>>();
    retired.sort();
    expected.sort();
    assert_eq!(retired, expected);
    let effects = report.take_native_effects();
    assert_eq!(effects.len(), delivered_children.len());
    let mut released = Vec::new();
    for effect in effects {
        let NativeEffectOperation::ReleaseChild { binding } = effect.operation() else {
            panic!("recovery must release exactly the vacated children");
        };
        assert!(delivered_children.contains(binding));
        released.push(binding.binding);
        assert!(matches!(
            effect.accepted(),
            Some(NativeEffectAcknowledgement::Close(_))
        ));
    }
    released.sort();
    let mut delivered = delivered_children
        .iter()
        .map(|child| child.binding)
        .collect::<Vec<_>>();
    delivered.sort();
    assert_eq!(released, delivered);
    // Conservation also checks each retained resource's exact BindingCleanup owner.
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
}

#[test]
fn native_create_recovery_blocked_first_host_does_not_starve_ready_second_host() {
    let (mut session, hosts, observed) = multiple_transferred_children();
    let [first, second] = observed.map(|(binding, _)| binding);
    measure_all_surfaces(&mut session);
    let stale = root_window_facts()
        .with_content_bounds(
            PhysicalRect::new(ROOT_ORIGIN_X + 20.0, ROOT_ORIGIN_Y, 640.0, 480.0).unwrap(),
        )
        .with_outer_bounds(
            PhysicalRect::new(ROOT_ORIGIN_X + 12.0, ROOT_ORIGIN_Y - 30.0, 656.0, 518.0).unwrap(),
        );
    observe_all(&mut session, hosts, stale, &observed);
    assert!(
        session
            .engine
            .scene()
            .ready_candidate(hosts[0].surface())
            .is_none()
    );
    assert!(
        session
            .engine
            .scene()
            .ready_candidate(hosts[1].surface())
            .is_some()
    );
    let first_saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .find_map(|(_, saga)| (saga.binding() == first.binding).then_some(saga.clone()))
        .unwrap();
    let items = session.engine.workspace().item_multiset();
    let roots = session
        .engine
        .workspace()
        .roots()
        .map(|(id, root)| (id, root.clone()))
        .collect::<Vec<_>>();
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let sequence = session.committed_source_sequence;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert_eq!(
        frame.native_create_recovery_surfaces().collect::<Vec<_>>(),
        vec![first.surface(), second.surface()]
    );
    let blocked = frame
        .recover_transferred_native_create(first.surface())
        .unwrap();
    let NativeCreateRecoveryStep::Blocked {
        submitted,
        surface,
        host,
        reason,
    } = blocked
    else {
        panic!("stale first-host coordinates must block A: {blocked:?}");
    };
    assert_eq!(
        (surface, host, reason),
        (
            first.surface(),
            hosts[0].surface(),
            NativeCreateRecoveryBlockedReason::HostAuthorityUnavailable
        )
    );
    let applied = frame
        .recover_transferred_native_create(second.surface())
        .unwrap();
    assert_eq!(
        frame.native_create_recovery_surfaces().collect::<Vec<_>>(),
        vec![first.surface()]
    );
    assert!(frame.view().surface(first.surface()).is_some());
    assert!(frame.view().surface(second.surface()).is_none());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeCreateFirstLiveRecoveryBlocked {
            surface,
            host,
            reason
        })
    );
    assert_applied(applied, second, hosts[1].surface(), &report);
    assert_eq!(session.committed_source_sequence, sequence + 2);
    assert_eq!(
        session
            .engine
            .viewport()
            .native_create_saga(first_saga.id()),
        Some(&first_saga)
    );
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 1);
    for item in [ITEM, SECOND_ITEM] {
        assert_eq!(
            session.view().item(item).unwrap().surface(),
            first.surface()
        );
    }
    for item in [THIRD_ITEM, FOURTH_ITEM] {
        let location = session.view().item(item).unwrap();
        assert_eq!(
            (location.surface(), location.root()),
            (hosts[1].surface(), SECOND_ROOT)
        );
        assert!(location.contained().is_some());
    }
    assert_eq!(session.engine.workspace().item_multiset(), items);
    assert_eq!(
        session
            .engine
            .workspace()
            .roots()
            .map(|(id, root)| (id, root.clone()))
            .collect::<Vec<_>>(),
        roots
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    assert_cleanup(&session, &mut report, &[second], &[second]);
}

#[test]
fn native_create_recovery_two_hosts_refresh_measurements_between_exact_cleanup_commits() {
    let (mut session, hosts, observed) = multiple_transferred_children();
    let children = observed.map(|(binding, _)| binding);
    measure_all_surfaces(&mut session);
    for host in hosts {
        assert!(
            session
                .engine
                .scene()
                .ready_candidate(host.surface())
                .is_some()
        );
    }
    let items = session.engine.workspace().item_multiset();
    let roots = session
        .engine
        .workspace()
        .roots()
        .map(|(id, root)| (id, root.clone()))
        .collect::<Vec<_>>();
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let sequence = session.committed_source_sequence;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert_eq!(
        frame.native_create_recovery_surfaces().collect::<Vec<_>>(),
        children.map(|child| child.surface())
    );
    let first = frame
        .recover_transferred_native_create(children[0].surface())
        .unwrap();
    assert!(matches!(first, NativeCreateRecoveryStep::Applied { .. }));
    assert_eq!(
        frame.native_create_recovery_surfaces().collect::<Vec<_>>(),
        vec![children[1].surface()]
    );
    // A's saga and Pending barrier survive until commit. Removing its surface
    // also changes the global popup roster, so B needs a fresh measured ticket.
    assert_eq!(
        frame.frame.view().viewport().native_create_sagas().count(),
        2
    );
    assert_eq!(
        frame
            .frame
            .view()
            .viewport()
            .viewport(children[0].surface())
            .unwrap()
            .admission(),
        ViewportAdmission::Pending
    );
    let first_location = frame.view().item(ITEM).unwrap();
    let restored_floating = first_location.contained().unwrap();
    let restored = frame
        .frame
        .view()
        .workspace()
        .contained_floating(restored_floating)
        .unwrap()
        .clone();
    let second = frame
        .recover_transferred_native_create(children[1].surface())
        .unwrap();
    assert!(
        matches!(
            second,
            NativeCreateRecoveryStep::Blocked {
                reason: NativeCreateRecoveryBlockedReason::HostAuthorityUnavailable,
                ..
            }
        ),
        "second recovery: {second:?}"
    );
    assert_eq!(
        frame
            .frame
            .view()
            .workspace()
            .contained_floating(restored_floating),
        Some(&restored)
    );
    assert_eq!(
        frame.native_create_recovery_surfaces().collect::<Vec<_>>(),
        vec![children[1].surface()]
    );
    assert!(frame.view().surface(children[0].surface()).is_none());
    assert!(frame.view().surface(children[1].surface()).is_some());
    assert_eq!(frame.next_source_sequence, sequence + 2);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_applied(first, children[0], hosts[0].surface(), &report);
    assert_eq!(session.committed_source_sequence, sequence + 2);
    assert_cleanup(&session, &mut report, &children[..1], &children[..1]);

    // No first-live paint is fabricated: one current measurement boundary is
    // sufficient to retry B through the same exact durable obligation.
    measure_all_surfaces(&mut session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let second = frame
        .recover_transferred_native_create(children[1].surface())
        .unwrap();
    assert!(
        matches!(second, NativeCreateRecoveryStep::Applied { .. }),
        "second recovery after measurement: {second:?}"
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_applied(second, children[1], hosts[1].surface(), &report);
    assert_eq!(session.committed_source_sequence, sequence + 3);
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 0);
    assert_eq!(session.engine.viewport().pending_recoveries().count(), 0);
    for (host, root, item) in [
        (hosts[0], ROOT, ITEM),
        (hosts[0], ROOT, SECOND_ITEM),
        (hosts[1], SECOND_ROOT, THIRD_ITEM),
        (hosts[1], SECOND_ROOT, FOURTH_ITEM),
    ] {
        let location = session.view().item(item).unwrap();
        assert_eq!(
            (location.surface(), location.root()),
            (host.surface(), root)
        );
        assert!(location.contained().is_some());
    }
    assert_eq!(session.engine.workspace().item_multiset(), items);
    assert_eq!(
        session
            .engine
            .workspace()
            .roots()
            .map(|(id, root)| (id, root.clone()))
            .collect::<Vec<_>>(),
        roots
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    assert_cleanup(&session, &mut report, &children, &children[1..]);
}
