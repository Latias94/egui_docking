//! Native document restoration continues several real Roots in one atomic replacement.

#![cfg(feature = "serde")]

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceHostFrame,
    DockspaceNativeSurfaceHosting, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostWindowToken, HostWorkAreaToken, NativeCreateReadiness, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeHostCapabilities, NativeHostCapability, NativePointerRoster,
    NativeReceiverAnswer, NativeRootContinuation, NativeSurfaceBinding, NativeSurfaceRole,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x44; 16]);
const A: SurfaceId = SurfaceId::new(10);
const B: SurfaceId = SurfaceId::new(20);
const EXTRA: SurfaceId = SurfaceId::new(90);
// Children sort before both recovery Roots, so iteration order cannot choose their host.
const CHILDREN: [SurfaceId; 2] = [SurfaceId::new(2), SurfaceId::new(4)];
const ROOTS: [SurfaceId; 2] = [A, B];
const ROOT_IDS: [RootId; 2] = [RootId::new(10), RootId::new(20)];
const CHILD_ROOT_IDS: [RootId; 2] = [RootId::new(2), RootId::new(4)];
const ROOT_ITEMS: [[ItemId; 2]; 2] = [
    [ItemId::new(1), ItemId::new(2)],
    [ItemId::new(3), ItemId::new(4)],
];
const CHILD_ITEMS: [ItemId; 2] = [ItemId::new(5), ItemId::new(6)];
const KEYS: [&str; 6] = [
    "a:first", "a:second", "b:first", "b:second", "a:child", "b:child",
];
const ROOT_TOKENS: [HostWindowToken; 3] = [
    HostWindowToken::new(1),
    HostWindowToken::new(2),
    HostWindowToken::new(3),
];

fn policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy
}

fn layout(with_children: bool, extra_root: bool) -> DockspaceLayout {
    let mut surfaces = ROOTS
        .into_iter()
        .enumerate()
        .map(|(index, surface)| {
            DockspaceSurfaceLayout::new(
                surface,
                DockspaceRootLayout::new(
                    ROOT_IDS[index],
                    DockspaceNode::central_tabs(ROOT_ITEMS[index]),
                ),
            )
        })
        .collect::<Vec<_>>();
    if with_children {
        surfaces.extend(CHILDREN.into_iter().enumerate().map(|(index, surface)| {
            DockspaceSurfaceLayout::new(
                surface,
                DockspaceRootLayout::new(
                    CHILD_ROOT_IDS[index],
                    DockspaceNode::tabs([CHILD_ITEMS[index]]),
                ),
            )
        }));
    }
    if extra_root {
        surfaces.push(DockspaceSurfaceLayout::new(
            EXTRA,
            DockspaceRootLayout::new(RootId::new(90), DockspaceNode::central_tabs([])),
        ));
    }
    DockspaceLayout::new(surfaces).unwrap()
}

fn native_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut report = finish(native_frame(session));
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    report
}

fn child_bounds(index: usize) -> PhysicalRect {
    PhysicalRect::new(180.0 + index as f64 * 900.0, 100.0, 400.0, 300.0).unwrap()
}

fn window_bounds(binding: NativeSurfaceBinding) -> PhysicalRect {
    if let Some(index) = ROOT_TOKENS
        .iter()
        .position(|&token| token == binding.window_token())
    {
        PhysicalRect::new(index as f64 * 900.0, 0.0, 800.0, 600.0).unwrap()
    } else {
        child_bounds(
            CHILDREN
                .iter()
                .position(|&surface| surface == binding.surface())
                .unwrap(),
        )
    }
}

fn window_facts(binding: NativeSurfaceBinding) -> NativeWindowFacts {
    let bounds = window_bounds(binding);
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 3200.0, 1080.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )])
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (PaintedSurfaceOutput, RecordingRenderer) {
    let bounds = window_bounds(binding);
    let mut frame = native_frame(session);
    frame
        .measure_surface(
            binding.surface(),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, bounds.width(), bounds.height()).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.paint_plan(binding.surface()).unwrap().unwrap();
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    (output, recording)
}

struct Host {
    session: DockspaceSession,
    roots: Vec<NativeSurfaceBinding>,
    children: Vec<NativeSurfaceBinding>,
}

impl Host {
    fn new(with_children: bool, extra_root: bool) -> Self {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        for (index, key) in KEYS.into_iter().enumerate() {
            assert_eq!(
                bootstrap.ensure_item(key).unwrap(),
                ItemId::new(index as u64 + 1)
            );
        }
        let mut session = DockspaceSession::from_persistent_layout(
            layout(with_children, extra_root),
            policy(),
            bootstrap,
        )
        .unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        let capabilities = [
            NativeHostCapability::NativeWindowLifecycle,
            NativeHostCapability::AuthoritativeInventory,
            NativeHostCapability::GlobalWindowPlacement,
            NativeHostCapability::WorkArea,
            NativeHostCapability::PointerHitTestObservation,
            NativeHostCapability::CloseCancellation,
        ]
        .into_iter()
        .fold(
            NativeHostCapabilities::none_supported(),
            |caps, capability| caps.with(capability),
        );
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        let root_surfaces = if extra_root {
            vec![A, B, EXTRA]
        } else {
            vec![A, B]
        };
        for (index, surface) in root_surfaces.into_iter().enumerate() {
            session
                .register_native_root(surface, ROOT_TOKENS[index])
                .unwrap();
        }
        let roots = idle(&mut session).native_bindings().to_vec();
        let mut host = Self {
            session,
            roots,
            children: Vec::new(),
        };
        if with_children {
            for (index, surface) in CHILDREN.into_iter().enumerate() {
                host.session
                    .register_owned_native_child(
                        surface,
                        HostWindowToken::new(100 + index as u64),
                        ROOTS[index],
                    )
                    .unwrap();
                let report = idle(&mut host.session);
                host.children.push(
                    *report
                        .native_bindings()
                        .iter()
                        .find(|binding| binding.surface() == surface)
                        .unwrap(),
                );
            }
        }
        host.observe();
        host.present_roots();
        host
    }

    fn observations(&self) -> Vec<(NativeSurfaceBinding, NativeWindowFacts)> {
        self.roots
            .iter()
            .copied()
            .map(|binding| (binding, window_facts(binding)))
            .chain(self.children.iter().copied().map(|binding| {
                (
                    binding,
                    window_facts(binding)
                        .with_create_readiness(NativeCreateReadiness::Prepared, None),
                )
            }))
            .collect()
    }

    fn observe(&mut self) {
        self.session
            .report_managed_native_snapshot(self.observations(), work_areas())
            .unwrap();
        idle(&mut self.session);
    }

    fn present_roots(&mut self) {
        for binding in self.roots.clone() {
            let (output, _) = paint(&mut self.session, binding);
            self.session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            idle(&mut self.session);
        }
    }

    fn install_continuations(&mut self, continuations: &[NativeRootContinuation]) {
        self.roots = continuations.iter().map(|pair| pair.successor()).collect();
        self.roots.sort_unstable_by_key(|binding| binding.surface());
        for pair in continuations {
            assert!(!self.session.is_current_native_binding(pair.predecessor()));
            assert!(self.session.is_current_native_binding(pair.successor()));
            assert!(
                self.session
                    .native_local_coordinates(pair.successor())
                    .is_none()
            );
            self.session
                .report_native_binding_quiescence(pair.predecessor())
                .unwrap();
        }
        idle(&mut self.session);
        idle(&mut self.session);
        self.observe();
        self.present_roots();
    }
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && KEYS.contains(&key)
}

fn saved_document() -> Vec<u8> {
    Host::new(true, false).session.save_document_json().unwrap()
}

fn assert_swapped(continuations: &[NativeRootContinuation], predecessors: &[NativeSurfaceBinding]) {
    assert_eq!(continuations.len(), 2);
    for (index, predecessor) in predecessors.iter().copied().enumerate() {
        let pair = continuations
            .iter()
            .find(|pair| pair.predecessor() == predecessor)
            .unwrap();
        assert_eq!(pair.successor().surface(), ROOTS[1 - index]);
        assert_eq!(pair.successor().window_token(), predecessor.window_token());
        assert_ne!(pair.successor(), predecessor);
    }
}

#[test]
fn two_external_roots_swap_atomically_and_children_recover_to_their_own_anchors() {
    let bytes = saved_document();
    let mut host = Host::new(false, false);
    let predecessors = host.roots.clone();
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    restore
        .validate_native_surface_hosting_for_roots([B, A])
        .unwrap();
    for index in 0..2 {
        let preference = restore
            .surface_presentation_preferences()
            .find(|preference| preference.surface() == CHILDREN[index])
            .unwrap();
        assert_eq!(
            preference.native_hosting(),
            Some(DockspaceNativeSurfaceHosting::Child {
                recovery_host: ROOTS[index]
            })
        );
        assert_eq!(preference.outer_rect(), Some(child_bounds(index)));
    }
    let before = host.session.version();
    let mut frame = native_frame(&mut host.session);
    frame
        .submit_native_document_restore_with_roots(
            &restore,
            [(predecessors[0], B), (predecessors[1], A)],
        )
        .unwrap();
    let proposed = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert_swapped(&proposed, &predecessors);
    let bindings = frame.proposed_native_bindings().collect::<Vec<_>>();
    let preferences = frame.surface_presentation_preferences().collect::<Vec<_>>();
    for index in 0..2 {
        assert_eq!(
            frame.view().item(ROOT_ITEMS[index][0]).unwrap().surface(),
            ROOTS[index]
        );
        assert_eq!(
            frame.view().item(CHILD_ITEMS[index]).unwrap().surface(),
            CHILDREN[index]
        );
    }
    let mut report = finish(frame);
    assert!(
        matches!(report.inputs(), [HostInputOutcome::DocumentRestored { previous, current }] if *previous == before && *current == report.after())
    );
    assert!(!host.session.version().is_same_epoch(before));
    assert_eq!(report.native_root_continuations(), proposed);
    assert_eq!(report.native_bindings(), bindings);
    assert_eq!(
        host.session
            .surface_presentation_preferences()
            .collect::<Vec<_>>(),
        preferences
    );
    assert!(report.take_native_effects().is_empty());
    assert!(report.native_admissions().is_empty());
    assert!(host.session.has_pending_native_create());
    host.install_continuations(&proposed);
    let restored_version = host.session.version();

    for index in 0..2 {
        let mut frame = native_frame(&mut host.session);
        assert!(frame.advance_next_native_surface_create().unwrap());
        let mut report = finish(frame);
        let [create] = report.take_native_effects().try_into().unwrap();
        let binding = match create.operation() {
            NativeEffectOperation::CreateWindow {
                binding,
                placement,
                role: NativeSurfaceRole::Child,
            } => {
                assert_eq!(binding.surface(), CHILDREN[index]);
                assert_eq!(*placement, child_bounds(index));
                *binding
            }
            operation => panic!("one restored Child must emit one exact Create: {operation:?}"),
        };
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = create.accepted() else {
            panic!("the Child Create must own its readiness acknowledgement");
        };
        let mut observations = host.observations();
        observations.push((
            binding,
            window_facts(binding)
                .with_create_readiness(NativeCreateReadiness::Prepared, Some(acknowledgement)),
        ));
        host.session
            .report_managed_native_snapshot(observations, work_areas())
            .unwrap();
        assert!(idle(&mut host.session).native_admissions().is_empty());
        host.children.push(binding);
        let (output, recording) = paint(&mut host.session, binding);
        assert!(recording.records.iter().any(|record| matches!(record, OwnedRecord::PaneContent { item: Some(item), .. } if *item == CHILD_ITEMS[index])));
        host.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut host.session);
        assert_eq!(report.native_admissions(), &[binding]);
        assert!(!report.workspace_changed());
        assert_eq!(host.session.version(), restored_version);
    }
    assert!(!host.session.has_pending_native_create());

    let mut observations = host
        .roots
        .iter()
        .copied()
        .map(|binding| (binding, window_facts(binding)))
        .collect::<Vec<_>>();
    observations.extend(
        host.children
            .iter()
            .copied()
            .map(|binding| (binding, NativeWindowFacts::destroyed())),
    );
    host.session
        .report_managed_native_snapshot(observations, work_areas())
        .unwrap();
    let report = idle(&mut host.session);
    assert!(report.workspace_changed());
    for index in 0..2 {
        let recovered = host.session.view().item(CHILD_ITEMS[index]).unwrap();
        assert_eq!(recovered.surface(), ROOTS[index]);
        assert!(recovered.contained().is_some());
        assert!(host.session.view().surface(CHILDREN[index]).is_none());
        assert!(!host.session.is_current_native_binding(host.children[index]));
    }
}

#[test]
fn dropped_multi_root_restore_retries_the_same_complete_continuation_batch() {
    let bytes = saved_document();
    let mut host = Host::new(false, false);
    let predecessors = host.roots.clone();
    let before = host.session.version();
    let graph = format!("{:?}", host.session.view());
    let generation = host.session.next_document_generation();
    let recorded = host.session.native_input_recorded_count();
    let coordinates = predecessors
        .iter()
        .map(|&binding| host.session.native_local_coordinates(binding))
        .collect::<Vec<_>>();
    let preferences = host
        .session
        .surface_presentation_preferences()
        .collect::<Vec<_>>();
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut exact = None;
    for attempt in 0..2 {
        let mut frame = native_frame(&mut host.session);
        frame
            .submit_native_document_restore_with_roots(
                &restore,
                [(predecessors[0], B), (predecessors[1], A)],
            )
            .unwrap();
        let proposed = frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>();
        assert_swapped(&proposed, &predecessors);
        assert_eq!(exact.get_or_insert_with(|| proposed.clone()), &proposed);
        if attempt == 0 {
            drop(frame);
            assert_eq!(host.session.version(), before);
            assert_eq!(format!("{:?}", host.session.view()), graph);
            assert_eq!(host.session.next_document_generation(), generation);
            assert_eq!(
                host.session
                    .surface_presentation_preferences()
                    .collect::<Vec<_>>(),
                preferences
            );
            assert!(!host.session.has_pending_native_create());
            for (index, &binding) in predecessors.iter().enumerate() {
                assert!(host.session.is_current_native_binding(binding));
                assert_eq!(
                    host.session.native_local_coordinates(binding),
                    coordinates[index]
                );
            }
            assert!(
                proposed
                    .iter()
                    .all(|pair| !host.session.recognizes_native_binding(pair.successor()))
            );
        } else {
            let mut report = finish(frame);
            assert_eq!(report.native_root_continuations(), proposed);
            assert!(report.take_native_effects().is_empty());
            assert!(!host.session.version().is_same_epoch(before));
            assert!(host.session.has_pending_native_create());
        }
        assert_eq!(host.session.native_input_recorded_count(), recorded);
    }
}

#[test]
fn multi_root_hosting_requires_the_complete_distinct_declared_root_set() {
    let bytes = saved_document();
    let host = Host::new(false, false);
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    restore
        .validate_native_surface_hosting_for_roots([A, B])
        .unwrap();
    for invalid in [vec![], vec![A], vec![A, A], vec![CHILDREN[0], B]] {
        assert!(
            restore
                .validate_native_surface_hosting_for_roots(invalid)
                .is_err()
        );
    }
}

fn assert_mapping_rejected_without_replacing_the_accepted_prefix(
    host: &mut Host,
    bytes: &[u8],
    mappings: &[(NativeSurfaceBinding, SurfaceId)],
) {
    let restore = host
        .session
        .prepare_document_restore_json(bytes, recognize)
        .unwrap();
    let before = host.session.version();
    let generation = host.session.next_document_generation();
    let recorded = host.session.native_input_recorded_count();
    let preferences = host
        .session
        .surface_presentation_preferences()
        .collect::<Vec<_>>();
    let bindings = idle(&mut host.session).native_bindings().to_vec();
    let mut frame = native_frame(&mut host.session);
    frame.select_item_current(ROOT_ITEMS[0][1]).unwrap();
    let prefix = frame.version();
    assert_ne!(prefix, before);
    assert!(
        frame
            .submit_native_document_restore_with_roots(&restore, mappings.iter().copied())
            .is_err()
    );
    assert_eq!(frame.version(), prefix);
    assert!(frame.proposed_native_root_continuations().next().is_none());
    assert!(frame.proposed_native_bindings().next().is_none());
    assert!(frame.view().item(ROOT_ITEMS[0][1]).unwrap().is_selected());
    assert_eq!(frame.view().surfaces().count(), bindings.len());
    frame.select_item_current(ROOT_ITEMS[1][1]).unwrap();
    let mut report = finish(frame);
    assert!(report.native_root_continuations().is_empty());
    assert_eq!(report.native_bindings(), bindings);
    assert!(
        bindings
            .iter()
            .all(|&binding| host.session.is_current_native_binding(binding))
    );
    assert!(
        report
            .inputs()
            .iter()
            .all(|input| !matches!(input, HostInputOutcome::DocumentRestored { .. }))
    );
    assert!(report.take_native_effects().is_empty());
    assert!(host.session.version().is_same_epoch(before));
    assert!(
        host.session
            .view()
            .item(ROOT_ITEMS[0][1])
            .unwrap()
            .is_selected()
    );
    assert!(
        host.session
            .view()
            .item(ROOT_ITEMS[1][1])
            .unwrap()
            .is_selected()
    );
    assert_eq!(host.session.next_document_generation(), generation);
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    assert_eq!(
        host.session
            .surface_presentation_preferences()
            .collect::<Vec<_>>(),
        preferences
    );
}

#[test]
fn duplicate_foreign_and_nonparticipating_root_mappings_reject_atomically() {
    let bytes = saved_document();
    let foreign = Host::new(false, false).roots[0];
    for case in 0..4 {
        let mut host = Host::new(false, case == 3);
        let a = host.roots[0];
        let b = host.roots[1];
        let mappings = match case {
            0 => [(a, A), (a, B)],
            1 => [(a, A), (b, A)],
            2 => [(foreign, A), (b, B)],
            3 => [(host.roots[2], A), (a, B)],
            _ => unreachable!(),
        };
        assert_mapping_rejected_without_replacing_the_accepted_prefix(&mut host, &bytes, &mappings);
    }
}

#[test]
fn stale_predecessor_cannot_continue_a_successor_in_a_later_restore() {
    let bytes = saved_document();
    let mut host = Host::new(false, false);
    let predecessor = host.roots[0];
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut frame = native_frame(&mut host.session);
    frame
        .submit_native_document_restore_with_roots(
            &restore,
            [(host.roots[0], A), (host.roots[1], B)],
        )
        .unwrap();
    let report = finish(frame);
    host.roots = ROOTS
        .into_iter()
        .map(|surface| {
            report
                .native_bindings()
                .iter()
                .copied()
                .find(|binding| binding.surface() == surface)
                .unwrap()
        })
        .collect();
    assert!(!host.session.is_current_native_binding(predecessor));
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let before = host.session.version();
    let graph = format!("{:?}", host.session.view());
    let generation = host.session.next_document_generation();
    let recorded = host.session.native_input_recorded_count();
    let mut frame = native_frame(&mut host.session);
    assert!(
        frame
            .submit_native_document_restore_with_roots(
                &restore,
                [(predecessor, A), (host.roots[1], B)]
            )
            .is_err()
    );
    assert_eq!(frame.version(), before);
    assert!(frame.proposed_native_root_continuations().next().is_none());
    let mut rejected = finish(frame);
    assert_eq!(rejected.native_bindings(), report.native_bindings());
    assert!(rejected.inputs().is_empty());
    assert!(rejected.take_native_effects().is_empty());
    assert_eq!(host.session.version(), before);
    assert_eq!(format!("{:?}", host.session.view()), graph);
    assert_eq!(host.session.next_document_generation(), generation);
    assert_eq!(host.session.native_input_recorded_count(), recorded);
}

#[test]
fn multi_root_layout_reset_swaps_a_complete_cycle_only_when_the_retry_commits() {
    let mut host = Host::new(false, false);
    let predecessors = host.roots.clone();
    let before = host.session.version();
    let graph = format!("{:?}", host.session.view());
    let generation = host.session.next_document_generation();
    let reset = host
        .session
        .prepare_layout_reset(layout(false, false))
        .unwrap();
    let mut exact = None;
    for attempt in 0..2 {
        let mut frame = native_frame(&mut host.session);
        frame
            .submit_native_layout_reset_with_roots(
                &reset,
                [(predecessors[0], B), (predecessors[1], A)],
            )
            .unwrap();
        let proposed = frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>();
        assert_swapped(&proposed, &predecessors);
        assert_eq!(exact.get_or_insert_with(|| proposed.clone()), &proposed);
        if attempt == 0 {
            drop(frame);
            assert_eq!(host.session.version(), before);
            assert_eq!(format!("{:?}", host.session.view()), graph);
            assert_eq!(host.session.next_document_generation(), generation);
            assert!(
                predecessors
                    .iter()
                    .all(|&binding| host.session.is_current_native_binding(binding))
            );
        } else {
            let mut report = finish(frame);
            assert_eq!(report.native_root_continuations(), proposed);
            assert_eq!(report.native_bindings().len(), 2);
            assert!(!host.session.version().is_same_epoch(before));
            assert!(report.take_native_effects().is_empty());
            for pair in &proposed {
                assert!(!host.session.is_current_native_binding(pair.predecessor()));
                assert!(host.session.is_current_native_binding(pair.successor()));
            }
            assert!(!host.session.has_pending_native_create());
        }
    }
}

#[test]
fn a_continuing_root_vacates_its_old_surface_for_a_restored_child() {
    let bytes = saved_document();
    let mut host = Host::new(false, false);
    let reset = host
        .session
        .prepare_layout_reset(
            DockspaceLayout::new(ROOTS.into_iter().enumerate().map(|(index, surface)| {
                DockspaceSurfaceLayout::new(
                    if surface == A { CHILDREN[0] } else { surface },
                    DockspaceRootLayout::new(
                        ROOT_IDS[index],
                        DockspaceNode::central_tabs(ROOT_ITEMS[index]),
                    ),
                )
            }))
            .unwrap(),
        )
        .unwrap();
    let mut frame = native_frame(&mut host.session);
    frame
        .submit_native_layout_reset_with_roots(
            &reset,
            [(host.roots[0], CHILDREN[0]), (host.roots[1], B)],
        )
        .unwrap();
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    host.install_continuations(report.native_root_continuations());
    let displaced_root = host.roots[0];
    assert_eq!(displaced_root.surface(), CHILDREN[0]);
    assert_eq!(displaced_root.window_token(), ROOT_TOKENS[0]);

    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut frame = native_frame(&mut host.session);
    frame
        .submit_native_document_restore_with_roots(
            &restore,
            [(displaced_root, A), (host.roots[1], B)],
        )
        .unwrap();
    let proposed = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert!(proposed.iter().any(|pair| {
        pair.predecessor() == displaced_root
            && pair.successor().surface() == A
            && pair.successor().window_token() == displaced_root.window_token()
    }));
    let mut report = finish(frame);
    assert_eq!(report.native_root_continuations(), proposed);
    assert!(report.take_native_effects().is_empty());
    assert!(!host.session.is_current_native_binding(displaced_root));
    assert_eq!(
        host.session.view().item(CHILD_ITEMS[0]).unwrap().surface(),
        CHILDREN[0]
    );
    host.install_continuations(&proposed);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let [create] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(create.operation(), NativeEffectOperation::CreateWindow {
        binding,
        role: NativeSurfaceRole::Child,
        placement,
    } if binding.surface() == CHILDREN[0]
        && *binding != displaced_root
        && binding.window_token() != displaced_root.window_token()
        && *placement == child_bounds(0))
    );
}
