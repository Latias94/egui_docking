//! Concurrent native creates retain their sources until exact first-live admission.

use super::*;
use crate::platform::frame::{BindingRetirementOrigin, NativeCreatePhase};
use crate::platform::registry::ViewportAdmission;
use crate::runtime::{HostInputOutcome, SurfacePaintDisposition, SurfacePaintProgramOperation};

const SECOND_HOST: SurfaceId = SurfaceId::new(2);
const SECOND_ROOT: RootId = RootId::new(3);
const FIRST_MAIN: RootId = RootId::new(2);
const SECOND_MAIN: RootId = RootId::new(4);
const FIRST_MAIN_ITEM: ItemId = ItemId::new(3);
const THIRD_ITEM: ItemId = ItemId::new(4);
const FOURTH_ITEM: ItemId = ItemId::new(5);
const SECOND_MAIN_ITEM: ItemId = ItemId::new(6);
const SOURCE_ROOTS: [RootId; 2] = [ROOT, SECOND_ROOT];
const SOURCE_ITEMS: [[ItemId; 2]; 2] = [[ITEM, SECOND_ITEM], [THIRD_ITEM, FOURTH_ITEM]];
const SOURCE_FLOATINGS: [FloatingPresentationId; 2] = [
    FloatingPresentationId::new(1),
    FloatingPresentationId::new(2),
];

fn second_host_facts() -> NativeWindowFacts {
    root_window_facts()
        .with_content_bounds(PhysicalRect::new(800.0, 80.0, 640.0, 480.0).unwrap())
        .with_outer_bounds(PhysicalRect::new(792.0, 50.0, 656.0, 518.0).unwrap())
}

#[inline(never)]
fn observe_all(
    session: &mut DockspaceSession,
    hosts: [NativeSurfaceBinding; 2],
    host_facts: [NativeWindowFacts; 2],
    children: &[(NativeSurfaceBinding, NativeWindowFacts)],
) {
    session
        .report_managed_native_snapshot(
            hosts
                .into_iter()
                .zip(host_facts)
                .chain(children.iter().copied()),
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the complete two-host inventory records");
    let mut report = commit_managed_frame_report(session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
}

#[inline(never)]
fn multiple_pending_children() -> (
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
    let host_facts = [root_window_facts(), second_host_facts()];
    observe_all(&mut session, hosts, host_facts, &[]);
    let report = paint_and_present_all_native_surfaces(&mut session);
    assert!(report.native_admissions().is_empty());
    let source = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;

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
        let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
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
                .with_presentation(NativeWindowPresentationState::Hidden)
                .with_create_readiness(
                    crate::runtime::NativeCreateReadiness::Created,
                    Some(created),
                ),
        ));
    }
    let mut observed: [_; 2] = observed.try_into().unwrap();
    let children = observed.map(|(binding, _)| binding);
    observe_all(&mut session, hosts, host_facts, &observed);
    for (_, facts) in &mut observed {
        *facts = facts
            .with_presentation(NativeWindowPresentationState::Visible)
            .with_create_readiness(crate::runtime::NativeCreateReadiness::Prepared, None);
    }
    observe_all(&mut session, hosts, host_facts, &observed);
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence + 2);
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 2);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        2
    );
    for (index, child) in children.into_iter().enumerate() {
        let saga = session
            .engine
            .viewport()
            .native_create_sagas()
            .find_map(|(_, saga)| (saga.binding() == child.binding).then_some(saga))
            .unwrap();
        assert_eq!(
            saga.prepared()
                .recovery_obligation()
                .target()
                .host_surface(),
            hosts[index].surface()
        );
        assert_pending_source(&session, child, index);
    }
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
    (session, hosts, observed)
}

fn assert_pending_source(session: &DockspaceSession, child: NativeSurfaceBinding, index: usize) {
    let surface = [SURFACE, SECOND_HOST][index];
    for item in SOURCE_ITEMS[index] {
        let source = session.view().item(item).unwrap();
        assert_eq!(
            (source.surface(), source.root(), source.contained()),
            (surface, SOURCE_ROOTS[index], Some(SOURCE_FLOATINGS[index])),
            "the pending child must not publish source ownership for {item:?}"
        );
    }
    let source = session.engine.workspace().surface(surface).unwrap();
    assert_eq!(source.main_root, Some([FIRST_MAIN, SECOND_MAIN][index]));
    assert_eq!(source.contained, [SOURCE_FLOATINGS[index]]);
    assert!(
        session
            .engine
            .workspace()
            .surface(child.surface())
            .is_none()
    );
    assert!(session.presented_surface(child.surface()).is_none());
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .expect("the pending child retains its exact binding")
            .admission(),
        ViewportAdmission::Pending
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .any(|(_, saga)| {
                saga.binding() == child.binding
                    && matches!(
                        saga.phase(),
                        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                    )
            })
    );
}

fn assert_admitted_child(session: &DockspaceSession, child: NativeSurfaceBinding, index: usize) {
    for item in SOURCE_ITEMS[index] {
        let current = session.view().item(item).unwrap();
        assert_eq!(
            (current.surface(), current.root(), current.contained()),
            (child.surface(), SOURCE_ROOTS[index], None)
        );
    }
    let surface = session.engine.workspace().surface(child.surface()).unwrap();
    assert_eq!(surface.main_root, Some(SOURCE_ROOTS[index]));
    assert!(surface.contained.is_empty());
    let host = session
        .engine
        .workspace()
        .surface([SURFACE, SECOND_HOST][index])
        .unwrap();
    assert_eq!(host.main_root, Some([FIRST_MAIN, SECOND_MAIN][index]));
    assert!(host.contained.is_empty());
    assert!(
        session
            .engine
            .workspace()
            .contained_floating(SOURCE_FLOATINGS[index])
            .is_none()
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .unwrap()
            .admission(),
        ViewportAdmission::Admitted
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .all(|(_, saga)| saga.binding() != child.binding)
    );
}

#[inline(never)]
fn paint_pending_children(
    session: &mut DockspaceSession,
    children: [NativeSurfaceBinding; 2],
    pending: &[usize],
) -> Vec<PaintedSurfaceOutput> {
    let source = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let mut measure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the concurrent first-live measurement frame begins");
    let surfaces = measure.surfaces();
    assert_eq!(surfaces.len(), 4);
    for surface in [
        SURFACE,
        SECOND_HOST,
        children[0].surface(),
        children[1].surface(),
    ] {
        assert!(surfaces.contains(&surface));
    }
    for surface in surfaces {
        let (width, height) = if children.iter().any(|child| child.surface() == surface) {
            (420.0, 320.0)
        } else {
            (640.0, 480.0)
        };
        measure
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, width, height).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .expect("every published host and prospective child measures");
    }
    measure
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = measure.commit().expect("the current measurements commit");
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence);

    let mut paint = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the concurrent first-live paint frame begins");
    let mut semantics = Vec::new();
    for &index in pending {
        let child = children[index];
        let selected = SOURCE_ITEMS[index]
            .into_iter()
            .find(|item| paint.view().item(*item).unwrap().is_selected())
            .expect("the source pane has one selected item");
        let plan = paint
            .paint_plan(child.surface())
            .expect("the real child plan resolves through the public facade")
            .expect("the measured child projection is paintable");
        assert_eq!(plan.surface(), child.surface());
        let mut roots = Vec::new();
        let mut panes = Vec::new();
        let mut tabs = Vec::new();
        let mut semantic_output = None;
        let receipt = plan
            .execute_surface_program(|step| {
                match step {
                    SurfacePaintProgramStep::RootBegin { root, paint } => {
                        assert_eq!(root.root(), SOURCE_ROOTS[index]);
                        assert!(root.contained().is_none());
                        assert_eq!(paint, SurfacePaintDisposition::Visible);
                        roots.push(root.root());
                    }
                    SurfacePaintProgramStep::Operation {
                        root,
                        operation,
                        paint,
                    } => {
                        assert_eq!(root.root(), SOURCE_ROOTS[index]);
                        assert_eq!(paint, SurfacePaintDisposition::Visible);
                        match operation {
                            SurfacePaintProgramOperation::Pane(pane) => {
                                assert_eq!(
                                    pane.bounds(),
                                    LogicalRect::new(0.0, 0.0, 420.0, 320.0).unwrap()
                                );
                                panes.push((pane.root(), pane.selected()));
                            }
                            SurfacePaintProgramOperation::Tab(tab) => {
                                tabs.push((tab.root(), tab.item(), tab.selected()));
                            }
                            _ => {}
                        }
                    }
                    SurfacePaintProgramStep::SurfaceEnd {
                        semantic_output: output,
                    } => {
                        assert!(semantic_output.replace(output).is_none());
                    }
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .expect("the renderer accepts the complete real child full-pane program");
        assert_eq!(roots, [SOURCE_ROOTS[index]]);
        assert_eq!(panes, [(SOURCE_ROOTS[index], Some(selected))]);
        assert_eq!(tabs.len(), 2);
        for item in SOURCE_ITEMS[index] {
            assert!(tabs.contains(&(SOURCE_ROOTS[index], item, item == selected)));
        }
        semantics.push((
            child,
            semantic_output.expect("the complete child program seals"),
        ));
        paint.confirm_surface_painted(receipt).unwrap();
    }
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = paint.commit().expect("the exact child paint outputs emit");
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), pending.len());
    let outputs = semantics
        .into_iter()
        .map(|(child, semantic_output)| {
            let index = outputs
                .iter()
                .position(|output| output.surface() == child.surface())
                .expect("each real child emits its own exact output");
            let output = outputs.remove(index);
            assert!(output.matches_native_binding(child));
            assert!(output.matches_semantic_output(semantic_output));
            output
        })
        .collect();
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence);
    for &index in pending {
        assert_pending_source(session, children[index], index);
    }
    outputs
}

#[test]
fn concurrent_first_live_projections_measure_and_paint_without_moving_either_source() {
    let (mut session, _, observed) = multiple_pending_children();
    let children = observed.map(|(child, _)| child);
    let source = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let sagas = session
        .engine
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.clone()))
        .collect::<Vec<_>>();
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let outputs = paint_pending_children(&mut session, children, &[0, 1]);
    let artifacts = outputs
        .iter()
        .map(|output| output.presentation_artifact())
        .collect::<Vec<_>>();

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    for (index, child) in children.into_iter().enumerate() {
        assert_pending_source(&session, child, index);
    }

    drop(outputs);
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(report.surface_presentation_commits().len(), 2);
    for artifact in artifacts {
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact))
        );
    }
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .map(|(id, saga)| (id, saga.clone()))
            .collect::<Vec<_>>(),
        sagas
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
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
}

#[test]
fn first_live_admission_invalidates_the_other_old_output_until_fresh_measure_and_paint() {
    let (mut session, hosts, observed) = multiple_pending_children();
    let children = observed.map(|(child, _)| child);
    let source = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let [first, stale_second] = paint_pending_children(&mut session, children, &[0, 1])
        .try_into()
        .unwrap_or_else(|outputs: Vec<_>| panic!("both real children must emit: {outputs:?}"));
    let first_artifact = first.presentation_artifact();
    let stale_artifact = stale_second.presentation_artifact();
    session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .expect("only the exact first child output is presented");
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    for (index, child) in children.into_iter().enumerate() {
        assert_pending_source(&session, child, index);
    }

    let ingress = session.engine.backend_ingress_committed_through();
    let mut candidate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first exact admission remains rollbackable until frame commit");
    for item in SOURCE_ITEMS[0] {
        let current = candidate.view().item(item).unwrap();
        assert_eq!(
            (current.surface(), current.root(), current.contained()),
            (children[0].surface(), ROOT, None)
        );
    }
    for item in SOURCE_ITEMS[1] {
        let current = candidate.view().item(item).unwrap();
        assert_eq!(
            (current.surface(), current.root(), current.contained()),
            (hosts[1].surface(), SECOND_ROOT, Some(SOURCE_FLOATINGS[1]))
        );
    }
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(candidate);
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.engine.backend_ingress_committed_through(), ingress);

    let mut report = commit_managed_frame_report(&mut session);
    assert_eq!(report.native_admissions(), &[children[0]]);
    assert!(report.take_native_effects().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the retry must settle exactly the first child's retained output");
    };
    assert!(presentation.presents(first_artifact));
    assert_admitted_child(&session, children[0], 0);
    assert_pending_source(&session, children[1], 1);
    assert_eq!(session.version().epoch(), version.epoch());
    assert_eq!(
        session.version().revision().get(),
        version.revision().get() + 1
    );
    assert!(
        session
            .presented_surface(children[0].surface())
            .unwrap()
            .matches_presentation_artifact(first_artifact)
    );
    let first_version = session.version();
    let after_first = session.engine.workspace().clone();
    let first_surface = after_first.surface(children[0].surface()).unwrap().clone();
    let first_root = after_first.root(ROOT).unwrap().clone();
    let second_saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.clone()))
        .collect::<Vec<_>>();
    assert_eq!(second_saga.len(), 1);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        1
    );

    session
        .report_surface_presentation(stale_second, SurfacePresentationResult::Presented)
        .expect("the old second output still receives its exact terminal disposition");
    assert_eq!(session.engine.workspace(), &after_first);
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the stale second output must settle without publishing its projection");
    };
    assert!(presentation.retires(stale_artifact));
    assert_eq!(session.engine.workspace(), &after_first);
    assert_eq!(session.version(), first_version);
    assert_admitted_child(&session, children[0], 0);
    assert_pending_source(&session, children[1], 1);
    assert_eq!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .map(|(id, saga)| (id, saga.clone()))
            .collect::<Vec<_>>(),
        second_saga
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        1
    );

    // Settling the old terminal grants no permission to move B on a later quiet frame.
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_eq!(session.engine.workspace(), &after_first);
    let [fresh_second] = paint_pending_children(&mut session, children, &[1])
        .try_into()
        .unwrap_or_else(|outputs: Vec<_>| {
            panic!("B must emit one fresh child output: {outputs:?}")
        });
    let fresh_artifact = fresh_second.presentation_artifact();
    assert_ne!(fresh_artifact, stale_artifact);
    assert_eq!(session.engine.workspace(), &after_first);
    session
        .report_surface_presentation(fresh_second, SurfacePresentationResult::Presented)
        .expect("the freshly measured and completely painted B output is presented");
    assert_pending_source(&session, children[1], 1);
    assert_eq!(session.version(), first_version);
    let mut report = commit_managed_frame_report(&mut session);
    assert_eq!(report.native_admissions(), &[children[1]]);
    let [focus] = report
        .take_native_effects()
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| {
            panic!("the winning admitted child must request its exact focus: {effects:?}")
        });
    assert!(matches!(
        focus.operation(),
        NativeEffectOperation::RequestFocus { binding } if *binding == children[1]
    ));
    assert!(focus.accepted().is_none());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the fresh second output must settle exactly once");
    };
    assert!(presentation.presents(fresh_artifact));
    assert!(!presentation.presents(stale_artifact));
    assert_eq!(session.version().epoch(), version.epoch());
    assert_eq!(
        session.version().revision().get(),
        version.revision().get() + 2
    );
    for (index, child) in children.into_iter().enumerate() {
        assert_admitted_child(&session, child, index);
    }
    assert_eq!(
        session.engine.workspace().surface(children[0].surface()),
        Some(&first_surface)
    );
    assert_eq!(session.engine.workspace().root(ROOT), Some(&first_root));
    assert_eq!(
        session.engine.workspace().item_multiset(),
        source.item_multiset()
    );
    assert_eq!(
        session.engine.workspace().roots().collect::<Vec<_>>(),
        source.roots().collect::<Vec<_>>()
    );
    for (index, host) in hosts.into_iter().enumerate() {
        let main = [FIRST_MAIN, SECOND_MAIN][index];
        assert_eq!(session.engine.workspace().root(main), source.root(main));
        let item = session
            .view()
            .item([FIRST_MAIN_ITEM, SECOND_MAIN_ITEM][index])
            .unwrap();
        assert_eq!(
            (item.surface(), item.root(), item.contained()),
            (host.surface(), main, None)
        );
    }
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 0);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        0
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
}

#[test]
fn cancelling_first_live_heads_retries_without_moving_sources_or_leaking_children() {
    let (mut session, hosts, observed) = multiple_pending_children();
    let children = observed.map(|(child, _)| child);
    // Revoke both source measurements; cancellation must not require recovery geometry.
    let host_facts = [
        root_window_facts()
            .with_content_bounds(
                PhysicalRect::new(ROOT_ORIGIN_X + 20.0, ROOT_ORIGIN_Y, 640.0, 480.0).unwrap(),
            )
            .with_outer_bounds(
                PhysicalRect::new(ROOT_ORIGIN_X + 12.0, ROOT_ORIGIN_Y - 30.0, 656.0, 518.0)
                    .unwrap(),
            ),
        second_host_facts()
            .with_content_bounds(PhysicalRect::new(820.0, 80.0, 640.0, 480.0).unwrap())
            .with_outer_bounds(PhysicalRect::new(812.0, 50.0, 656.0, 518.0).unwrap()),
    ];
    observe_all(&mut session, hosts, host_facts, &observed);
    for host in hosts {
        assert!(
            session
                .engine
                .scene()
                .ready_candidate(host.surface())
                .is_none()
        );
    }
    let source = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let sagas = session
        .engine
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.clone()))
        .collect::<Vec<_>>();
    assert_eq!(sagas.len(), 2);
    for (index, (_, saga)) in sagas.iter().enumerate() {
        assert_eq!(saga.binding(), children[index].binding);
    }
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();

    for index in 0..2 {
        let child = children[index];
        let (saga_id, saga) = &sagas[index];
        let before_sequence = session.committed_source_sequence;
        let ingress = session.engine.backend_ingress_committed_through();
        let mut rollback = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the head cancellation candidate begins without source remeasurement");
        assert!(rollback.cancel_next_native_create().unwrap());
        assert_eq!(rollback.next_source_sequence, before_sequence + 1);
        assert_eq!(rollback.frame.view().workspace(), &source);
        assert_eq!(rollback.version(), version);
        assert_eq!(
            rollback
                .frame
                .view()
                .viewport()
                .native_create_sagas()
                .map(|(id, saga)| (id, saga.clone()))
                .collect::<Vec<_>>(),
            sagas[index + 1..]
        );
        assert!(
            rollback
                .frame
                .view()
                .viewport()
                .viewport(child.surface())
                .is_none()
        );
        rollback
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        drop(rollback);

        assert_eq!(session.engine.workspace(), &source);
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, before_sequence);
        assert_eq!(session.engine.backend_ingress_committed_through(), ingress);
        assert_eq!(
            session
                .engine
                .viewport()
                .native_create_sagas()
                .map(|(id, saga)| (id, saga.clone()))
                .collect::<Vec<_>>(),
            sagas[index..]
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            resources[index..]
        );
        assert_eq!(session.engine.viewport().binding_retirements().count(), 0);
        for pending in index..2 {
            assert_pending_source(&session, children[pending], pending);
        }

        let mut retry = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the same exact head cancellation retries after rollback");
        assert!(retry.cancel_next_native_create().unwrap());
        assert_eq!(retry.next_source_sequence, before_sequence + 1);
        retry
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = retry
            .commit()
            .expect("one cancellation input commits for this head");
        assert!(report.native_admissions().is_empty());
        assert!(report.presentation_transitions().is_empty());
        assert_eq!(session.engine.workspace(), &source);
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, before_sequence + 1);
        assert!(
            session
                .engine
                .viewport()
                .native_create_saga(*saga_id)
                .is_none()
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .native_create_sagas()
                .map(|(id, saga)| (id, saga.clone()))
                .collect::<Vec<_>>(),
            sagas[index + 1..]
        );
        let retirements = session
            .engine
            .viewport()
            .binding_retirements()
            .collect::<Vec<_>>();
        assert_eq!(retirements.len(), 1);
        assert_eq!(retirements[0].0, child.binding);
        assert_eq!(
            retirements[0].1.origin(),
            BindingRetirementOrigin::NativeCreateAborted {
                create: saga.create()
            }
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            resources[index..]
        );
        session
            .engine
            .viewport()
            .validate_native_staging_resource_conservation()
            .unwrap();
        for pending in index + 1..2 {
            assert_pending_source(&session, children[pending], pending);
        }

        // Settle the actual compensation through its original owner before the next head.
        let compensation = take_only_native_effect(&mut report);
        assert!(matches!(
            compensation.operation(),
            NativeEffectOperation::CompensatingClose { binding } if *binding == child
        ));
        let Some(NativeEffectAcknowledgement::Close(closed)) = compensation.accepted() else {
            panic!("the original compensation must return its exact close acknowledgement");
        };
        let mut quiet = commit_managed_frame_report(&mut session);
        assert!(quiet.native_admissions().is_empty());
        assert!(quiet.take_native_effects().is_empty());
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            resources[index..]
        );
        let remaining = [(child, NativeWindowFacts::destroyed_after(closed))]
            .into_iter()
            .chain(observed[index + 1..].iter().copied())
            .collect::<Vec<_>>();
        observe_all(&mut session, hosts, host_facts, &remaining);
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            resources[index + 1..]
        );
        assert_eq!(session.engine.workspace(), &source);
        for pending in index + 1..2 {
            assert_pending_source(&session, children[pending], pending);
        }
        session
            .report_native_binding_quiescence(child)
            .expect("the exactly destroyed child has no remaining external callbacks");
        let mut report = commit_managed_frame_report(&mut session);
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert_eq!(session.engine.viewport().binding_retirements().count(), 0);
        assert_eq!(session.engine.workspace(), &source);
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, before_sequence + 1);
        session
            .engine
            .viewport()
            .validate_native_staging_resource_conservation()
            .unwrap();
    }

    let mut empty = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!empty.cancel_next_native_create().unwrap());
    assert_eq!(empty.next_source_sequence, sequence + 2);
    empty
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = empty.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence + 2);
    assert_eq!(session.engine.viewport().native_create_sagas().count(), 0);
    assert_eq!(session.engine.viewport().pending_recoveries().count(), 0);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        0
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .bindings()
            .terminal_binding_guards(),
        0
    );
}
