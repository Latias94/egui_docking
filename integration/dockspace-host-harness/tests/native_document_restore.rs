//! Native document restoration through public persistence and host contracts.

#![cfg(feature = "serde")]

use std::num::NonZeroUsize;
use std::time::Duration;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceView, FloatingPresentationId, ItemId, NativeWindowPlacement,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceHostFrame,
    DockspaceNativeSurfaceHosting, DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostWindowToken, HostWorkAreaToken, NativeCreateEffectAcknowledgement,
    NativeCreateReadiness, NativeCreateStrategy, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeEffectAcknowledgement, NativeEffectOperation,
    NativeHostCapabilities, NativeHostCapability, NativeInputBudget, NativeInputStorageLimit,
    NativeMonotonicClock, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, NativeProducerStorageLimit,
    NativeReceiverAnswer, NativeReportStorageLimit, NativeStagingPresentationPhase,
    NativeSurfaceBinding, NativeSurfaceRole, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaFacts, NativeWorkAreaRoster, PaintedSurfaceOutput,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x6e; 16]);
// Both saved children sort before the external recovery root.
const HOST: SurfaceId = SurfaceId::new(9);
const CHILDREN: [SurfaceId; 2] = [SurfaceId::new(2), SurfaceId::new(4)];
const ROOTS: [RootId; 6] = [
    RootId::new(10),
    RootId::new(20),
    RootId::new(30),
    RootId::new(40),
    RootId::new(50),
    RootId::new(60),
];
const FLOATING: [FloatingPresentationId; 3] = [
    FloatingPresentationId::new(3),
    FloatingPresentationId::new(8),
    FloatingPresentationId::new(11),
];
const HOST_ITEM: ItemId = ItemId::new(1);
const HOST_DETAIL: ItemId = ItemId::new(2);
const MAIN: [ItemId; 2] = [ItemId::new(3), ItemId::new(4)];
const SIDE: ItemId = ItemId::new(5);
const BACK: [ItemId; 2] = [ItemId::new(6), ItemId::new(7)];
const FRONT: ItemId = ItemId::new(8);
const OTHER: [ItemId; 2] = [ItemId::new(9), ItemId::new(10)];
const OTHER_SIDE: ItemId = ItemId::new(11);
const OTHER_FLOAT: ItemId = ItemId::new(12);
const HISTORICAL: ItemId = ItemId::new(13);
const KEYS: [&str; 13] = [
    "pane:host",
    "pane:host-detail",
    "pane:main",
    "pane:main-detail",
    "pane:side",
    "pane:back",
    "pane:back-detail",
    "pane:front",
    "pane:other",
    "pane:other-detail",
    "pane:other-side",
    "pane:other-float",
    "pane:historical",
];

fn policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy
}

fn bootstrap(include_historical: bool) -> DockspaceDocumentBootstrap {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let count = if include_historical {
        KEYS.len()
    } else {
        KEYS.len() - 1
    };
    for (index, key) in KEYS[..count].iter().enumerate() {
        assert_eq!(
            bootstrap.ensure_item(*key).unwrap(),
            ItemId::new(index as u64 + 1)
        );
    }
    bootstrap
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && KEYS.contains(&key)
}

fn contained_rect(index: usize) -> LogicalRect {
    [
        LogicalRect::new(45.0, 60.0, 320.0, 240.0).unwrap(),
        LogicalRect::new(155.0, 175.0, 420.0, 280.0).unwrap(),
        LogicalRect::new(80.0, 95.0, 300.0, 220.0).unwrap(),
    ][index]
}

fn host_surface(root: RootId) -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(
        HOST,
        DockspaceRootLayout::new(
            root,
            DockspaceNode::central_tabs_with_selection([HOST_ITEM, HOST_DETAIL], Some(HOST_ITEM))
                .unwrap(),
        ),
    )
}

fn document_layout() -> DockspaceLayout {
    DockspaceLayout::new([
        host_surface(ROOTS[0]),
        DockspaceSurfaceLayout::new(
            CHILDREN[0],
            DockspaceRootLayout::new(
                ROOTS[1],
                DockspaceNode::split(
                    DockspaceAxis::Horizontal,
                    [
                        (
                            DockspaceNode::central_tabs_with_selection(MAIN, Some(MAIN[1]))
                                .unwrap(),
                            3.0,
                        ),
                        (DockspaceNode::tabs([SIDE]), 1.0),
                    ],
                )
                .unwrap(),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING[0],
            DockspaceRootLayout::new(
                ROOTS[2],
                DockspaceNode::tabs_with_selection(BACK, Some(BACK[1])).unwrap(),
            ),
            contained_rect(0),
        ))
        .with_contained(DockspaceContainedLayout::new(
            FLOATING[1],
            DockspaceRootLayout::new(ROOTS[3], DockspaceNode::tabs([FRONT])),
            contained_rect(1),
        )),
        DockspaceSurfaceLayout::new(
            CHILDREN[1],
            DockspaceRootLayout::new(
                ROOTS[4],
                DockspaceNode::split(
                    DockspaceAxis::Vertical,
                    [
                        (
                            DockspaceNode::central_tabs_with_selection(OTHER, Some(OTHER[0]))
                                .unwrap(),
                            2.0,
                        ),
                        (DockspaceNode::tabs([OTHER_SIDE]), 1.0),
                    ],
                )
                .unwrap(),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING[2],
            DockspaceRootLayout::new(ROOTS[5], DockspaceNode::tabs([OTHER_FLOAT])),
            contained_rect(2),
        )),
    ])
    .unwrap()
}

fn capabilities() -> NativeHostCapabilities {
    [
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::WorkArea,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::CloseCancellation,
    ]
    .into_iter()
    .fold(NativeHostCapabilities::none_supported(), |caps, cap| {
        caps.with(cap)
    })
}

fn placement(index: usize) -> PhysicalRect {
    [
        PhysicalRect::new(-900.0, 120.0, 640.0, 480.0).unwrap(),
        PhysicalRect::new(850.0, 180.0, 520.0, 420.0).unwrap(),
    ][index]
}

fn window_facts(bounds: PhysicalRect) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_outer_bounds(bounds)
        .with_content_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
}

fn root_facts() -> NativeWindowFacts {
    window_facts(PhysicalRect::new(100.0, 80.0, 640.0, 480.0).unwrap())
}

fn work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(-1920.0, 0.0, 3840.0, 1080.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )])
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

fn assert_no_effects(report: &mut HostFrameReport) {
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_native_staging_outputs().is_empty());
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut report = finish(native_frame(session));
    assert_no_effects(&mut report);
    assert!(report.take_painted_outputs().is_empty());
    report
}

fn measure(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    let mut frame = native_frame(session);
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    let mut report = finish(frame);
    assert_no_effects(&mut report);
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (PaintedSurfaceOutput, RecordingRenderer) {
    measure(session);
    let mut frame = native_frame(session);
    let plan = frame
        .paint_plan(binding.surface())
        .unwrap()
        .expect("a prepared exact binding must expose its complete surface program");
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    (output, recording)
}

fn assert_suspended(session: &mut DockspaceSession, surfaces: &[SurfaceId]) {
    // Measuring first distinguishes native suspension from an absent measurement.
    measure(session);
    let mut frame = native_frame(session);
    for &surface in surfaces {
        let scheduled = frame.surfaces().contains(&surface);
        match frame.paint_plan(surface) {
            Ok(None) => {}
            Ok(Some(_)) => panic!("an unhosted restored surface must not project headlessly"),
            Err(error) => {
                assert!(!scheduled);
                assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
            }
        }
    }
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    assert!(report.take_painted_outputs().is_empty());
    assert!(report.native_admissions().is_empty());
    for &surface in surfaces {
        assert!(session.presented_surface(surface).is_none());
    }
}

#[derive(Clone, Copy)]
enum SavedHosting {
    Children,
    Undeclared,
    ExtraRoot,
    MissingPlacement,
    ForeignRecoveryHost,
}

fn saved_document(hosting: SavedHosting) -> Vec<u8> {
    let mut source =
        DockspaceSession::from_persistent_layout(document_layout(), policy(), bootstrap(true))
            .unwrap();
    if !matches!(hosting, SavedHosting::Undeclared) {
        source
            .enable_managed_native_host(
                NativeCreateStrategy::DirectPrepared,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        source
            .configure_managed_native_capabilities(capabilities())
            .unwrap();
        source
            .register_native_root(HOST, HostWindowToken::new(91))
            .unwrap();
        let root = idle(&mut source).native_bindings()[0];
        let mut bindings = vec![(root, root_facts())];
        // The foreign host is a real registered Root, so the document itself is valid.
        let order = if matches!(hosting, SavedHosting::ForeignRecoveryHost) {
            [1, 0]
        } else {
            [0, 1]
        };
        for index in order {
            if matches!(hosting, SavedHosting::ExtraRoot) && index == 0
                || matches!(hosting, SavedHosting::ForeignRecoveryHost) && index == 1
            {
                source
                    .register_native_root(CHILDREN[index], HostWindowToken::new(20 + index as u64))
                    .unwrap();
            } else {
                let recovery_host = if matches!(hosting, SavedHosting::ForeignRecoveryHost) {
                    CHILDREN[1]
                } else {
                    HOST
                };
                source
                    .register_owned_native_child(
                        CHILDREN[index],
                        HostWindowToken::new(20 + index as u64),
                        recovery_host,
                    )
                    .unwrap();
            }
            let report = idle(&mut source);
            let binding = *report
                .native_bindings()
                .iter()
                .find(|binding| binding.surface() == CHILDREN[index])
                .expect("public registration must publish the declared native role");
            bindings.push((binding, window_facts(placement(index))));
        }
        if !matches!(hosting, SavedHosting::MissingPlacement) {
            source
                .report_managed_native_snapshot(bindings, work_areas())
                .unwrap();
            idle(&mut source);
        }
    }
    // Public saving owns both the document envelope and its integrity hash.
    source.save_document_json().unwrap();
    source.save_document_json().unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentIds {
    surfaces: [SurfaceId; 3],
    roots: [RootId; 6],
    contained: [FloatingPresentationId; 3],
}

impl ContentIds {
    fn children(self) -> [SurfaceId; 2] {
        [self.surfaces[1], self.surfaces[2]]
    }
}

fn assert_content(view: DockspaceView<'_>) -> ContentIds {
    assert_eq!(view.surfaces().count(), 3);
    let host = view.item(HOST_ITEM).unwrap();
    let main = view.item(MAIN[0]).unwrap();
    let back = view.item(BACK[0]).unwrap();
    let front = view.item(FRONT).unwrap();
    let other = view.item(OTHER[0]).unwrap();
    let other_float = view.item(OTHER_FLOAT).unwrap();
    let ids = ContentIds {
        surfaces: [host.surface(), main.surface(), other.surface()],
        roots: [
            host.root(),
            main.root(),
            back.root(),
            front.root(),
            other.root(),
            other_float.root(),
        ],
        contained: [
            back.contained().unwrap(),
            front.contained().unwrap(),
            other_float.contained().unwrap(),
        ],
    };
    assert_eq!(ids.surfaces[0], HOST);
    assert_ne!(ids.surfaces[1], HOST);
    assert_ne!(ids.surfaces[2], HOST);
    assert_ne!(ids.surfaces[1], ids.surfaces[2]);
    for (index, root_index, contained_indices) in [(0, 0, 0..0), (1, 1, 0..2), (2, 4, 2..3)] {
        let surface = view.surface(ids.surfaces[index]).unwrap();
        assert_eq!(surface.main_root().unwrap().id(), ids.roots[root_index]);
        assert_eq!(surface.contained_count(), contained_indices.len());
        assert_eq!(
            surface
                .contained()
                .map(|contained| (contained.id(), contained.rect()))
                .collect::<Vec<_>>(),
            contained_indices
                .map(|index| (ids.contained[index], contained_rect(index)))
                .collect::<Vec<_>>()
        );
    }
    for (items, selected, surface, root, contained, central) in [
        (
            &[HOST_ITEM, HOST_DETAIL][..],
            HOST_ITEM,
            HOST,
            ids.roots[0],
            None,
            true,
        ),
        (
            &MAIN[..],
            MAIN[1],
            ids.surfaces[1],
            ids.roots[1],
            None,
            true,
        ),
        (
            &[SIDE][..],
            SIDE,
            ids.surfaces[1],
            ids.roots[1],
            None,
            false,
        ),
        (
            &BACK[..],
            BACK[1],
            ids.surfaces[1],
            ids.roots[2],
            Some(ids.contained[0]),
            false,
        ),
        (
            &[FRONT][..],
            FRONT,
            ids.surfaces[1],
            ids.roots[3],
            Some(ids.contained[1]),
            false,
        ),
        (
            &OTHER[..],
            OTHER[0],
            ids.surfaces[2],
            ids.roots[4],
            None,
            true,
        ),
        (
            &[OTHER_SIDE][..],
            OTHER_SIDE,
            ids.surfaces[2],
            ids.roots[4],
            None,
            false,
        ),
        (
            &[OTHER_FLOAT][..],
            OTHER_FLOAT,
            ids.surfaces[2],
            ids.roots[5],
            Some(ids.contained[2]),
            false,
        ),
    ] {
        for &item in items {
            let item_view = view.item(item).unwrap();
            assert_eq!(
                (item_view.surface(), item_view.root(), item_view.contained()),
                (surface, root, contained)
            );
            assert_eq!(item_view.tabs().items(), items);
            assert_eq!(item_view.tabs().selected(), Some(selected));
            assert_eq!(item_view.tabs().is_central(), central);
            assert_eq!(item_view.is_selected(), item == selected);
        }
    }
    for (root, axis, weights) in [
        (ids.roots[1], DockspaceAxis::Horizontal, [0.75, 0.25]),
        (
            ids.roots[4],
            DockspaceAxis::Vertical,
            [2.0 / 3.0, 1.0 / 3.0],
        ),
    ] {
        let split = view.root(root).unwrap().content().unwrap().split().unwrap();
        assert_eq!(split.axis(), axis);
        assert_eq!(split.children().count(), 2);
        let actual = split.weights().collect::<Vec<_>>();
        assert_eq!(actual.len(), weights.len());
        for (actual, expected) in actual.into_iter().zip(weights) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
    for (index, root_index) in [(0, 2), (1, 3), (2, 5)] {
        assert_eq!(
            view.contained(ids.contained[index])
                .unwrap()
                .root()
                .unwrap()
                .id(),
            ids.roots[root_index]
        );
    }
    assert!(view.item(HISTORICAL).is_none());
    ids
}

struct NativeDocumentHost {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
    original_children: Vec<NativeSurfaceBinding>,
}

impl NativeDocumentHost {
    fn new(binding_slots: usize, retire_saved_children: bool) -> Self {
        Self::with_host_storage(
            binding_slots,
            retire_saved_children,
            NativeCreateStrategy::DirectPrepared,
            None,
        )
    }

    fn with_host_storage(
        binding_slots: usize,
        retire_saved_children: bool,
        strategy: NativeCreateStrategy,
        reports: Option<NativeReportStorageLimit>,
    ) -> Self {
        let mut session =
            DockspaceSession::from_persistent_layout(document_layout(), policy(), bootstrap(false))
                .unwrap();
        if retire_saved_children {
            let reset = session
                .prepare_layout_reset(
                    DockspaceLayout::new([host_surface(RootId::new(900))]).unwrap(),
                )
                .unwrap();
            let mut frame = session.begin_host_frame().unwrap();
            frame.submit_layout_reset(&reset).unwrap();
            finish(frame);
        }
        let mut storage = NativeInputStorageLimit::new(NonZeroUsize::new(32).unwrap(), 1 << 20)
            .with_producer_storage_limit(NativeProducerStorageLimit::new(
                NonZeroUsize::new(binding_slots).unwrap(),
                NonZeroUsize::MIN,
                1 << 20,
            ));
        if let Some(reports) = reports {
            storage = storage.with_report_storage_limit(reports);
        }
        session
            .enable_managed_native_host_with_input_storage_limit(
                strategy,
                NativePointerRoster::Exact(Vec::new()),
                storage,
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(capabilities())
            .unwrap();
        session
            .register_native_root(HOST, HostWindowToken::new(1))
            .unwrap();
        let root = idle(&mut session).native_bindings()[0];
        let mut original_children = Vec::new();
        if !retire_saved_children {
            for (index, surface) in CHILDREN.into_iter().enumerate() {
                session
                    .register_owned_native_child(
                        surface,
                        HostWindowToken::new(2 + index as u64),
                        HOST,
                    )
                    .unwrap();
                let report = idle(&mut session);
                original_children.push(
                    *report
                        .native_bindings()
                        .iter()
                        .find(|binding| binding.surface() == surface)
                        .unwrap(),
                );
            }
        }
        let mut observations = vec![(root, root_facts())];
        observations.extend(
            original_children
                .iter()
                .enumerate()
                .map(|(index, &binding)| (binding, window_facts(placement(index)))),
        );
        session
            .report_managed_native_snapshot(observations, work_areas())
            .unwrap();
        idle(&mut session);
        let (output, _) = paint(&mut session, root);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut session);
        assert!(session.native_local_coordinates(root).is_some());
        Self {
            session,
            root,
            original_children,
        }
    }

    fn restore(&mut self, bytes: &[u8]) -> (ContentIds, HostFrameReport) {
        let restore = self
            .session
            .prepare_document_restore_json(bytes, recognize)
            .unwrap();
        restore.validate_native_surface_hosting(HOST).unwrap();
        let mut frame = native_frame(&mut self.session);
        frame
            .submit_native_document_restore(&restore, HOST)
            .unwrap();
        let ids = assert_content(frame.view());
        let report = finish(frame);
        let [root] = report.native_bindings() else {
            panic!("terminal restore must retain exactly the real external root");
        };
        assert_eq!(root.surface(), HOST);
        assert_eq!(root.window_token(), self.root.window_token());
        assert_ne!(*root, self.root);
        self.root = *root;
        assert!(self.session.has_pending_native_create());
        assert!(self.session.native_local_coordinates(self.root).is_none());
        (ids, report)
    }

    fn ready_root(&mut self) {
        self.session
            .report_managed_native_snapshot([(self.root, root_facts())], work_areas())
            .unwrap();
        idle(&mut self.session);
        let (output, _) = paint(&mut self.session, self.root);
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut self.session);
        assert!(self.session.native_local_coordinates(self.root).is_some());
    }

    fn quiesce(&mut self, binding: NativeSurfaceBinding) {
        self.session
            .report_native_binding_quiescence(binding)
            .unwrap();
        assert!(!self.session.recognizes_native_binding(binding));
        idle(&mut self.session);
        // Reclaim committed ingress and its exact terminal guard before advancing.
        idle(&mut self.session);
    }

    fn assert_waiting(&mut self) {
        let version = self.session.version();
        let recorded = self.session.native_input_recorded_count();
        let mut frame = native_frame(&mut self.session);
        assert!(!frame.advance_next_native_surface_create().unwrap());
        assert!(!frame.advance_next_native_surface_create().unwrap());
        let mut report = finish(frame);
        assert!(
            report.inputs().is_empty(),
            "temporary waiting must append no lifecycle input"
        );
        assert_no_effects(&mut report);
        assert!(!report.workspace_changed());
        assert_eq!(self.session.version(), version);
        assert_eq!(self.session.native_input_recorded_count(), recorded);
    }

    fn admit(
        &mut self,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCreateEffectAcknowledgement,
        ids: ContentIds,
        index: usize,
        admitted: &[NativeSurfaceBinding],
    ) {
        let mut observations = vec![(self.root, root_facts())];
        for &other in admitted {
            let index = ids
                .children()
                .iter()
                .position(|&surface| surface == other.surface())
                .unwrap();
            observations.push((
                other,
                window_facts(placement(index))
                    .with_create_readiness(NativeCreateReadiness::Prepared, None),
            ));
        }
        observations.push((
            binding,
            window_facts(placement(index))
                .with_create_readiness(NativeCreateReadiness::Prepared, Some(acknowledgement)),
        ));
        self.session
            .report_managed_native_snapshot(observations, work_areas())
            .unwrap();
        assert!(idle(&mut self.session).native_admissions().is_empty());
        let (output, recording) = paint(&mut self.session, binding);
        let roots = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::RootBegin(root) => Some(*root),
                _ => None,
            })
            .collect::<Vec<_>>();
        let contents = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::PaneContent {
                    item: Some(item), ..
                } => Some(*item),
                _ => None,
            })
            .collect::<Vec<_>>();
        if index == 0 {
            assert_eq!(roots, ids.roots[1..4]);
            assert_eq!(contents, [MAIN[1], SIDE, BACK[1], FRONT]);
        } else {
            assert_eq!(roots, ids.roots[4..6]);
            assert_eq!(contents, [OTHER[0], OTHER_SIDE, OTHER_FLOAT]);
        }
        let artifact = output.presentation_artifact();
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut self.session);
        assert_eq!(report.native_admissions(), &[binding]);
        assert!(!report.workspace_changed());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(artifact))
        );
        assert!(
            self.session
                .presented_surface(binding.surface())
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
        assert!(self.session.native_local_coordinates(binding).is_some());
        assert_eq!(assert_content(self.session.view()), ids);
    }
}

fn take_create(
    report: &mut HostFrameReport,
    surface: SurfaceId,
    index: usize,
) -> (NativeSurfaceBinding, NativeCreateEffectAcknowledgement) {
    let requested = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceCreateRequested { binding } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [binding] = requested.as_slice() else {
        panic!("one advancement must report exactly one public native creation request");
    };
    let binding = *binding;
    assert_eq!(binding.surface(), surface);
    assert!(report.native_bindings().contains(&binding));
    assert!(report.native_admissions().is_empty());
    assert!(!report.workspace_changed());
    let [create] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(create.operation(), NativeEffectOperation::CreateWindow {
        binding: actual, placement: bounds, role: NativeSurfaceRole::Child,
        strategy: NativeCreateStrategy::DirectPrepared,
    } if *actual == binding && *bounds == placement(index))
    );
    let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = create.accepted() else {
        panic!("the exact Create must own its renderer readiness acknowledgement");
    };
    (binding, acknowledgement)
}

#[test]
fn multiple_children_restore_atomically_rebase_the_complete_forest_and_advance_once_each() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host = NativeDocumentHost::new(8, true);
    let old_root = host.root;
    let before = host.session.version();
    let generation = host.session.next_document_generation();
    let recorded = host.session.native_input_recorded_count();
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    restore.validate_native_surface_hosting(HOST).unwrap();
    let original = assert_content(restore.view());
    assert_eq!(original.children(), CHILDREN);
    let mut candidate_ids = None;
    let mut successor = None;
    for attempt in 0..3 {
        let mut frame = native_frame(&mut host.session);
        frame
            .submit_native_document_restore(&restore, HOST)
            .unwrap();
        let ids = assert_content(frame.view());
        assert_eq!(*candidate_ids.get_or_insert(ids), ids);
        for surface in CHILDREN {
            assert!(frame.view().surface(surface).is_none());
        }
        for root in ROOTS {
            assert!(frame.view().root(root).is_none());
        }
        for floating in FLOATING {
            assert!(frame.view().contained(floating).is_none());
        }
        assert!(ids.roots.iter().all(|root| root.get() > 900));
        let bindings = frame.proposed_native_bindings().collect::<Vec<_>>();
        let [root] = bindings.as_slice() else {
            panic!("restore may queue children but cannot allocate their bindings");
        };
        assert_eq!(root.surface(), HOST);
        assert_eq!(root.window_token(), old_root.window_token());
        assert_ne!(*root, old_root);
        assert_eq!(*successor.get_or_insert(*root), *root);
        assert_eq!(frame.external_key_for_item(HISTORICAL), Some(KEYS[12]));
        assert_eq!(
            frame.select_item_current(HOST_DETAIL).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        assert_eq!(
            frame
                .advance_next_native_surface_create()
                .unwrap_err()
                .kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        match attempt {
            0 => drop(frame),
            1 => {
                frame.commit().expect_err(
                    "an incomplete terminal frame must publish neither graph nor intents",
                );
            }
            _ => {
                let mut report = finish(frame);
                assert!(
                    matches!(report.inputs(), [HostInputOutcome::DocumentRestored { previous, current }] if *previous == before && *current == report.after())
                );
                assert_no_effects(&mut report);
                assert!(report.take_painted_outputs().is_empty());
                assert_eq!(report.native_bindings(), &[*root]);
            }
        }
        assert_eq!(host.session.native_input_recorded_count(), recorded);
        if attempt < 2 {
            assert_eq!(host.session.version(), before);
            assert_eq!(host.session.next_document_generation(), generation);
            assert_eq!(host.session.external_key_for_item(HISTORICAL), None);
            assert_eq!(host.session.view().surfaces().count(), 1);
            assert!(host.session.is_current_native_binding(old_root));
            assert!(!host.session.has_pending_native_create());
        }
    }
    host.root = successor.unwrap();
    let ids = candidate_ids.unwrap();
    assert_eq!(host.session.next_document_generation(), Some(2));
    assert!(host.session.has_pending_native_create());
    let preferences = host
        .session
        .surface_presentation_preferences()
        .collect::<Vec<_>>();
    assert_eq!(preferences.len(), 3);
    let mut declared_children = 0;
    for preference in preferences {
        if let Some(index) = ids
            .children()
            .iter()
            .position(|&surface| surface == preference.surface())
        {
            assert_eq!(
                preference.native_hosting(),
                Some(DockspaceNativeSurfaceHosting::Child {
                    recovery_host: HOST
                })
            );
            assert_eq!(preference.outer_rect(), Some(placement(index)));
            declared_children += 1;
        }
    }
    assert_eq!(declared_children, 2);
    assert_suspended(&mut host.session, &ids.children());
    host.ready_root();
    host.assert_waiting();
    host.quiesce(old_root);
    host.ready_root();

    let movement = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::Moved,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    );
    host.session
        .record_native_pointer_sequence([movement; 2])
        .unwrap();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    budget.complete_unsequenced_records(255);
    let mut prefix = host
        .session
        .poll_native_host_frame(&mut budget, |_| unreachable!())
        .unwrap()
        .unwrap();
    assert!(prefix.native_input_pending());
    assert_eq!(
        prefix
            .advance_next_native_surface_create()
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    let mut report = finish(prefix);
    assert_no_effects(&mut report);
    assert!(report.native_input_pending());

    let version = host.session.version();
    let recorded = host.session.native_input_recorded_count();
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert_eq!(host.session.version(), version);
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    assert!(host.session.has_pending_native_input());
    assert!(host.session.has_pending_native_create());
    assert_eq!(idle(&mut host.session).native_bindings(), &[host.root]);
    let mut admitted = Vec::new();
    for index in 0..2 {
        let mut frame = native_frame(&mut host.session);
        assert!(frame.advance_next_native_surface_create().unwrap());
        if index == 1 {
            assert!(!frame.advance_next_native_surface_create().unwrap());
        }
        let mut report = finish(frame);
        let (binding, acknowledgement) = take_create(&mut report, ids.children()[index], index);
        assert_eq!(assert_content(host.session.view()), ids);
        host.admit(binding, acknowledgement, ids, index, &admitted);
        admitted.push(binding);
    }
    assert_ne!(admitted[0], admitted[1]);
    assert!(!host.session.has_pending_native_create());
    for _ in 0..2 {
        host.assert_waiting();
    }
}

#[test]
fn fresh_root_facts_and_every_inherited_cleanup_and_terminal_guard_are_required() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host = NativeDocumentHost::new(8, false);
    let old_root = host.root;
    let old_children = host.original_children.clone();
    let (old_output, _) = paint(&mut host.session, old_root);
    let old_artifact = old_output.presentation_artifact();
    let (_, mut first) = host.restore(&bytes);
    let cleanups = first.take_native_effects();
    assert_eq!(cleanups.len(), 2);
    assert!(cleanups.iter().all(|effect| matches!(effect.operation(), NativeEffectOperation::ReleaseChild { binding } if old_children.contains(binding))));
    let intermediate_root = host.root;
    // The second replacement must inherit cleanup keys and the older Root guard.
    let (ids, mut second) = host.restore(&bytes);
    let continuations = second.take_native_effects();
    assert_eq!(continuations.len(), old_children.len());
    for continuation in continuations {
        assert!(
            matches!(continuation.operation(), NativeEffectOperation::AwaitCleanup { binding } if old_children.contains(binding))
        );
        assert!(matches!(
            continuation.accepted(),
            Some(NativeEffectAcknowledgement::Cleanup(_))
        ));
    }
    assert_no_effects(&mut second);
    host.assert_waiting();
    let recorded = host.session.native_input_recorded_count();
    host.session
        .report_managed_native_snapshot([(old_root, root_facts())], work_areas())
        .expect_err("late live facts cannot prepare the retained Root successor");
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    assert!(host.session.native_local_coordinates(host.root).is_none());
    host.session
        .report_surface_presentation(old_output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut host.session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(old_artifact))
    );
    assert!(host.session.native_local_coordinates(host.root).is_none());
    host.assert_waiting();
    host.quiesce(intermediate_root);
    host.ready_root();
    let (obsolete_root_output, _) = paint(&mut host.session, host.root);
    let obsolete_root_artifact = obsolete_root_output.presentation_artifact();
    host.assert_waiting();
    for cleanup in cleanups {
        let binding = match cleanup.operation() {
            NativeEffectOperation::ReleaseChild { binding } => *binding,
            operation => panic!("unexpected terminal cleanup: {operation:?}"),
        };
        let Some(NativeEffectAcknowledgement::Close(destroyed)) = cleanup.accepted() else {
            panic!("cleanup must retain its exact Close acknowledgement");
        };
        host.session
            .report_managed_native_snapshot(
                [
                    (host.root, root_facts()),
                    (binding, NativeWindowFacts::destroyed_after(destroyed)),
                ],
                work_areas(),
            )
            .unwrap();
        idle(&mut host.session);
        host.assert_waiting();
    }
    // Destroyed is not producer quiescence, and quiescing a different key is insufficient.
    for binding in old_children {
        host.quiesce(binding);
        host.assert_waiting();
    }
    host.quiesce(old_root);
    // With every old key reclaimed, missing current geometry must independently wait.
    host.session
        .report_managed_native_snapshot([(host.root, NativeWindowFacts::live())], work_areas())
        .unwrap();
    idle(&mut host.session);
    assert!(host.session.native_local_coordinates(host.root).is_none());
    host.assert_waiting();
    host.session
        .report_surface_presentation(obsolete_root_output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut host.session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(obsolete_root_artifact))
    );
    assert!(host.session.native_local_coordinates(host.root).is_none());
    host.assert_waiting();
    host.ready_root();
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let (first, acknowledgement) = take_create(&mut report, ids.children()[0], 0);
    host.admit(first, acknowledgement, ids, 0, &[]);
    // Explicit creation can also consume a Waiting intent through the same ready gate.
    let mut frame = native_frame(&mut host.session);
    let action = frame
        .create_native_surface_current(
            ids.children()[1],
            HOST,
            NativeWindowPlacement::new(placement(1)),
        )
        .unwrap();
    let mut report = finish(frame);
    let (second, acknowledgement) = take_create(&mut report, ids.children()[1], 1);
    assert_eq!(
        report.submitted_action_outcome(action),
        Some(&HostInputOutcome::NativeSurfaceCreateRequested { binding: second })
    );
    host.admit(second, acknowledgement, ids, 1, &[first]);
    assert!(!host.session.has_pending_native_create());
    host.assert_waiting();
}

#[test]
fn queued_shutdown_cancellation_needs_no_binding_and_explicit_retry_preserves_suspension() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host = NativeDocumentHost::new(8, true);
    let old_root = host.root;
    let (ids, mut report) = host.restore(&bytes);
    assert_no_effects(&mut report);
    let version = host.session.version();
    let mut dropped = native_frame(&mut host.session);
    assert!(dropped.cancel_next_native_create().unwrap());
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(dropped);
    assert!(host.session.has_pending_native_create());
    let mut frame = native_frame(&mut host.session);
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    let cancellations = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceCreateIntentCancelled { surface, changed } => {
                Some((*surface, *changed))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations, ids.children().map(|surface| (surface, true)));
    assert_eq!(report.native_bindings(), &[host.root]);
    assert_no_effects(&mut report);
    assert!(!report.workspace_changed());
    assert!(!host.session.has_pending_native_create());
    assert_suspended(&mut host.session, &ids.children());
    host.assert_waiting();
    let mut frame = native_frame(&mut host.session);
    let blocked = frame
        .create_native_surface_current(
            ids.children()[0],
            HOST,
            NativeWindowPlacement::new(placement(0)),
        )
        .unwrap();
    let mut report = finish(frame);
    assert!(
        matches!(report.submitted_action_outcome(blocked), Some(HostInputOutcome::NativeSurfaceCreateRejected { surface, .. }) if *surface == ids.children()[0])
    );
    assert_no_effects(&mut report);
    assert!(!host.session.has_pending_native_create());
    assert_suspended(&mut host.session, &ids.children());
    host.quiesce(old_root);
    host.ready_root();
    let mut admitted = Vec::new();
    for index in 0..2 {
        host.assert_waiting();
        let mut frame = native_frame(&mut host.session);
        let action = frame
            .create_native_surface_current(
                ids.children()[index],
                HOST,
                NativeWindowPlacement::new(placement(index)),
            )
            .unwrap();
        let mut report = finish(frame);
        let (binding, acknowledgement) = take_create(&mut report, ids.children()[index], index);
        assert_eq!(
            report.submitted_action_outcome(action),
            Some(&HostInputOutcome::NativeSurfaceCreateRequested { binding })
        );
        assert_suspended(&mut host.session, &[binding.surface()]);
        host.admit(binding, acknowledgement, ids, index, &admitted);
        admitted.push(binding);
        assert!(!host.session.has_pending_native_create());
    }
    assert_eq!(host.session.version(), version);
    assert_eq!(assert_content(host.session.view()), ids);
}

#[test]
fn invalid_hosting_declarations_reject_atomically_and_ordinary_restore_remains_inert() {
    let valid = saved_document(SavedHosting::Children);
    let cases = [
        (
            valid.clone(),
            SurfaceId::new(99),
            DockspaceRuntimeErrorKind::InvalidConfiguration,
        ),
        (
            valid.clone(),
            CHILDREN[0],
            DockspaceRuntimeErrorKind::InvalidConfiguration,
        ),
        (
            saved_document(SavedHosting::Undeclared),
            HOST,
            DockspaceRuntimeErrorKind::Unsupported,
        ),
        (
            saved_document(SavedHosting::ExtraRoot),
            HOST,
            DockspaceRuntimeErrorKind::Unsupported,
        ),
        (
            saved_document(SavedHosting::MissingPlacement),
            HOST,
            DockspaceRuntimeErrorKind::Unsupported,
        ),
        (
            saved_document(SavedHosting::ForeignRecoveryHost),
            HOST,
            DockspaceRuntimeErrorKind::Unsupported,
        ),
    ];
    for (bytes, root, kind) in cases {
        let mut host = NativeDocumentHost::new(8, true);
        let before = host.session.version();
        let generation = host.session.next_document_generation();
        let recorded = host.session.native_input_recorded_count();
        let restore = host
            .session
            .prepare_document_restore_json(&bytes, recognize)
            .unwrap();
        assert_eq!(
            restore
                .validate_native_surface_hosting(root)
                .unwrap_err()
                .kind(),
            kind
        );
        let mut frame = native_frame(&mut host.session);
        frame.select_item_current(HOST_DETAIL).unwrap();
        let prefix = frame.version();
        assert_ne!(prefix, before);
        assert_eq!(
            frame
                .submit_native_document_restore(&restore, root)
                .unwrap_err()
                .kind(),
            kind
        );
        assert_eq!(frame.version(), prefix);
        assert_eq!(frame.view().surfaces().count(), 1);
        assert!(frame.view().item(HOST_DETAIL).unwrap().is_selected());
        assert_eq!(frame.external_key_for_item(HISTORICAL), None);
        drop(frame);
        assert_eq!(host.session.version(), before);
        assert_eq!(host.session.next_document_generation(), generation);
        assert_eq!(host.session.native_input_recorded_count(), recorded);
        assert_eq!(host.session.external_key_for_item(HISTORICAL), None);
        assert!(host.session.is_current_native_binding(host.root));
        assert!(!host.session.has_pending_native_create());
        let mut report = idle(&mut host.session);
        assert!(report.inputs().is_empty());
        assert_no_effects(&mut report);
    }

    let mut host = NativeDocumentHost::new(8, true);
    let restore = host
        .session
        .prepare_document_restore_json(&valid, recognize)
        .unwrap();
    restore.validate_native_surface_hosting(HOST).unwrap();
    let mut frame = native_frame(&mut host.session);
    frame.submit_document_restore(&restore).unwrap();
    let ids = assert_content(frame.view());
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    assert!(!host.session.has_pending_native_create());
    assert_eq!(report.native_bindings().len(), 1);
    assert!(ids.children().iter().all(|surface| {
        report
            .native_bindings()
            .iter()
            .all(|binding| binding.surface() != *surface)
    }));
    host.root = report.native_bindings()[0];
    for _ in 0..2 {
        host.assert_waiting();
    }

    // An unspecified root declaration is valid when no extra surface needs hosting.
    let mut unspecified = DockspaceSession::from_persistent_layout(
        DockspaceLayout::new([host_surface(ROOTS[0])]).unwrap(),
        policy(),
        bootstrap(true),
    )
    .unwrap();
    let bytes = unspecified.save_document_json().unwrap();
    let mut host = NativeDocumentHost::new(8, true);
    let root = host.root;
    let restore = host
        .session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    assert!(
        restore
            .surface_presentation_preferences()
            .all(|preference| preference.native_hosting().is_none())
    );
    restore.validate_native_surface_hosting(HOST).unwrap();
    let mut frame = native_frame(&mut host.session);
    frame
        .submit_native_document_restore(&restore, HOST)
        .unwrap();
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    let [retained] = report.native_bindings() else {
        panic!("the actual external root must survive an unspecified declaration");
    };
    assert_eq!(retained.surface(), HOST);
    assert_eq!(retained.window_token(), root.window_token());
    assert_ne!(*retained, root);
    assert!(!host.session.has_pending_native_create());

    let mut unregistered = DockspaceSession::from_persistent_layout(
        DockspaceLayout::new([host_surface(ROOTS[0])]).unwrap(),
        policy(),
        bootstrap(false),
    )
    .unwrap();
    unregistered
        .enable_managed_native_host(
            NativeCreateStrategy::DirectPrepared,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    unregistered
        .configure_managed_native_capabilities(capabilities())
        .unwrap();
    idle(&mut unregistered);
    let before = unregistered.version();
    let restore = unregistered
        .prepare_document_restore_json(&valid, recognize)
        .unwrap();
    restore.validate_native_surface_hosting(HOST).unwrap();
    let mut frame = native_frame(&mut unregistered);
    frame
        .submit_native_document_restore(&restore, HOST)
        .expect_err("declarations alone cannot manufacture the external Root to retain");
    assert_eq!(frame.version(), before);
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    assert!(report.native_bindings().is_empty());
    assert_eq!(unregistered.version(), before);
    assert!(!unregistered.has_pending_native_create());
}

#[test]
fn finite_producer_slots_wait_without_appending_and_dropped_frames_retry_exact_creates() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host = NativeDocumentHost::new(2, true);
    let old_root = host.root;
    let backing = host.session.native_input_producer_storage_capacity_bytes();
    let reserved = host.session.native_input_producer_storage_reserved_bytes();
    let (ids, mut report) = host.restore(&bytes);
    assert_no_effects(&mut report);
    let version = host.session.version();
    host.ready_root();
    host.assert_waiting();
    host.quiesce(old_root);
    let recorded = host.session.native_input_recorded_count();
    let mut committed = None;
    for attempt in 0..3 {
        let mut frame = native_frame(&mut host.session);
        assert!(frame.advance_next_native_surface_create().unwrap());
        assert!(!frame.advance_next_native_surface_create().unwrap());
        match attempt {
            0 => {
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                drop(frame);
            }
            1 => {
                frame.commit().expect_err(
                    "failed publication must release its tentative producer reservation",
                );
            }
            _ => {
                let mut report = finish(frame);
                let (actual, acknowledgement) = take_create(&mut report, ids.children()[0], 0);
                committed = Some((actual, acknowledgement));
            }
        }
        assert_eq!(host.session.version(), version);
        assert_eq!(host.session.native_input_recorded_count(), recorded);
        assert_eq!(
            host.session.native_input_producer_storage_capacity_bytes(),
            backing
        );
        assert_eq!(
            host.session.native_input_producer_storage_reserved_bytes(),
            reserved
        );
        if attempt < 2 {
            assert_eq!(idle(&mut host.session).native_bindings(), &[host.root]);
        }
    }
    let (cancelled, readiness) = committed.unwrap();
    let mut frame = native_frame(&mut host.session);
    assert!(frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    assert!(
        report
            .inputs()
            .contains(&HostInputOutcome::NativeSurfaceCreateCancelled {
                binding: cancelled,
                changed: true
            })
    );
    assert!(
        host.session.has_pending_native_create(),
        "the other Waiting intent still counts as pending"
    );
    assert!(!host.session.is_current_native_binding(cancelled));
    assert_no_effects(&mut report);
    host.assert_waiting();
    // Dispatch may have created a window after cancellation. Only its exact
    // late appearance permits destructive compensation for this incarnation.
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (
                    cancelled,
                    window_facts(placement(0))
                        .with_create_readiness(NativeCreateReadiness::Created, Some(readiness)),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let mut report = finish(native_frame(&mut host.session));
    assert!(host.session.native_local_coordinates(cancelled).is_none());
    assert!(!host.session.native_binding_can_report_focus(cancelled));
    let [cleanup] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(cleanup.operation(), NativeEffectOperation::CompensatingClose { binding } if *binding == cancelled)
    );
    // The cancelled lifetime still occupies the sole child slot after its owner stops.
    host.assert_waiting();
    let Some(NativeEffectAcknowledgement::Close(destroyed)) = cleanup.accepted() else {
        panic!("cancellation must own its exact cleanup acknowledgement");
    };
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (cancelled, NativeWindowFacts::destroyed_after(destroyed)),
            ],
            work_areas(),
        )
        .unwrap();
    idle(&mut host.session);
    let recorded = host.session.native_input_recorded_count();
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (cancelled, window_facts(placement(0))),
            ],
            work_areas(),
        )
        .expect_err("a bare terminal guard cannot accept live facts after exact cleanup");
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    host.assert_waiting();
    host.quiesce(cancelled);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert_eq!(idle(&mut host.session).native_bindings(), &[host.root]);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    assert!(!frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let (binding, acknowledgement) = take_create(&mut report, ids.children()[1], 1);
    assert_ne!(binding, cancelled);
    host.admit(binding, acknowledgement, ids, 1, &[]);
    assert_suspended(&mut host.session, &[ids.children()[0]]);
    assert!(!host.session.has_pending_native_create());
    host.assert_waiting();
    assert_eq!(host.session.version(), version);
    assert_eq!(assert_content(host.session.view()), ids);
    assert_eq!(
        host.session.native_input_producer_storage_capacity_bytes(),
        backing
    );
    assert_eq!(
        host.session.native_input_producer_storage_reserved_bytes(),
        reserved
    );
}

#[test]
fn incoming_child_cannot_replace_an_existing_external_root_and_failed_restore_is_atomic() {
    let bytes = saved_document(SavedHosting::Children);
    let mut session =
        DockspaceSession::from_persistent_layout(document_layout(), policy(), bootstrap(false))
            .unwrap();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::DirectPrepared,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(capabilities())
        .unwrap();
    // Both windows are externally owned Roots before the incoming Child declaration.
    session
        .register_native_root(HOST, HostWindowToken::new(101))
        .unwrap();
    session
        .register_native_root(CHILDREN[0], HostWindowToken::new(102))
        .unwrap();
    let registration = idle(&mut session);
    let bindings = registration.native_bindings().to_vec();
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().any(|binding| binding.surface() == HOST));
    assert!(
        bindings
            .iter()
            .any(|binding| binding.surface() == CHILDREN[0])
    );
    session
        .report_managed_native_snapshot(
            bindings.iter().copied().map(|binding| {
                let facts = if binding.surface() == HOST {
                    root_facts()
                } else {
                    window_facts(placement(0))
                };
                (binding, facts)
            }),
            work_areas(),
        )
        .unwrap();
    idle(&mut session);
    let before = session.version();
    let view = format!("{:?}", session.view());
    let generation = session.next_document_generation();
    let recorded = session.native_input_recorded_count();
    let restore = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    restore.validate_native_surface_hosting(HOST).unwrap();
    let child = restore
        .surface_presentation_preferences()
        .find(|preference| preference.surface() == CHILDREN[0])
        .unwrap();
    assert_eq!(
        child.native_hosting(),
        Some(DockspaceNativeSurfaceHosting::Child {
            recovery_host: HOST
        })
    );
    let mut frame = native_frame(&mut session);
    assert_eq!(
        frame
            .submit_native_document_restore(&restore, HOST)
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    drop(frame);
    assert_eq!(session.version(), before);
    assert_eq!(format!("{:?}", session.view()), view);
    assert_eq!(session.next_document_generation(), generation);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.external_key_for_item(HISTORICAL), None);
    assert!(!session.has_pending_native_create());
    assert!(!session.has_pending_native_input());
    for &binding in &bindings {
        assert!(session.is_current_native_binding(binding));
        assert!(session.recognizes_native_binding(binding));
    }
    let mut report = idle(&mut session);
    assert_eq!(report.native_bindings(), bindings);
    assert!(report.inputs().is_empty());
    assert_no_effects(&mut report);
    assert_eq!(session.version(), before);
    assert_eq!(format!("{:?}", session.view()), view);
    assert_eq!(session.next_document_generation(), generation);
}

fn paint_staging(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
) -> dockspace::runtime::PaintedNativeStagingOutput {
    let mut frame = native_frame(session);
    let requests = frame.native_staging_paints().collect::<Vec<_>>();
    let [request] = requests.as_slice() else {
        panic!("one exact staging request is required");
    };
    assert_eq!(request.binding(), binding);
    assert_eq!(request.phase(), phase);
    assert!(frame.paint_plan(binding.surface()).unwrap().is_none());
    frame.confirm_native_staging_painted(*request).unwrap();
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    assert!(report.native_admissions().is_empty());
    let [output] = report
        .take_painted_native_staging_outputs()
        .try_into()
        .unwrap();
    output
}

#[test]
fn restored_staged_child_keeps_its_full_forest_through_placeholder_and_first_live_barriers() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host =
        NativeDocumentHost::with_host_storage(8, true, NativeCreateStrategy::Staged, None);
    let old_root = host.root;
    let (ids, mut report) = host.restore(&bytes);
    assert_no_effects(&mut report);
    let version = host.session.version();
    host.quiesce(old_root);
    host.ready_root();
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let [create] = report.take_native_effects().try_into().unwrap();
    let binding = match create.operation() {
        NativeEffectOperation::CreateWindow {
            binding,
            strategy: NativeCreateStrategy::Staged,
            ..
        } => *binding,
        operation => panic!("expected staged create, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("exact create acknowledgement is required");
    };
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (
                    binding,
                    window_facts(placement(0))
                        .with_presentation(NativeWindowPresentationState::Hidden, None)
                        .with_create_readiness(NativeCreateReadiness::Created, Some(created)),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    idle(&mut host.session);
    let dropped = paint_staging(
        &mut host.session,
        binding,
        NativeStagingPresentationPhase::PreShow,
    );
    host.session
        .report_native_staging_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    idle(&mut host.session);
    assert!(host.session.native_local_coordinates(binding).is_none());
    let output = paint_staging(
        &mut host.session,
        binding,
        NativeStagingPresentationPhase::PreShow,
    );
    host.session
        .report_native_staging_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut report = finish(native_frame(&mut host.session));
    let [show] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(show.operation(), NativeEffectOperation::ShowWindow { binding: actual } if *actual == binding)
    );
    let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
        panic!("exact Show acknowledgement is required");
    };
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (
                    binding,
                    window_facts(placement(0))
                        .with_presentation(NativeWindowPresentationState::Visible, Some(shown))
                        .with_create_readiness(NativeCreateReadiness::Created, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    idle(&mut host.session);
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (
                    binding,
                    window_facts(placement(0))
                        .with_create_readiness(NativeCreateReadiness::Created, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    idle(&mut host.session);
    let output = paint_staging(
        &mut host.session,
        binding,
        NativeStagingPresentationPhase::PostShow,
    );
    host.session
        .report_native_staging_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(idle(&mut host.session).native_admissions().is_empty());
    assert!(host.session.native_local_coordinates(binding).is_none());
    host.admit(binding, created, ids, 0, &[]);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    assert!(
        report
            .inputs()
            .contains(&HostInputOutcome::NativeSurfaceCreateIntentCancelled {
                surface: ids.children()[1],
                changed: true
            })
    );
    assert!(!host.session.has_pending_native_create());
    assert!(host.session.native_local_coordinates(binding).is_some());
    assert_eq!(host.session.version(), version);
    assert_eq!(assert_content(host.session.view()), ids);
}

#[test]
fn automatic_create_survives_effect_report_backpressure_in_the_existing_ledger() {
    let bytes = saved_document(SavedHosting::Children);
    let reports =
        NativeReportStorageLimit::new(NonZeroUsize::new(8).unwrap(), NonZeroUsize::MIN, 1 << 20);
    let mut host = NativeDocumentHost::with_host_storage(
        8,
        true,
        NativeCreateStrategy::DirectPrepared,
        Some(reports),
    );
    let old_root = host.root;
    let (ids, mut report) = host.restore(&bytes);
    assert_no_effects(&mut report);
    host.quiesce(old_root);
    host.ready_root();
    let version = host.session.version();
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let [held] = report.take_native_effects().try_into().unwrap();
    let first = match held.operation() {
        NativeEffectOperation::CreateWindow { binding, .. } => *binding,
        operation => panic!("expected first Create, got {operation:?}"),
    };
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    assert!(!frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let [second] = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceCreateRequested { binding } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert_ne!(first, second);
    assert_eq!([first.surface(), second.surface()], ids.children());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(report.native_bindings().len(), 3);
    assert_eq!(host.session.version(), version);
    let Some(NativeEffectAcknowledgement::Create(first_created)) = held.accepted() else {
        panic!("first request keeps its exact capability");
    };
    let mut report = finish(native_frame(&mut host.session));
    assert!(report.inputs().is_empty());
    let [create] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(create.operation(), NativeEffectOperation::CreateWindow { binding, .. } if *binding == second)
    );
    let Some(NativeEffectAcknowledgement::Create(second_created)) = create.accepted() else {
        panic!("deferred request keeps its exact capability");
    };
    host.session
        .report_managed_native_snapshot(
            [
                (host.root, root_facts()),
                (
                    first,
                    window_facts(placement(0))
                        .with_create_readiness(NativeCreateReadiness::Created, Some(first_created)),
                ),
                (
                    second,
                    window_facts(placement(1)).with_create_readiness(
                        NativeCreateReadiness::Created,
                        Some(second_created),
                    ),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    idle(&mut host.session);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    for binding in [first, second] {
        let [cleanup] = report.take_native_effects().try_into().unwrap();
        assert!(
            matches!(cleanup.operation(), NativeEffectOperation::CompensatingClose { binding: actual } if *actual == binding)
        );
        let Some(NativeEffectAcknowledgement::Close(destroyed)) = cleanup.accepted() else {
            panic!("cleanup keeps its exact capability");
        };
        host.session
            .report_managed_native_snapshot(
                [
                    (host.root, root_facts()),
                    (binding, NativeWindowFacts::destroyed_after(destroyed)),
                ],
                work_areas(),
            )
            .unwrap();
        report = finish(native_frame(&mut host.session));
    }
    assert_no_effects(&mut report);
    host.quiesce(first);
    host.quiesce(second);
    assert!(!host.session.has_pending_native_create());
    assert_suspended(&mut host.session, &ids.children());
    assert_eq!(host.session.version(), version);
    assert_eq!(assert_content(host.session.view()), ids);
}

#[test]
fn definite_policy_rejection_stops_automatic_intents_until_an_explicit_retry() {
    let bytes = saved_document(SavedHosting::Children);
    let mut host = NativeDocumentHost::new(8, true);
    let old_root = host.root;
    let (ids, mut report) = host.restore(&bytes);
    assert_no_effects(&mut report);
    host.quiesce(old_root);
    host.ready_root();
    let mut frame = native_frame(&mut host.session);
    frame.replace_policy(DockPolicy::default()).unwrap();
    finish(frame);
    let mut frame = native_frame(&mut host.session);
    assert!(frame.advance_next_native_surface_create().unwrap());
    assert!(frame.advance_next_native_surface_create().unwrap());
    assert!(!frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    assert_no_effects(&mut report);
    for surface in ids.children() {
        assert!(
            report
                .inputs()
                .contains(&HostInputOutcome::NativeSurfaceCreateRejected {
                    surface,
                    reason: dockspace::model::DockspaceActionRejection::PolicyDenied,
                })
        );
    }
    assert!(!host.session.has_pending_native_create());
    assert_suspended(&mut host.session, &ids.children());
    let mut frame = native_frame(&mut host.session);
    frame.replace_policy(policy()).unwrap();
    finish(frame);
    host.assert_waiting();
    let mut frame = native_frame(&mut host.session);
    frame
        .create_native_surface_current(
            ids.children()[0],
            HOST,
            NativeWindowPlacement::new(placement(0)),
        )
        .unwrap();
    let mut report = finish(frame);
    let (binding, created) = take_create(&mut report, ids.children()[0], 0);
    host.admit(binding, created, ids, 0, &[]);
    assert!(!host.session.has_pending_native_create());
    assert_suspended(&mut host.session, &[ids.children()[1]]);
    assert_eq!(assert_content(host.session.view()), ids);
}
