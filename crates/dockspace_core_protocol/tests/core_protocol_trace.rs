use dockspace_core_protocol::{
    AuthorityUnavailableReasonSpec, BaselineId, BoundaryId, CanonicalRoot, CanonicalSurface,
    CanonicalWorkspace, CoordinateGenerationIngress, CoreProtocolHarness, CoreProtocolTrace,
    CoreProtocolTraceBoundary, CoreProtocolTraceCommand, CoreProtocolTraceId,
    CoreProtocolTraceInput, CoreProtocolTraceProvenance, CoreProtocolTraceSuite,
    DesktopRouteIngress, EffectKey, ExpectedCanonicalSnapshot, ExpectedCloseTarget,
    ExpectedFocusDelta, ExpectedFocusValueChange, ExpectedInteractionCancelReason,
    ExpectedInteractionEvent, ExpectedInteractionEventKind, ExpectedInteractionOutcome,
    ExpectedInteractionState, ExpectedPaneFocusDisposition, ExpectedPlatformEffect,
    ExpectedPlatformEffectEmission, ExpectedPointerEdge, ExpectedPointerEdgeCause,
    ExpectedPresentationObservationOutcome, ExpectedPresentationObservationRejection,
    ExpectedPresentedSurfaceAuthority, ExpectedPreviewResolutionStatus, ExpectedPreviewVisual,
    ExpectedRecordedObserveOnlyActivation, ExpectedReducedInteractionOutcome,
    ExpectedReductionCause, ExpectedRetiredPresentation, ExpectedScrollReceiver,
    ExpectedScrollSuppressionReason, ExpectedScrollTerminationReason,
    ExpectedSurfaceContributionOutcome, ExpectedSurfaceSceneDelta, ExpectedSurfaceSceneState,
    ExpectedSurfaceSceneStateKind, ExpectedTabStripControl, ExpectedTransition,
    ExpectedViewportActivationCause, ExpectedViewportActivationRequest,
    ExpectedWorkspaceDeliveryKind, HostFrameEvent, IngressRef, InitialPolicyFixture, InitialRoot,
    InitialWorkspace, ItemCount, ItemKey, ItemLocation, LicenseProvenance, LifecycleIngress,
    MeasurementUnavailableReasonSpec, NodeFixture, ObservedWindowFixture, ObservedWorkAreaFixture,
    PlatformCapabilitiesFixture, PlatformObservationIngress, PlatformRequirementSpec,
    PlatformSnapshotFixture, PointAuthorityIngress, PointFixture, PointerCaptureIngress,
    PointerDeliveryIngress, PointerDropTargetIngress, PointerEdgeIngress, PointerEdgeKindSpec,
    PointerEventDeliveryIngress, PointerHoverIngress, PointerLocationIngress,
    PointerProviderIngress, PointerProviderScopeIngress, PointerReceiverIngress,
    PresentationDispositionIngress, PresentationDispositionSpec, PresentationEndpointRef,
    PresentationObservationIngress, PresentationStreamObservationIngress,
    PresentationStreamObservationSpec, PresentationStreamRef, PresentationUnavailableReasonSpec,
    ProducerId, RectFixture, ReducerTick, RetiredPresentationIngress, RootKey, RootOwner,
    ScrollDeliveryEndpointIngress, ScrollDeltaIngress, ScrollEdgeIngress,
    ScrollModifiersAuthorityIngress, ScrollMomentumAuthorityIngress, ScrollMomentumSpec,
    ScrollPhaseSpec, ScrollVectorFixture, SourceBaseline, SurfaceContributionIngress,
    SurfaceFixture, SurfaceKey, SurfaceMeasurementIngress, VersionExpectation, ViewportRoleSpec,
    WindowInputStateSpec, WindowPresentationStateSpec, WorkAreaRosterObservationFixture,
    decode_core_protocol_trace_suite, replay_core_protocol_trace_suite,
};

const BOUNDS: RectFixture = RectFixture {
    x: 0.0,
    y: 0.0,
    width: 640.0,
    height: 480.0,
};

const fn scene_state(state: ExpectedSurfaceSceneStateKind) -> ExpectedSurfaceSceneState {
    ExpectedSurfaceSceneState {
        state,
        presented: None,
    }
}

const fn scene_delta(
    surface: u64,
    before: ExpectedSurfaceSceneStateKind,
    after: ExpectedSurfaceSceneStateKind,
) -> ExpectedSurfaceSceneDelta {
    ExpectedSurfaceSceneDelta {
        surface: SurfaceKey(surface),
        before: Some(scene_state(before)),
        after: Some(scene_state(after)),
    }
}

const fn presented_scene_state(
    state: ExpectedSurfaceSceneStateKind,
    stream: PresentationStreamRef,
    emission: u64,
    coordinate_generation: u64,
) -> ExpectedSurfaceSceneState {
    ExpectedSurfaceSceneState {
        state,
        presented: Some(ExpectedPresentedSurfaceAuthority {
            stream,
            emission,
            coordinate_generation,
        }),
    }
}

const fn presentation_scene_delta(
    surface: u64,
    before: ExpectedSurfaceSceneState,
    after: ExpectedSurfaceSceneState,
) -> ExpectedSurfaceSceneDelta {
    ExpectedSurfaceSceneDelta {
        surface: SurfaceKey(surface),
        before: Some(before),
        after: Some(after),
    }
}

fn pointer_interaction_event(
    sequence: u64,
    revision: u64,
    event: ExpectedInteractionEventKind,
) -> ExpectedInteractionEvent {
    ExpectedInteractionEvent {
        cause: ExpectedReductionCause::PointerEdge { sequence },
        version: VersionExpectation { epoch: 0, revision },
        event,
    }
}

const fn painted_surface(surface: u64) -> PresentationDispositionIngress {
    PresentationDispositionIngress::Surface {
        surface: SurfaceKey(surface),
        disposition: PresentationDispositionSpec::Painted {},
    }
}

const fn unavailable_surface(surface: u64) -> PresentationDispositionIngress {
    PresentationDispositionIngress::Surface {
        surface: SurfaceKey(surface),
        disposition: PresentationDispositionSpec::Unavailable {
            reason: PresentationUnavailableReasonSpec::OutputNotProduced,
        },
    }
}

const fn unavailable_native_staging(surface: u64) -> PresentationDispositionIngress {
    PresentationDispositionIngress::NativeStaging {
        surface: SurfaceKey(surface),
        disposition: PresentationDispositionSpec::Unavailable {
            reason: PresentationUnavailableReasonSpec::OutputNotProduced,
        },
    }
}

fn painted_native_surfaces() -> Vec<PresentationDispositionIngress> {
    vec![painted_surface(1), painted_surface(2)]
}

fn unavailable_native_surfaces() -> Vec<PresentationDispositionIngress> {
    vec![unavailable_surface(1), unavailable_surface(2)]
}

const fn headless_stream(surface: u64) -> PresentationStreamRef {
    PresentationStreamRef {
        surface: SurfaceKey(surface),
        endpoint: PresentationEndpointRef::Headless,
        sequence: 1,
    }
}

const fn native_root_stream(surface: u64) -> PresentationStreamRef {
    PresentationStreamRef {
        surface: SurfaceKey(surface),
        endpoint: PresentationEndpointRef::Native {
            role: ViewportRoleSpec::Root,
        },
        sequence: 1,
    }
}

fn presented_observation(
    stream: PresentationStreamRef,
    generation: u64,
    settled_through: u64,
) -> PresentationStreamObservationIngress {
    PresentationStreamObservationIngress {
        stream,
        observation: PresentationStreamObservationSpec::Retired {
            generation,
            settled_through,
            presented: RetiredPresentationIngress::Presented {
                emission: settled_through,
            },
        },
    }
}

fn expected_presented(
    stream: PresentationStreamRef,
    generation: u64,
    settled_through: u64,
    retired_output_count: usize,
) -> ExpectedPresentationObservationOutcome {
    ExpectedPresentationObservationOutcome::Presented {
        stream,
        generation,
        settled_through,
        presented: settled_through,
        retired_output_count,
        promotion_eligible: true,
    }
}

const fn no_presentation_update(
    stream: PresentationStreamRef,
) -> PresentationStreamObservationIngress {
    PresentationStreamObservationIngress {
        stream,
        observation: PresentationStreamObservationSpec::NoUpdate,
    }
}

const fn expected_no_presentation_update(
    stream: PresentationStreamRef,
) -> ExpectedPresentationObservationOutcome {
    ExpectedPresentationObservationOutcome::NoUpdate { stream }
}

const fn captured_unknown_presentation(
    stream: PresentationStreamRef,
    generation: u64,
    reason: AuthorityUnavailableReasonSpec,
) -> PresentationStreamObservationIngress {
    PresentationStreamObservationIngress {
        stream,
        observation: PresentationStreamObservationSpec::CapturedUnknown { generation, reason },
    }
}

const fn expected_captured_unknown_presentation(
    stream: PresentationStreamRef,
    generation: u64,
    reason: AuthorityUnavailableReasonSpec,
) -> ExpectedPresentationObservationOutcome {
    ExpectedPresentationObservationOutcome::CapturedUnknown {
        stream,
        generation,
        reason,
    }
}

const fn retired_unknown_presentation(
    stream: PresentationStreamRef,
    generation: u64,
    settled_through: u64,
    reason: AuthorityUnavailableReasonSpec,
) -> PresentationStreamObservationIngress {
    PresentationStreamObservationIngress {
        stream,
        observation: PresentationStreamObservationSpec::Retired {
            generation,
            settled_through,
            presented: RetiredPresentationIngress::Unknown { reason },
        },
    }
}

const fn expected_retired_unknown_presentation(
    stream: PresentationStreamRef,
    generation: u64,
    settled_through: u64,
    retired_output_count: usize,
    reason: AuthorityUnavailableReasonSpec,
) -> ExpectedPresentationObservationOutcome {
    ExpectedPresentationObservationOutcome::Retired {
        stream,
        generation,
        settled_through,
        presented: ExpectedRetiredPresentation::Unknown { reason },
        retired_output_count,
        promotion_eligible: false,
    }
}

const fn expected_rejected_presentation(
    stream: PresentationStreamRef,
    reason: ExpectedPresentationObservationRejection,
) -> ExpectedPresentationObservationOutcome {
    ExpectedPresentationObservationOutcome::Rejected { stream, reason }
}

fn minimal_suite() -> CoreProtocolTraceSuite {
    let node = NodeFixture::Tabs {
        items: vec![ItemKey(1), ItemKey(2)],
        selected: Some(ItemKey(1)),
        mru: vec![ItemKey(1), ItemKey(2)],
    };
    CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: vec![SourceBaseline {
            id: BaselineId("open-gpui".into()),
            project: "open-gpui".into(),
            repository: "repo-ref/open-gpui".into(),
            revision: dockspace_core_protocol::OPEN_GPUI_BASELINE_REVISION.into(),
            license: LicenseProvenance {
                expression: "Apache-2.0".into(),
                file: "repo-ref/open-gpui/LICENSE-APACHE".into(),
            },
        }],
        traces: vec![CoreProtocolTrace {
            id: CoreProtocolTraceId("bootstrap".into()),
            provenance: CoreProtocolTraceProvenance {
                baseline: BaselineId("open-gpui".into()),
                path: "fixture".into(),
                test: "bootstrap".into(),
                retained_behavior: "ready measurement compiles one scene".into(),
                deliberate_strengthening: "pointer facts remain outside platform snapshots".into(),
            },
            initial_workspace: InitialWorkspace {
                policy: Default::default(),
                roots: vec![InitialRoot {
                    id: RootKey(1),
                    central_path: Some(dockspace_core_protocol::StructuralPath(vec![])),
                    node: node.clone(),
                }],
                surfaces: vec![SurfaceFixture {
                    id: SurfaceKey(1),
                    main_root: Some(RootKey(1)),
                    contained: vec![],
                }],
            },
            boundaries: vec![CoreProtocolTraceBoundary {
                id: BoundaryId("bootstrap".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![unavailable_surface(1)],
                events: vec![],
                surface_contributions: vec![SurfaceContributionIngress {
                    surface: SurfaceKey(1),
                    measurements: SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                }],
                expected: ExpectedTransition {
                    tick: ReducerTick(1),
                    before: VersionExpectation {
                        epoch: 0,
                        revision: 0,
                    },
                    after: VersionExpectation {
                        epoch: 0,
                        revision: 0,
                    },
                    reduced: vec![],
                    reduced_interaction_outcomes: vec![],
                    reduced_pointer_edges: vec![],
                    presentation_observations: vec![],
                    presentation_emissions: 0,
                    surface_contributions: vec![
                        dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Ready {
                            surface: SurfaceKey(1),
                        },
                    ],
                    interaction_events: vec![],
                    platform_effects: vec![],
                    focus_delta: Default::default(),
                    surface_scene_deltas: vec![scene_delta(
                        1,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                        ExpectedSurfaceSceneStateKind::Ready,
                    )],
                    interaction: ExpectedInteractionState::Idle,
                    interactive_surface_roster: vec![],
                    published_state_changed: true,
                },
            }],
            expected_final: ExpectedCanonicalSnapshot {
                workspace: CanonicalWorkspace {
                    version: VersionExpectation {
                        epoch: 0,
                        revision: 0,
                    },
                    roots: vec![CanonicalRoot {
                        id: RootKey(1),
                        central_path: Some(dockspace_core_protocol::StructuralPath(vec![])),
                        owner: RootOwner::Main {
                            surface: SurfaceKey(1),
                        },
                        node,
                    }],
                    surfaces: vec![CanonicalSurface {
                        id: SurfaceKey(1),
                        main_root: Some(RootKey(1)),
                        contained: vec![],
                    }],
                    item_multiset: vec![
                        ItemCount {
                            item: ItemKey(1),
                            count: 1,
                        },
                        ItemCount {
                            item: ItemKey(2),
                            count: 1,
                        },
                    ],
                    item_owners: vec![
                        dockspace_core_protocol::ItemOwner {
                            item: ItemKey(1),
                            root: RootKey(1),
                            path: dockspace_core_protocol::StructuralPath(vec![]),
                            owner: RootOwner::Main {
                                surface: SurfaceKey(1),
                            },
                        },
                        dockspace_core_protocol::ItemOwner {
                            item: ItemKey(2),
                            root: RootKey(1),
                            path: dockspace_core_protocol::StructuralPath(vec![]),
                            owner: RootOwner::Main {
                                surface: SurfaceKey(1),
                            },
                        },
                    ],
                },
            },
        }],
    }
}

fn retained() -> SurfaceContributionIngress {
    SurfaceContributionIngress {
        surface: SurfaceKey(1),
        measurements: SurfaceMeasurementIngress::Retained {},
    }
}

fn local_edge(
    sequence: u64,
    kind: PointerEdgeKindSpec,
    x: f64,
    y: f64,
    delivery: Option<PointerDeliveryIngress>,
    hover: Option<PointerHoverIngress>,
) -> PointerEdgeIngress {
    PointerEdgeIngress {
        sequence,
        pointer: 7,
        kind,
        location: PointerLocationIngress::SurfaceLocal {
            position: PointAuthorityIngress::Known {
                point: PointFixture { x, y },
            },
        },
        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
        capture: PointerCaptureIngress::ProviderEndpoint,
        receiver: PointerReceiverIngress::Presented { delivery, hover },
    }
}

fn local_not_applicable_edge(
    sequence: u64,
    kind: PointerEdgeKindSpec,
    x: f64,
    y: f64,
) -> PointerEdgeIngress {
    PointerEdgeIngress {
        sequence,
        pointer: 7,
        kind,
        location: PointerLocationIngress::SurfaceLocal {
            position: PointAuthorityIngress::Known {
                point: PointFixture { x, y },
            },
        },
        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
        capture: PointerCaptureIngress::ProviderEndpoint,
        receiver: PointerReceiverIngress::NotApplicable,
    }
}

fn center_hover(surface: u64, root: u64) -> PointerHoverIngress {
    PointerHoverIngress::DockTarget {
        surface: SurfaceKey(surface),
        target: PointerDropTargetIngress::Center {
            root: RootKey(root),
            path: dockspace_core_protocol::StructuralPath(vec![]),
        },
    }
}

fn empty_expected(
    tick: u64,
    emissions: usize,
    contribution: dockspace_core_protocol::ExpectedSurfaceContributionOutcome,
    interaction: ExpectedInteractionState,
    changed: bool,
) -> ExpectedTransition {
    ExpectedTransition {
        tick: ReducerTick(tick),
        before: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        after: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        reduced: vec![],
        reduced_interaction_outcomes: vec![],
        reduced_pointer_edges: vec![],
        presentation_observations: vec![],
        presentation_emissions: emissions,
        surface_contributions: vec![contribution],
        interaction_events: vec![],
        platform_effects: vec![],
        focus_delta: Default::default(),
        surface_scene_deltas: vec![],
        interaction,
        interactive_surface_roster: vec![],
        published_state_changed: changed,
    }
}

fn selection_command_trace() -> CoreProtocolTrace {
    let mut trace = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace");
    trace.id = CoreProtocolTraceId("workspace-command-selection".into());
    trace.provenance.test = "select_updates_mru_atomically".into();
    trace.provenance.retained_behavior =
        "A semantic selection command updates selection, MRU, and workspace revision atomically."
            .into();
    trace.provenance.deliberate_strengthening =
        "The command shares the same host-frame reducer as presentation and surface facts.".into();
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("select-second-tab".into()),
        provider: None,
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![unavailable_surface(1)],
        events: vec![HostFrameEvent::SemanticInput {
            producer: ProducerId("workspace".into()),
            source_sequence: 1,
            input: CoreProtocolTraceInput::WorkspaceCommand {
                command: CoreProtocolTraceCommand::Select {
                    source: ItemLocation {
                        root: RootKey(1),
                        path: dockspace_core_protocol::StructuralPath(vec![]),
                        item: ItemKey(2),
                    },
                },
            },
        }],
        surface_contributions: vec![SurfaceContributionIngress {
            surface: SurfaceKey(1),
            measurements: SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
        }],
        expected: ExpectedTransition {
            tick: ReducerTick(2),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 1,
            },
            reduced: vec![IngressRef {
                producer: ProducerId("workspace".into()),
                source_sequence: 1,
            }],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![],
            presentation_observations: vec![],
            presentation_emissions: 0,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Ready {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![scene_delta(
                1,
                ExpectedSurfaceSceneStateKind::Ready,
                ExpectedSurfaceSceneStateKind::Ready,
            )],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![],
            published_state_changed: true,
        },
    });
    trace.expected_final.workspace.version.revision = 1;
    let NodeFixture::Tabs { selected, mru, .. } = &mut trace.expected_final.workspace.roots[0].node
    else {
        panic!("command trace root must remain a tab stack");
    };
    *selected = Some(ItemKey(2));
    *mru = vec![ItemKey(2), ItemKey(1)];
    trace
}

fn local_pointer_setup_suite() -> CoreProtocolTraceSuite {
    let mut suite = minimal_suite();
    let trace = &mut suite.traces[0];
    trace.id = CoreProtocolTraceId("local-pointer".into());
    trace.provenance.path = "docs/knowledge/pointer-edge-journal-contract.md".into();
    trace.provenance.test = "PEJ-01, PEJ-05, PEJ-14".into();
    trace.provenance.retained_behavior =
        "Ordered release and press edges settle independently; a missing release never commits."
            .into();
    trace.provenance.deliberate_strengthening =
        "The harness binds explicit semantic receiver identities to core-frozen probes.".into();
    trace.boundaries = vec![
        trace.boundaries[0].clone(),
        CoreProtocolTraceBoundary {
            id: BoundaryId("paint".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![],
            surface_contributions: vec![retained()],
            expected: empty_expected(
                2,
                1,
                dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Retained {
                    surface: SurfaceKey(1),
                },
                ExpectedInteractionState::Idle,
                false,
            ),
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("settle-paint".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::Batch {
                observations: vec![presented_observation(headless_stream(1), 1, 1)],
            },
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![],
            surface_contributions: vec![retained()],
            expected: ExpectedTransition {
                presentation_observations: vec![expected_presented(headless_stream(1), 1, 1, 1)],
                presentation_emissions: 1,
                surface_scene_deltas: vec![presentation_scene_delta(
                    1,
                    scene_state(ExpectedSurfaceSceneStateKind::Ready),
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(1),
                        1,
                        0,
                    ),
                )],
                interactive_surface_roster: vec![SurfaceKey(1)],
                ..empty_expected(
                    3,
                    0,
                    dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Retained {
                        surface: SurfaceKey(1),
                    },
                    ExpectedInteractionState::Idle,
                    true,
                )
            },
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("press".into()),
            provider: Some(PointerProviderIngress::Activate {
                scope: PointerProviderScopeIngress::SurfaceLocal {
                    surface: SurfaceKey(1),
                },
                committed_through: 0,
            }),
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 0,
                through: 1,
                edges: vec![PointerEdgeIngress {
                    sequence: 1,
                    pointer: 7,
                    kind: PointerEdgeKindSpec::PrimaryPressed,
                    location: PointerLocationIngress::SurfaceLocal {
                        position: dockspace_core_protocol::PointAuthorityIngress::Known {
                            point: dockspace_core_protocol::PointFixture { x: 96.0, y: 24.0 },
                        },
                    },
                    delivery: PointerEventDeliveryIngress::ProviderEndpoint,
                    capture: PointerCaptureIngress::ProviderEndpoint,
                    receiver: PointerReceiverIngress::Presented {
                        delivery: Some(PointerDeliveryIngress::Tab { item: ItemKey(1) }),
                        hover: None,
                    },
                }],
            }],
            surface_contributions: vec![retained()],
            expected: ExpectedTransition {
                tick: ReducerTick(4),
                before: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                after: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                reduced: vec![],
                reduced_interaction_outcomes: vec![],
                reduced_pointer_edges: vec![dockspace_core_protocol::ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 1,
                    outcomes: vec![dockspace_core_protocol::ExpectedInteractionOutcome::DragArmed],
                }],
                presentation_observations: vec![],
                presentation_emissions: 1,
                surface_contributions: vec![
                    dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Retained {
                        surface: SurfaceKey(1),
                    },
                ],
                interaction_events: vec![],
                platform_effects: vec![],
                focus_delta: Default::default(),
                surface_scene_deltas: vec![],
                interaction: ExpectedInteractionState::Armed,
                interactive_surface_roster: vec![SurfaceKey(1)],
                published_state_changed: true,
            },
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("move".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 1,
                through: 2,
                edges: vec![local_edge(
                    2,
                    PointerEdgeKindSpec::Moved,
                    320.0,
                    254.0,
                    None,
                    Some(center_hover(1, 1)),
                )],
            }],
            surface_contributions: vec![retained()],
            expected: ExpectedTransition {
                tick: ReducerTick(5),
                before: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                after: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                reduced: vec![],
                reduced_interaction_outcomes: vec![],
                reduced_pointer_edges: vec![ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 2,
                    outcomes: vec![
                        ExpectedInteractionOutcome::DragBegan,
                        ExpectedInteractionOutcome::Preview {
                            status: ExpectedPreviewResolutionStatus::Resolved,
                        },
                    ],
                }],
                presentation_observations: vec![],
                presentation_emissions: 1,
                surface_contributions: vec![
                    dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Retained {
                        surface: SurfaceKey(1),
                    },
                ],
                interaction_events: vec![pointer_interaction_event(
                    2,
                    0,
                    ExpectedInteractionEventKind::PreviewPublished {
                        visual: ExpectedPreviewVisual::Dock {
                            surface: SurfaceKey(1),
                            target: PointerDropTargetIngress::Center {
                                root: RootKey(1),
                                path: dockspace_core_protocol::StructuralPath(vec![]),
                            },
                            rect: RectFixture {
                                x: 0.0,
                                y: 28.0,
                                width: 640.0,
                                height: 452.0,
                            },
                        },
                    },
                )],
                platform_effects: vec![],
                focus_delta: Default::default(),
                surface_scene_deltas: vec![],
                interaction: ExpectedInteractionState::Dragging,
                interactive_surface_roster: vec![SurfaceKey(1)],
                published_state_changed: true,
            },
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("no-release-edge".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::Batch {
                observations: vec![presented_observation(headless_stream(1), 2, 4)],
            },
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 2,
                through: 2,
                edges: vec![],
            }],
            surface_contributions: vec![retained()],
            expected: ExpectedTransition {
                tick: ReducerTick(6),
                before: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                after: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                reduced: vec![],
                reduced_interaction_outcomes: vec![],
                reduced_pointer_edges: vec![],
                presentation_observations: vec![expected_presented(headless_stream(1), 2, 4, 3)],
                presentation_emissions: 1,
                surface_contributions: vec![
                    dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Retained {
                        surface: SurfaceKey(1),
                    },
                ],
                interaction_events: vec![],
                platform_effects: vec![],
                focus_delta: Default::default(),
                surface_scene_deltas: vec![presentation_scene_delta(
                    1,
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(1),
                        1,
                        0,
                    ),
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(1),
                        4,
                        0,
                    ),
                )],
                interaction: ExpectedInteractionState::Dragging,
                interactive_surface_roster: vec![SurfaceKey(1)],
                published_state_changed: true,
            },
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("release-a-then-press-b".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::Batch {
                observations: vec![presented_observation(headless_stream(1), 3, 5)],
            },
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 2,
                through: 4,
                edges: vec![
                    local_edge(
                        3,
                        PointerEdgeKindSpec::PrimaryReleased,
                        320.0,
                        254.0,
                        None,
                        Some(center_hover(1, 1)),
                    ),
                    local_edge(
                        4,
                        PointerEdgeKindSpec::PrimaryPressed,
                        160.0,
                        24.0,
                        Some(PointerDeliveryIngress::Tab { item: ItemKey(2) }),
                        None,
                    ),
                ],
            }],
            surface_contributions: vec![SurfaceContributionIngress {
                surface: SurfaceKey(1),
                measurements: SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
            }],
            expected: ExpectedTransition {
                tick: ReducerTick(7),
                before: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                after: VersionExpectation {
                    epoch: 0,
                    revision: 1,
                },
                reduced: vec![],
                reduced_interaction_outcomes: vec![],
                reduced_pointer_edges: vec![
                    ExpectedPointerEdge {
                        cause: ExpectedPointerEdgeCause::PointerEdge,
                        provider_incarnation: 1,
                        stream_incarnation: 1,
                        sequence: 3,
                        outcomes: vec![
                            ExpectedInteractionOutcome::Preview {
                                status: ExpectedPreviewResolutionStatus::Resolved,
                            },
                            ExpectedInteractionOutcome::DragDelivered,
                        ],
                    },
                    ExpectedPointerEdge {
                        cause: ExpectedPointerEdgeCause::PointerEdge,
                        provider_incarnation: 1,
                        stream_incarnation: 1,
                        sequence: 4,
                        outcomes: vec![ExpectedInteractionOutcome::DragArmed],
                    },
                ],
                presentation_observations: vec![expected_presented(headless_stream(1), 3, 5, 1)],
                presentation_emissions: 1,
                surface_contributions: vec![
                    dockspace_core_protocol::ExpectedSurfaceContributionOutcome::Ready {
                        surface: SurfaceKey(1),
                    },
                ],
                interaction_events: vec![pointer_interaction_event(
                    3,
                    0,
                    ExpectedInteractionEventKind::Delivered {
                        delivery: ExpectedWorkspaceDeliveryKind::Dock,
                    },
                )],
                platform_effects: vec![],
                focus_delta: Default::default(),
                surface_scene_deltas: vec![presentation_scene_delta(
                    1,
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(1),
                        4,
                        0,
                    ),
                    scene_state(ExpectedSurfaceSceneStateKind::Ready),
                )],
                interaction: ExpectedInteractionState::Armed,
                interactive_surface_roster: vec![],
                published_state_changed: true,
            },
        },
    ];
    trace.expected_final.workspace.version.revision = 1;
    let NodeFixture::Tabs { selected, mru, .. } = &mut trace.expected_final.workspace.roots[0].node
    else {
        panic!("local trace root must remain a tab stack");
    };
    *selected = Some(ItemKey(2));
    *mru = vec![ItemKey(2), ItemKey(1)];
    suite.traces.push(selection_command_trace());
    suite
}

fn provider_retirement_suite() -> CoreProtocolTraceSuite {
    let mut suite = local_pointer_setup_suite();
    suite.traces.truncate(1);
    let trace = &mut suite.traces[0];
    trace.boundaries.truncate(4);
    trace.id = CoreProtocolTraceId("pointer-provider-retirement".into());
    trace.provenance.path = "docs/knowledge/pointer-edge-journal-contract.md".into();
    trace.provenance.test = "PEJ-03 provider retirement boundary".into();
    trace.provenance.retained_behavior =
        "Pointer-provider retirement cancels its exact gesture owner and advances one reducer tick."
            .into();
    trace.provenance.deliberate_strengthening =
        "Retirement is a complete transition and cannot be followed by an implicit empty host frame."
            .into();
    trace.expected_final = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace")
        .expected_final;
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("retire-provider".into()),
        provider: Some(PointerProviderIngress::Retire {}),
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![],
        events: vec![],
        surface_contributions: vec![],
        expected: ExpectedTransition {
            tick: ReducerTick(5),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![],
            presentation_observations: vec![],
            presentation_emissions: 0,
            surface_contributions: vec![],
            interaction_events: vec![ExpectedInteractionEvent {
                cause: dockspace_core_protocol::ExpectedReductionCause::PointerProviderRetirement,
                version: VersionExpectation {
                    epoch: 0,
                    revision: 0,
                },
                event: ExpectedInteractionEventKind::Cancelled {
                    status: ExpectedInteractionState::Armed,
                    reason: ExpectedInteractionCancelReason::PointerProviderRetired,
                },
            }],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });
    suite
}

fn scroll_receiver() -> ExpectedScrollReceiver {
    ExpectedScrollReceiver::TabStrip {
        surface: SurfaceKey(1),
        root: RootKey(1),
        path: dockspace_core_protocol::StructuralPath(vec![]),
    }
}

fn scroll_edge(
    sequence: u64,
    phase: ScrollPhaseSpec,
    delta: Option<ScrollDeltaIngress>,
    momentum: ScrollMomentumAuthorityIngress,
    modifiers: ScrollModifiersAuthorityIngress,
    endpoint: ScrollDeliveryEndpointIngress,
    receiver: PointerReceiverIngress,
) -> PointerEdgeIngress {
    PointerEdgeIngress {
        sequence,
        pointer: 7,
        kind: PointerEdgeKindSpec::Scrolled {
            scroll: ScrollEdgeIngress {
                device: 9,
                sequence: Some(41),
                phase,
                delta,
                momentum,
                modifiers,
                delivery: endpoint,
            },
        },
        location: PointerLocationIngress::SurfaceLocal {
            position: PointAuthorityIngress::Known {
                point: PointFixture { x: 320.0, y: 14.0 },
            },
        },
        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
        capture: PointerCaptureIngress::None,
        receiver,
    }
}

fn known_scroll_receiver() -> PointerReceiverIngress {
    PointerReceiverIngress::Presented {
        delivery: Some(PointerDeliveryIngress::TabStripScroll {
            root: RootKey(1),
            path: dockspace_core_protocol::StructuralPath(vec![]),
        }),
        hover: None,
    }
}

fn scroll_expected(
    tick: u64,
    sequence: u64,
    outcomes: Vec<ExpectedInteractionOutcome>,
    contribution: ExpectedSurfaceContributionOutcome,
    interactive: bool,
    published_state_changed: bool,
) -> ExpectedTransition {
    ExpectedTransition {
        tick: ReducerTick(tick),
        before: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        after: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        reduced: vec![],
        reduced_interaction_outcomes: vec![],
        reduced_pointer_edges: vec![ExpectedPointerEdge {
            cause: ExpectedPointerEdgeCause::PointerEdge,
            provider_incarnation: 1,
            stream_incarnation: 1,
            sequence,
            outcomes,
        }],
        presentation_observations: vec![],
        presentation_emissions: 1,
        surface_contributions: vec![contribution],
        interaction_events: vec![],
        platform_effects: vec![],
        focus_delta: Default::default(),
        surface_scene_deltas: vec![],
        interaction: ExpectedInteractionState::Idle,
        interactive_surface_roster: interactive.then_some(SurfaceKey(1)).into_iter().collect(),
        published_state_changed,
    }
}

fn scroll_trace_suite() -> CoreProtocolTraceSuite {
    let mut suite = local_pointer_setup_suite();
    suite.traces.truncate(1);
    let trace = &mut suite.traces[0];
    trace.boundaries.truncate(3);
    trace.id = CoreProtocolTraceId("ordered-scroll-journal".into());
    trace.provenance.path = "docs/knowledge/pointer-scroll-journal-contract.md".into();
    trace.provenance.test = "PEJ-03, PEJ-06; scroll smooth FSM, lifecycle ABA, rollback".into();
    trace.provenance.retained_behavior =
        "A phaseful scroll sequence locks one explicit tab strip, preserves its owner through Unknown receiver authority, and terminates exactly once."
            .into();
    trace.provenance.deliberate_strengthening =
        "The fixture supplies the structural receiver receipt and every raw device, unit, phase, modifier, momentum, endpoint, and token fact."
            .into();
    trace.expected_final.workspace.version.revision = 0;

    let items = (1..=12).map(ItemKey).collect::<Vec<_>>();
    let tabs = NodeFixture::Tabs {
        items: items.clone(),
        selected: Some(ItemKey(1)),
        mru: items.clone(),
    };
    trace.initial_workspace.roots[0].node = tabs.clone();
    trace.expected_final.workspace.roots[0].node = tabs;
    trace.expected_final.workspace.item_multiset = items
        .iter()
        .copied()
        .map(|item| ItemCount { item, count: 1 })
        .collect();
    trace.expected_final.workspace.item_owners = items
        .iter()
        .copied()
        .map(|item| dockspace_core_protocol::ItemOwner {
            item,
            root: RootKey(1),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(1),
            },
        })
        .collect();

    let known_endpoint = ScrollDeliveryEndpointIngress::Headless {
        surface: SurfaceKey(1),
        coordinate_generation: CoordinateGenerationIngress::Current,
    };
    let direct = ScrollMomentumAuthorityIngress::Known {
        momentum: ScrollMomentumSpec::Direct,
    };
    let momentum = ScrollMomentumAuthorityIngress::Known {
        momentum: ScrollMomentumSpec::Momentum,
    };
    let plain_modifiers = ScrollModifiersAuthorityIngress::Known {
        shift: false,
        control: false,
        alt: false,
        command: false,
    };
    let line_delta = ScrollDeltaIngress::Lines {
        delta: ScrollVectorFixture { x: -1.0, y: 7.0 },
    };

    trace.boundaries.extend([
        CoreProtocolTraceBoundary {
            id: BoundaryId("scroll-begin".into()),
            provider: Some(PointerProviderIngress::Activate {
                scope: PointerProviderScopeIngress::SurfaceLocal {
                    surface: SurfaceKey(1),
                },
                committed_through: 0,
            }),
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 0,
                through: 1,
                edges: vec![scroll_edge(
                    1,
                    ScrollPhaseSpec::Begin,
                    None,
                    direct,
                    ScrollModifiersAuthorityIngress::Known {
                        shift: true,
                        control: false,
                        alt: false,
                        command: false,
                    },
                    known_endpoint,
                    known_scroll_receiver(),
                )],
            }],
            surface_contributions: vec![retained()],
            expected: scroll_expected(
                4,
                1,
                vec![ExpectedInteractionOutcome::ScrollBegan {
                    receiver: scroll_receiver(),
                }],
                ExpectedSurfaceContributionOutcome::Retained {
                    surface: SurfaceKey(1),
                },
                true,
                false,
            ),
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("scroll-update-receiver-unknown".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 1,
                through: 2,
                edges: vec![scroll_edge(
                    2,
                    ScrollPhaseSpec::Update,
                    Some(line_delta),
                    momentum,
                    ScrollModifiersAuthorityIngress::Unknown {
                        reason: AuthorityUnavailableReasonSpec::NotReported,
                    },
                    known_endpoint,
                    PointerReceiverIngress::Unknown {
                        reason: dockspace_core_protocol::PointerReceiverUnknownReasonSpec::EventCorrelationUnavailable,
                    },
                )],
            }],
            surface_contributions: vec![retained()],
            expected: scroll_expected(
                5,
                2,
                vec![ExpectedInteractionOutcome::ScrollSuppressed {
                    session: true,
                    sequence: Some(41),
                    phase: ScrollPhaseSpec::Update,
                    reason: ExpectedScrollSuppressionReason::ReceiverUnknown,
                }],
                ExpectedSurfaceContributionOutcome::Retained {
                    surface: SurfaceKey(1),
                },
                true,
                false,
            ),
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("scroll-update-applied".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 2,
                through: 3,
                edges: vec![scroll_edge(
                    3,
                    ScrollPhaseSpec::Update,
                    Some(line_delta),
                    momentum,
                    ScrollModifiersAuthorityIngress::Known {
                        shift: false,
                        control: true,
                        alt: true,
                        command: false,
                    },
                    known_endpoint,
                    known_scroll_receiver(),
                )],
            }],
            surface_contributions: vec![SurfaceContributionIngress {
                surface: SurfaceKey(1),
                measurements: SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
            }],
            expected: ExpectedTransition {
                surface_scene_deltas: vec![presentation_scene_delta(
                    1,
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(1),
                        1,
                        0,
                    ),
                    scene_state(ExpectedSurfaceSceneStateKind::Ready),
                )],
                ..scroll_expected(
                    6,
                    3,
                    vec![ExpectedInteractionOutcome::ScrollApplied {
                        session: true,
                        receiver: scroll_receiver(),
                        phase: ScrollPhaseSpec::Update,
                        requested_delta: 40.0,
                        applied_delta: 40.0,
                        unapplied_delta: 0.0,
                        offset: 40.0,
                    }],
                    ExpectedSurfaceContributionOutcome::Ready {
                        surface: SurfaceKey(1),
                    },
                    false,
                    true,
                )
            },
        },
        CoreProtocolTraceBoundary {
            id: BoundaryId("scroll-end-with-unknown-receiver".into()),
            provider: None,
            presentation_observation: PresentationObservationIngress::NoUpdate {},
            presentation_dispositions: vec![painted_surface(1)],
            events: vec![HostFrameEvent::PointerJournal {
                previous: 3,
                through: 4,
                edges: vec![scroll_edge(
                    4,
                    ScrollPhaseSpec::End,
                    None,
                    ScrollMomentumAuthorityIngress::Unknown {
                        reason: AuthorityUnavailableReasonSpec::NotReported,
                    },
                    plain_modifiers,
                    ScrollDeliveryEndpointIngress::Unknown {
                        reason: AuthorityUnavailableReasonSpec::NotReported,
                    },
                    PointerReceiverIngress::Unknown {
                        reason: dockspace_core_protocol::PointerReceiverUnknownReasonSpec::NotReported,
                    },
                )],
            }],
            surface_contributions: vec![SurfaceContributionIngress {
                surface: SurfaceKey(1),
                measurements: SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
            }],
            expected: ExpectedTransition {
                surface_scene_deltas: vec![scene_delta(
                    1,
                    ExpectedSurfaceSceneStateKind::Ready,
                    ExpectedSurfaceSceneStateKind::Ready,
                )],
                ..scroll_expected(
                    7,
                    4,
                    vec![ExpectedInteractionOutcome::ScrollTerminated {
                        receiver: Some(scroll_receiver()),
                        reason: ExpectedScrollTerminationReason::Completed,
                    }],
                    ExpectedSurfaceContributionOutcome::Ready {
                        surface: SurfaceKey(1),
                    },
                    false,
                    true,
                )
            },
        },
    ]);
    suite
}

const TAB_CLOSE_POINT: PointFixture = PointFixture { x: 112.0, y: 14.0 };

fn expected_item_close(reused: bool) -> ExpectedInteractionOutcome {
    ExpectedInteractionOutcome::CloseRequested {
        target: ExpectedCloseTarget::Item { item: ItemKey(1) },
        items: vec![ItemKey(1)],
        reused,
    }
}

fn tab_close_edge(
    sequence: u64,
    kind: PointerEdgeKindSpec,
    delivery: PointerDeliveryIngress,
) -> PointerEdgeIngress {
    PointerEdgeIngress {
        sequence,
        pointer: 7,
        kind,
        location: PointerLocationIngress::SurfaceLocal {
            position: PointAuthorityIngress::Known {
                point: TAB_CLOSE_POINT,
            },
        },
        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
        capture: PointerCaptureIngress::ProviderEndpoint,
        receiver: PointerReceiverIngress::Presented {
            delivery: Some(delivery),
            hover: None,
        },
    }
}

fn journal_click_trace(id: &str, terminal: JournalClickTerminal) -> CoreProtocolTrace {
    let mut trace = local_pointer_setup_suite()
        .traces
        .into_iter()
        .next()
        .expect("local pointer suite has one primary trace");
    trace.boundaries.truncate(3);
    trace.id = CoreProtocolTraceId(id.into());
    trace.provenance.path = "docs/knowledge/pointer-edge-journal-contract.md".into();
    trace.provenance.test = "click release protocol".into();
    trace.provenance.retained_behavior =
        "A close control activates only on a matching primary release; terminal mismatch and Escape cancel exactly once."
            .into();
    trace.provenance.deliberate_strengthening =
        "Expected outcomes are fixture-authored and bind semantic Escape to its exact ingress."
            .into();
    trace.expected_final = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace")
        .expected_final;

    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("press-tab-close".into()),
        provider: Some(PointerProviderIngress::Activate {
            scope: PointerProviderScopeIngress::SurfaceLocal {
                surface: SurfaceKey(1),
            },
            committed_through: 0,
        }),
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![HostFrameEvent::PointerJournal {
            previous: 0,
            through: 1,
            edges: vec![tab_close_edge(
                1,
                PointerEdgeKindSpec::PrimaryPressed,
                PointerDeliveryIngress::TabClose { item: ItemKey(1) },
            )],
        }],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(4),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![ExpectedPointerEdge {
                cause: ExpectedPointerEdgeCause::PointerEdge,
                provider_incarnation: 1,
                stream_incarnation: 1,
                sequence: 1,
                outcomes: vec![],
            }],
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Pressed,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });

    let (events, reduced, reduced_interaction_outcomes, reduced_pointer_edges, interaction_events) =
        match terminal {
            JournalClickTerminal::MatchingRelease => (
                vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 2,
                    edges: vec![tab_close_edge(
                        2,
                        PointerEdgeKindSpec::PrimaryReleased,
                        PointerDeliveryIngress::TabClose { item: ItemKey(1) },
                    )],
                }],
                vec![],
                vec![],
                vec![ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 2,
                    outcomes: vec![expected_item_close(false)],
                }],
                vec![],
            ),
            JournalClickTerminal::ReceiverMismatch => (
                vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 2,
                    edges: vec![tab_close_edge(
                        2,
                        PointerEdgeKindSpec::PrimaryReleased,
                        PointerDeliveryIngress::NoReceiver,
                    )],
                }],
                vec![],
                vec![],
                vec![ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 2,
                    outcomes: vec![ExpectedInteractionOutcome::Cancelled {
                        reason: ExpectedInteractionCancelReason::ClickReceiverMismatch,
                    }],
                }],
                vec![pointer_interaction_event(
                    2,
                    0,
                    ExpectedInteractionEventKind::Cancelled {
                        status: ExpectedInteractionState::Pressed,
                        reason: ExpectedInteractionCancelReason::ClickReceiverMismatch,
                    },
                )],
            ),
            JournalClickTerminal::Escape => {
                let ingress = IngressRef {
                    producer: ProducerId("keyboard".into()),
                    source_sequence: 1,
                };
                (
                    vec![
                        HostFrameEvent::SemanticInput {
                            producer: ingress.producer.clone(),
                            source_sequence: ingress.source_sequence,
                            input: CoreProtocolTraceInput::CancelActiveClickWithEscape {},
                        },
                        HostFrameEvent::PointerJournal {
                            previous: 1,
                            through: 1,
                            edges: vec![],
                        },
                    ],
                    vec![ingress.clone()],
                    vec![ExpectedReducedInteractionOutcome {
                        ingress: ingress.clone(),
                        outcome: ExpectedInteractionOutcome::Cancelled {
                            reason: ExpectedInteractionCancelReason::Escape,
                        },
                    }],
                    vec![],
                    vec![ExpectedInteractionEvent {
                        cause: ExpectedReductionCause::Input { ingress },
                        version: VersionExpectation {
                            epoch: 0,
                            revision: 0,
                        },
                        event: ExpectedInteractionEventKind::Cancelled {
                            status: ExpectedInteractionState::Pressed,
                            reason: ExpectedInteractionCancelReason::Escape,
                        },
                    }],
                )
            }
        };
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId(terminal.boundary_id().into()),
        provider: None,
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events,
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(5),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced,
            reduced_interaction_outcomes,
            reduced_pointer_edges,
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events,
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });
    trace
}

fn journal_click_reuse_trace() -> CoreProtocolTrace {
    let mut trace = journal_click_trace(
        "journal-close-existing-plan-reuse",
        JournalClickTerminal::MatchingRelease,
    );
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("press-tab-close-again".into()),
        provider: None,
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![HostFrameEvent::PointerJournal {
            previous: 2,
            through: 3,
            edges: vec![tab_close_edge(
                3,
                PointerEdgeKindSpec::PrimaryPressed,
                PointerDeliveryIngress::TabClose { item: ItemKey(1) },
            )],
        }],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(6),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![ExpectedPointerEdge {
                cause: ExpectedPointerEdgeCause::PointerEdge,
                provider_incarnation: 1,
                stream_incarnation: 1,
                sequence: 3,
                outcomes: vec![],
            }],
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Pressed,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("release-tab-close-reuses-plan".into()),
        provider: None,
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![HostFrameEvent::PointerJournal {
            previous: 3,
            through: 4,
            edges: vec![tab_close_edge(
                4,
                PointerEdgeKindSpec::PrimaryReleased,
                PointerDeliveryIngress::TabClose { item: ItemKey(1) },
            )],
        }],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(7),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![ExpectedPointerEdge {
                cause: ExpectedPointerEdgeCause::PointerEdge,
                provider_incarnation: 1,
                stream_incarnation: 1,
                sequence: 4,
                outcomes: vec![expected_item_close(true)],
            }],
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });
    trace
}

fn journal_click_same_segment_trace() -> CoreProtocolTrace {
    let mut trace = local_pointer_setup_suite()
        .traces
        .into_iter()
        .next()
        .expect("local pointer suite has one primary trace");
    trace.boundaries.truncate(3);
    trace.id = CoreProtocolTraceId("journal-close-same-segment".into());
    trace.provenance.path = "docs/knowledge/pointer-edge-journal-contract.md".into();
    trace.provenance.test = "close_press_then_release_in_one_segment".into();
    trace.provenance.retained_behavior =
        "One ordered journal segment reduces close press before its matching release.".into();
    trace.provenance.deliberate_strengthening =
        "The release outcome is authored by the fixture rather than inferred from final state."
            .into();
    trace.expected_final = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace")
        .expected_final;
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("press-release-tab-close".into()),
        provider: Some(PointerProviderIngress::Activate {
            scope: PointerProviderScopeIngress::SurfaceLocal {
                surface: SurfaceKey(1),
            },
            committed_through: 0,
        }),
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![HostFrameEvent::PointerJournal {
            previous: 0,
            through: 2,
            edges: vec![
                tab_close_edge(
                    1,
                    PointerEdgeKindSpec::PrimaryPressed,
                    PointerDeliveryIngress::TabClose { item: ItemKey(1) },
                ),
                tab_close_edge(
                    2,
                    PointerEdgeKindSpec::PrimaryReleased,
                    PointerDeliveryIngress::TabClose { item: ItemKey(1) },
                ),
            ],
        }],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(4),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![
                ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 1,
                    outcomes: vec![],
                },
                ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 2,
                    outcomes: vec![expected_item_close(false)],
                },
            ],
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: true,
        },
    });
    trace
}

#[derive(Debug, Clone, Copy)]
enum JournalClickTerminal {
    MatchingRelease,
    ReceiverMismatch,
    Escape,
}

impl JournalClickTerminal {
    const fn boundary_id(self) -> &'static str {
        match self {
            Self::MatchingRelease => "release-tab-close",
            Self::ReceiverMismatch => "release-other-receiver",
            Self::Escape => "escape-cancels-click",
        }
    }
}

fn capture_change_edge(sequence: u64, capture: PointerCaptureIngress) -> PointerEdgeIngress {
    PointerEdgeIngress {
        sequence,
        pointer: 7,
        kind: PointerEdgeKindSpec::CaptureChanged,
        location: PointerLocationIngress::SurfaceLocal {
            position: PointAuthorityIngress::Known {
                point: PointFixture { x: 320.0, y: 254.0 },
            },
        },
        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
        capture,
        receiver: PointerReceiverIngress::NotApplicable,
    }
}

fn active_local_drag_trace(id: &str) -> CoreProtocolTrace {
    let mut trace = local_pointer_setup_suite()
        .traces
        .into_iter()
        .next()
        .expect("local pointer suite has one primary trace");
    trace.boundaries.truncate(5);
    trace.id = CoreProtocolTraceId(id.into());
    trace.provenance.path = "docs/knowledge/pointer-edge-journal-contract.md".into();
    trace.provenance.test = "PEJ-09".into();
    trace.provenance.retained_behavior =
        "Unknown capture authority preserves the active drag; known capture loss cancels it."
            .into();
    trace.provenance.deliberate_strengthening =
        "Repeated authoritative capture loss must produce exactly one cancellation outcome.".into();
    trace.expected_final = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace")
        .expected_final;
    trace
}

fn capture_change_boundary(
    id: &str,
    edges: Vec<PointerEdgeIngress>,
    outcomes: Vec<Vec<ExpectedInteractionOutcome>>,
    interaction: ExpectedInteractionState,
    published_state_changed: bool,
) -> CoreProtocolTraceBoundary {
    assert_eq!(edges.len(), outcomes.len());
    let through = 2 + u64::try_from(edges.len()).expect("capture fixture edge count fits u64");
    let reduced_pointer_edges = outcomes
        .into_iter()
        .enumerate()
        .map(|(index, outcomes)| ExpectedPointerEdge {
            cause: ExpectedPointerEdgeCause::PointerEdge,
            provider_incarnation: 1,
            stream_incarnation: 1,
            sequence: 3 + u64::try_from(index).expect("capture fixture index fits u64"),
            outcomes,
        })
        .collect::<Vec<_>>();
    let interaction_events = reduced_pointer_edges
        .iter()
        .find_map(|edge| {
            edge.outcomes.iter().find_map(|outcome| match outcome {
                ExpectedInteractionOutcome::Cancelled { reason } => {
                    Some(pointer_interaction_event(
                        edge.sequence,
                        0,
                        ExpectedInteractionEventKind::Cancelled {
                            status: ExpectedInteractionState::Dragging,
                            reason: *reason,
                        },
                    ))
                }
                _ => None,
            })
        })
        .into_iter()
        .collect();
    CoreProtocolTraceBoundary {
        id: BoundaryId(id.into()),
        provider: None,
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![HostFrameEvent::PointerJournal {
            previous: 2,
            through,
            edges,
        }],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(6),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges,
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events,
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed,
        },
    }
}

fn capture_change_suite() -> CoreProtocolTraceSuite {
    let unknown_capture = PointerCaptureIngress::Unknown {
        reason: AuthorityUnavailableReasonSpec::NotReported,
    };
    let mut unknown = active_local_drag_trace("capture-unknown");
    unknown.boundaries.push(capture_change_boundary(
        "capture-unknown-retains-drag",
        vec![capture_change_edge(3, unknown_capture.clone())],
        vec![vec![]],
        ExpectedInteractionState::Dragging,
        true,
    ));

    let known_loss_trace = |id: &str, capture: PointerCaptureIngress| {
        let mut trace = active_local_drag_trace(id);
        trace.boundaries.push(capture_change_boundary(
            "capture-known-loss-cancels-once",
            vec![
                capture_change_edge(3, unknown_capture.clone()),
                capture_change_edge(4, capture.clone()),
                capture_change_edge(5, capture),
            ],
            vec![
                vec![],
                vec![ExpectedInteractionOutcome::Cancelled {
                    reason: ExpectedInteractionCancelReason::CaptureLost,
                }],
                vec![],
            ],
            ExpectedInteractionState::Idle,
            true,
        ));
        trace
    };

    let mut suite = minimal_suite();
    suite.traces = vec![
        unknown,
        known_loss_trace("capture-none", PointerCaptureIngress::None),
        known_loss_trace("capture-foreign", PointerCaptureIngress::Foreign),
    ];
    suite
}

fn native_workspace() -> InitialWorkspace {
    InitialWorkspace {
        policy: Default::default(),
        roots: vec![
            InitialRoot {
                id: RootKey(1),
                central_path: None,
                node: NodeFixture::Tabs {
                    items: vec![ItemKey(1), ItemKey(3)],
                    selected: Some(ItemKey(1)),
                    mru: vec![ItemKey(1), ItemKey(3)],
                },
            },
            InitialRoot {
                id: RootKey(2),
                central_path: None,
                node: NodeFixture::Tabs {
                    items: vec![ItemKey(2)],
                    selected: Some(ItemKey(2)),
                    mru: vec![ItemKey(2)],
                },
            },
        ],
        surfaces: vec![
            SurfaceFixture {
                id: SurfaceKey(1),
                main_root: Some(RootKey(1)),
                contained: vec![],
            },
            SurfaceFixture {
                id: SurfaceKey(2),
                main_root: Some(RootKey(2)),
                contained: vec![],
            },
        ],
    }
}

fn native_snapshot() -> ExpectedCanonicalSnapshot {
    ExpectedCanonicalSnapshot {
        workspace: CanonicalWorkspace {
            version: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            roots: vec![
                CanonicalRoot {
                    id: RootKey(1),
                    central_path: None,
                    owner: RootOwner::Main {
                        surface: SurfaceKey(1),
                    },
                    node: NodeFixture::Tabs {
                        items: vec![ItemKey(1), ItemKey(3)],
                        selected: Some(ItemKey(1)),
                        mru: vec![ItemKey(1), ItemKey(3)],
                    },
                },
                CanonicalRoot {
                    id: RootKey(2),
                    central_path: None,
                    owner: RootOwner::Main {
                        surface: SurfaceKey(2),
                    },
                    node: NodeFixture::Tabs {
                        items: vec![ItemKey(2)],
                        selected: Some(ItemKey(2)),
                        mru: vec![ItemKey(2)],
                    },
                },
            ],
            surfaces: vec![
                CanonicalSurface {
                    id: SurfaceKey(1),
                    main_root: Some(RootKey(1)),
                    contained: vec![],
                },
                CanonicalSurface {
                    id: SurfaceKey(2),
                    main_root: Some(RootKey(2)),
                    contained: vec![],
                },
            ],
            item_multiset: vec![
                ItemCount {
                    item: ItemKey(1),
                    count: 1,
                },
                ItemCount {
                    item: ItemKey(2),
                    count: 1,
                },
                ItemCount {
                    item: ItemKey(3),
                    count: 1,
                },
            ],
            item_owners: vec![
                dockspace_core_protocol::ItemOwner {
                    item: ItemKey(1),
                    root: RootKey(1),
                    path: dockspace_core_protocol::StructuralPath(vec![]),
                    owner: RootOwner::Main {
                        surface: SurfaceKey(1),
                    },
                },
                dockspace_core_protocol::ItemOwner {
                    item: ItemKey(3),
                    root: RootKey(1),
                    path: dockspace_core_protocol::StructuralPath(vec![]),
                    owner: RootOwner::Main {
                        surface: SurfaceKey(1),
                    },
                },
                dockspace_core_protocol::ItemOwner {
                    item: ItemKey(2),
                    root: RootKey(2),
                    path: dockspace_core_protocol::StructuralPath(vec![]),
                    owner: RootOwner::Main {
                        surface: SurfaceKey(2),
                    },
                },
            ],
        },
    }
}

fn popup_workspace() -> InitialWorkspace {
    let mut workspace = native_workspace();
    let items = vec![
        ItemKey(1),
        ItemKey(3),
        ItemKey(4),
        ItemKey(5),
        ItemKey(6),
        ItemKey(7),
        ItemKey(8),
        ItemKey(9),
        ItemKey(10),
        ItemKey(11),
        ItemKey(12),
        ItemKey(13),
    ];
    workspace.roots[0].node = NodeFixture::Tabs {
        items: items.clone(),
        selected: Some(ItemKey(1)),
        mru: items,
    };
    workspace
}

fn popup_snapshot() -> ExpectedCanonicalSnapshot {
    let mut snapshot = native_snapshot();
    snapshot.workspace.roots[0].node = popup_workspace().roots.remove(0).node;
    snapshot.workspace.item_multiset = (1..=13)
        .map(|item| ItemCount {
            item: ItemKey(item),
            count: 1,
        })
        .collect();
    snapshot.workspace.item_owners = [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
        .into_iter()
        .map(|item| dockspace_core_protocol::ItemOwner {
            item: ItemKey(item),
            root: RootKey(1),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(1),
            },
        })
        .chain(std::iter::once(dockspace_core_protocol::ItemOwner {
            item: ItemKey(2),
            root: RootKey(2),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(2),
            },
        }))
        .collect();
    snapshot
}

fn native_contribution(
    surface: u64,
    measurements: SurfaceMeasurementIngress,
) -> SurfaceContributionIngress {
    SurfaceContributionIngress {
        surface: SurfaceKey(surface),
        measurements,
    }
}

fn native_contributions(
    measurements: SurfaceMeasurementIngress,
) -> Vec<SurfaceContributionIngress> {
    vec![
        native_contribution(1, measurements.clone()),
        native_contribution(2, measurements),
    ]
}

fn platform_event(sequence: u64, input: CoreProtocolTraceInput) -> HostFrameEvent {
    HostFrameEvent::SemanticInput {
        producer: ProducerId("platform".into()),
        source_sequence: sequence,
        input,
    }
}

fn platform_ref(sequence: u64) -> IngressRef {
    IngressRef {
        producer: ProducerId("platform".into()),
        source_sequence: sequence,
    }
}

#[allow(clippy::too_many_arguments)]
fn native_expected(
    tick: u64,
    reduced: Vec<IngressRef>,
    pointer_edges: Vec<ExpectedPointerEdge>,
    observations: Vec<ExpectedPresentationObservationOutcome>,
    emissions: usize,
    contributions: Vec<ExpectedSurfaceContributionOutcome>,
    interaction: ExpectedInteractionState,
    changed: bool,
) -> ExpectedTransition {
    ExpectedTransition {
        tick: ReducerTick(tick),
        before: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        after: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        reduced,
        reduced_interaction_outcomes: vec![],
        reduced_pointer_edges: pointer_edges,
        presentation_observations: observations,
        presentation_emissions: emissions,
        surface_contributions: contributions,
        interaction_events: vec![],
        platform_effects: vec![],
        focus_delta: Default::default(),
        surface_scene_deltas: match tick {
            1 | 2 => two_surface_scene_deltas(
                ExpectedSurfaceSceneStateKind::Bootstrap,
                ExpectedSurfaceSceneStateKind::Bootstrap,
            ),
            3 => two_surface_scene_deltas(
                ExpectedSurfaceSceneStateKind::Bootstrap,
                ExpectedSurfaceSceneStateKind::Ready,
            ),
            5 => [1, 2]
                .into_iter()
                .map(|surface| {
                    presentation_scene_delta(
                        surface,
                        scene_state(ExpectedSurfaceSceneStateKind::Ready),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            native_root_stream(surface),
                            1,
                            surface + 2,
                        ),
                    )
                })
                .collect(),
            9 => [1, 2]
                .into_iter()
                .map(|surface| {
                    presentation_scene_delta(
                        surface,
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            native_root_stream(surface),
                            1,
                            surface + 2,
                        ),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            native_root_stream(surface),
                            5,
                            surface + 2,
                        ),
                    )
                })
                .collect(),
            _ => vec![],
        },
        interaction,
        interactive_surface_roster: if tick >= 5 {
            vec![SurfaceKey(1), SurfaceKey(2)]
        } else {
            vec![]
        },
        published_state_changed: changed,
    }
}

fn expected_native_contributions(
    outcome: fn(SurfaceKey) -> ExpectedSurfaceContributionOutcome,
) -> Vec<ExpectedSurfaceContributionOutcome> {
    vec![outcome(SurfaceKey(1)), outcome(SurfaceKey(2))]
}

fn two_surface_scene_deltas(
    before: ExpectedSurfaceSceneStateKind,
    after: ExpectedSurfaceSceneStateKind,
) -> Vec<ExpectedSurfaceSceneDelta> {
    vec![scene_delta(1, before, after), scene_delta(2, before, after)]
}

fn ready(surface: SurfaceKey) -> ExpectedSurfaceContributionOutcome {
    ExpectedSurfaceContributionOutcome::Ready { surface }
}

fn retained_outcome(surface: SurfaceKey) -> ExpectedSurfaceContributionOutcome {
    ExpectedSurfaceContributionOutcome::Retained { surface }
}

fn unavailable(surface: SurfaceKey) -> ExpectedSurfaceContributionOutcome {
    ExpectedSurfaceContributionOutcome::Unavailable { surface }
}

fn popup_gate_expected(
    tick: u64,
    observations: Vec<ExpectedPresentationObservationOutcome>,
    emissions: usize,
    contributions: Vec<ExpectedSurfaceContributionOutcome>,
    interactive_surface_roster: Vec<SurfaceKey>,
    changed: bool,
) -> ExpectedTransition {
    ExpectedTransition {
        tick: ReducerTick(tick),
        before: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        after: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        reduced: vec![],
        reduced_interaction_outcomes: vec![],
        reduced_pointer_edges: vec![],
        presentation_observations: observations,
        presentation_emissions: emissions,
        surface_contributions: contributions,
        interaction_events: vec![],
        platform_effects: vec![],
        focus_delta: Default::default(),
        surface_scene_deltas: if tick == 1 {
            two_surface_scene_deltas(
                ExpectedSurfaceSceneStateKind::Bootstrap,
                ExpectedSurfaceSceneStateKind::Ready,
            )
        } else {
            vec![]
        },
        interaction: ExpectedInteractionState::Idle,
        interactive_surface_roster,
        published_state_changed: changed,
    }
}

fn popup_open_ingress() -> IngressRef {
    IngressRef {
        producer: ProducerId("keyboard".into()),
        source_sequence: 1,
    }
}

fn popup_open_event(path: Vec<usize>) -> HostFrameEvent {
    popup_open_event_for(SurfaceKey(1), RootKey(1), path)
}

fn popup_open_event_for(surface: SurfaceKey, root: RootKey, path: Vec<usize>) -> HostFrameEvent {
    let ingress = popup_open_ingress();
    HostFrameEvent::SemanticInput {
        producer: ingress.producer,
        source_sequence: ingress.source_sequence,
        input: CoreProtocolTraceInput::OpenTabListMenu {
            surface,
            bar: dockspace_core_protocol::NodeLocation {
                root,
                path: dockspace_core_protocol::StructuralPath(path),
            },
        },
    }
}

fn popup_open_expected(
    tick: u64,
    observations: Vec<ExpectedPresentationObservationOutcome>,
) -> ExpectedTransition {
    let ingress = popup_open_ingress();
    let surface_scene_deltas = match tick {
        3 => two_surface_scene_deltas(
            ExpectedSurfaceSceneStateKind::Ready,
            ExpectedSurfaceSceneStateKind::Stale,
        ),
        5 => [1, 2]
            .into_iter()
            .map(|surface| {
                presentation_scene_delta(
                    surface,
                    presented_scene_state(
                        ExpectedSurfaceSceneStateKind::Ready,
                        headless_stream(surface),
                        1,
                        0,
                    ),
                    scene_state(ExpectedSurfaceSceneStateKind::Stale),
                )
            })
            .collect(),
        _ => vec![],
    };
    let interaction_events = (tick == 5)
        .then(|| ExpectedInteractionEvent {
            cause: dockspace_core_protocol::ExpectedReductionCause::Input {
                ingress: ingress.clone(),
            },
            version: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            event: ExpectedInteractionEventKind::Cancelled {
                status: ExpectedInteractionState::Resizing,
                reason: ExpectedInteractionCancelReason::SceneUnavailable,
            },
        })
        .into_iter()
        .collect();
    ExpectedTransition {
        reduced: vec![ingress.clone()],
        reduced_interaction_outcomes: vec![ExpectedReducedInteractionOutcome {
            ingress,
            outcome: ExpectedInteractionOutcome::TabStripControlActivated {
                control: ExpectedTabStripControl::TabListMenu,
                changed: true,
                menu_open: true,
            },
        }],
        interaction_events,
        surface_scene_deltas,
        ..popup_gate_expected(
            tick,
            observations,
            0,
            expected_native_contributions(unavailable),
            vec![],
            true,
        )
    }
}

fn deferred_native_contributions() -> Vec<SurfaceContributionIngress> {
    native_contributions(SurfaceMeasurementIngress::Unavailable {
        reason: MeasurementUnavailableReasonSpec::Deferred,
    })
}

fn popup_gate_partition_trace(first_surface: u64) -> CoreProtocolTrace {
    let second_surface = if first_surface == 1 { 2 } else { 1 };
    let first = headless_stream(first_surface);
    let second = headless_stream(second_surface);
    let one = headless_stream(1);
    let two = headless_stream(2);

    CoreProtocolTrace {
        id: CoreProtocolTraceId(format!("popup-gate-{first_surface}-then-{second_surface}")),
        provenance: CoreProtocolTraceProvenance {
            baseline: BaselineId("open-gpui".into()),
            path: "crates/gpui_docking/src/host_viewport_route_tests.rs".into(),
            test: "partitioned_popup_presentation_is_order_independent".into(),
            retained_behavior:
                "No surface becomes interactive until every popup-plane surface has a final presentation proof."
                    .into(),
            deliberate_strengthening:
                "Each provider fact names a stable stream and emission; callback order cannot partially open the global interaction gate."
                    .into(),
        },
        initial_workspace: popup_workspace(),
        boundaries: vec![
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: popup_gate_expected(
                    1,
                    vec![],
                    0,
                    expected_native_contributions(ready),
                    vec![],
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    2,
                    vec![],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("present-inactive-and-open-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(one, 1, 1),
                        presented_observation(two, 1, 1),
                    ],
                },
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![popup_open_event(vec![])],
                surface_contributions: deferred_native_contributions(),
                expected: popup_open_expected(3, vec![
                    expected_presented(one, 1, 1, 1),
                    expected_presented(two, 1, 1, 1),
                ]),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: ExpectedTransition {
                    surface_scene_deltas: two_surface_scene_deltas(
                        ExpectedSurfaceSceneStateKind::Stale,
                        ExpectedSurfaceSceneStateKind::Ready,
                    ),
                    ..popup_gate_expected(
                        4,
                        vec![],
                        0,
                        expected_native_contributions(ready),
                        vec![],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    5,
                    vec![],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId(format!("present-{first_surface}-only")),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(first, 2, 2),
                        no_presentation_update(second),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    6,
                    if first_surface == 1 {
                        vec![
                            expected_presented(first, 2, 2, 1),
                            expected_no_presentation_update(second),
                        ]
                    } else {
                        vec![
                            expected_no_presentation_update(second),
                            expected_presented(first, 2, 2, 1),
                        ]
                    },
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId(format!("present-{second_surface}-completes-roster")),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        no_presentation_update(first),
                        presented_observation(second, 3, 3),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: ExpectedTransition {
                    surface_scene_deltas: [1, 2]
                        .into_iter()
                        .map(|surface| {
                            presentation_scene_delta(
                                surface,
                                scene_state(ExpectedSurfaceSceneStateKind::Ready),
                                presented_scene_state(
                                    ExpectedSurfaceSceneStateKind::Ready,
                                    headless_stream(surface),
                                    if surface == first_surface { 2 } else { 3 },
                                    0,
                                ),
                            )
                        })
                        .collect(),
                    ..popup_gate_expected(
                        7,
                        if first_surface == 1 {
                            vec![
                                expected_no_presentation_update(first),
                                expected_presented(second, 3, 3, 2),
                            ]
                        } else {
                            vec![
                                expected_presented(second, 3, 3, 2),
                                expected_no_presentation_update(first),
                            ]
                        },
                        2,
                        expected_native_contributions(retained_outcome),
                        vec![SurfaceKey(1), SurfaceKey(2)],
                        true,
                    )
                },
            },
        ],
        expected_final: popup_snapshot(),
    }
}

fn popup_gate_unknown_retirement_trace() -> CoreProtocolTrace {
    let first = headless_stream(1);
    let second = headless_stream(2);
    CoreProtocolTrace {
        id: CoreProtocolTraceId("popup-gate-unknown-retirement".into()),
        provenance: CoreProtocolTraceProvenance {
            baseline: BaselineId("open-gpui".into()),
            path: "crates/gpui_docking/src/host_viewport_route_tests.rs".into(),
            test: "unknown_or_unavailable_popup_presentation_fails_closed".into(),
            retained_behavior:
                "Unknown or unavailable final presentation never grants partial interaction authority."
                    .into(),
            deliberate_strengthening:
                "A terminal unavailable fact remains distinct from a non-terminal captured Unknown fact."
                    .into(),
        },
        initial_workspace: popup_workspace(),
        boundaries: vec![
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: popup_gate_expected(
                    1,
                    vec![],
                    0,
                    expected_native_contributions(ready),
                    vec![],
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    2,
                    vec![],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("present-inactive-and-open-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(first, 1, 1),
                        presented_observation(second, 1, 1),
                    ],
                },
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![popup_open_event(vec![])],
                surface_contributions: deferred_native_contributions(),
                expected: popup_open_expected(3, vec![
                    expected_presented(first, 1, 1, 1),
                    expected_presented(second, 1, 1, 1),
                ]),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: ExpectedTransition {
                    surface_scene_deltas: two_surface_scene_deltas(
                        ExpectedSurfaceSceneStateKind::Stale,
                        ExpectedSurfaceSceneStateKind::Ready,
                    ),
                    ..popup_gate_expected(
                        4,
                        vec![],
                        0,
                        expected_native_contributions(ready),
                        vec![],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    5,
                    vec![],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("present-a-capture-b-unknown".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(first, 2, 2),
                        captured_unknown_presentation(
                            second,
                            2,
                            AuthorityUnavailableReasonSpec::NotReported,
                        ),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    6,
                    vec![
                        expected_presented(first, 2, 2, 1),
                        expected_captured_unknown_presentation(
                            second,
                            2,
                            AuthorityUnavailableReasonSpec::NotReported,
                        ),
                    ],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("reject-repeated-b-capture-generation".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        no_presentation_update(first),
                        captured_unknown_presentation(
                            second,
                            2,
                            AuthorityUnavailableReasonSpec::NotReported,
                        ),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    7,
                    vec![
                        expected_no_presentation_update(first),
                        expected_rejected_presentation(
                            second,
                            ExpectedPresentationObservationRejection::CaptureGenerationNotIncreasing,
                        ),
                    ],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("retire-b-as-unavailable".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        no_presentation_update(first),
                        retired_unknown_presentation(
                            second,
                            3,
                            4,
                            AuthorityUnavailableReasonSpec::SurfaceUnavailable,
                        ),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: popup_gate_expected(
                    8,
                    vec![
                        expected_no_presentation_update(first),
                        expected_retired_unknown_presentation(
                            second,
                            3,
                            4,
                            3,
                            AuthorityUnavailableReasonSpec::SurfaceUnavailable,
                        ),
                    ],
                    2,
                    expected_native_contributions(retained_outcome),
                    vec![],
                    true,
                ),
            },
        ],
        expected_final: popup_snapshot(),
    }
}

fn popup_gate_resize_workspace() -> InitialWorkspace {
    let mut workspace = native_workspace();
    let first = vec![
        ItemKey(1),
        ItemKey(4),
        ItemKey(5),
        ItemKey(6),
        ItemKey(7),
        ItemKey(8),
    ];
    let second = vec![
        ItemKey(3),
        ItemKey(9),
        ItemKey(10),
        ItemKey(11),
        ItemKey(12),
        ItemKey(13),
    ];
    workspace.roots[0].node = NodeFixture::Split {
        axis: dockspace_core_protocol::AxisSpec::Horizontal,
        weights: vec![0.5, 0.5],
        children: vec![
            NodeFixture::Tabs {
                items: first.clone(),
                selected: Some(ItemKey(1)),
                mru: first,
            },
            NodeFixture::Tabs {
                items: second.clone(),
                selected: Some(ItemKey(3)),
                mru: second,
            },
        ],
    };
    let second_surface = vec![
        ItemKey(2),
        ItemKey(14),
        ItemKey(15),
        ItemKey(16),
        ItemKey(17),
        ItemKey(18),
        ItemKey(19),
        ItemKey(20),
        ItemKey(21),
        ItemKey(22),
        ItemKey(23),
        ItemKey(24),
    ];
    workspace.roots[1].node = NodeFixture::Tabs {
        items: second_surface.clone(),
        selected: Some(ItemKey(2)),
        mru: second_surface,
    };
    workspace
}

fn popup_gate_resize_snapshot() -> ExpectedCanonicalSnapshot {
    let mut snapshot = popup_snapshot();
    let mut roots = popup_gate_resize_workspace().roots;
    snapshot.workspace.roots[0].node = roots.remove(0).node;
    snapshot.workspace.roots[1].node = roots.remove(0).node;
    snapshot.workspace.item_multiset = (1..=24)
        .map(|item| ItemCount {
            item: ItemKey(item),
            count: 1,
        })
        .collect();
    snapshot.workspace.item_owners =
        [1, 4, 5, 6, 7, 8]
            .into_iter()
            .map(|item| dockspace_core_protocol::ItemOwner {
                item: ItemKey(item),
                root: RootKey(1),
                path: dockspace_core_protocol::StructuralPath(vec![0]),
                owner: RootOwner::Main {
                    surface: SurfaceKey(1),
                },
            })
            .chain([3, 9, 10, 11, 12, 13].into_iter().map(|item| {
                dockspace_core_protocol::ItemOwner {
                    item: ItemKey(item),
                    root: RootKey(1),
                    path: dockspace_core_protocol::StructuralPath(vec![1]),
                    owner: RootOwner::Main {
                        surface: SurfaceKey(1),
                    },
                }
            }))
            .chain(
                [2, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24]
                    .into_iter()
                    .map(|item| dockspace_core_protocol::ItemOwner {
                        item: ItemKey(item),
                        root: RootKey(2),
                        path: dockspace_core_protocol::StructuralPath(vec![]),
                        owner: RootOwner::Main {
                            surface: SurfaceKey(2),
                        },
                    }),
            )
            .collect();
    snapshot
}

fn popup_gate_revocation_during_resize_trace() -> CoreProtocolTrace {
    let first = headless_stream(1);
    let second = headless_stream(2);
    let retained = || native_contributions(SurfaceMeasurementIngress::Retained {});
    let retained_outcomes = || expected_native_contributions(retained_outcome);
    CoreProtocolTrace {
        id: CoreProtocolTraceId("popup-gate-revokes-active-resize".into()),
        provenance: CoreProtocolTraceProvenance {
            baseline: BaselineId("open-gpui".into()),
            path: "crates/gpui_docking/src/host_viewport_route_tests.rs".into(),
            test: "popup_gate_revocation_cancels_active_resize_before_release".into(),
            retained_behavior:
                "Losing one sibling surface's popup-plane authority cancels an active resize; later fresh authority cannot revive the old gesture."
                    .into(),
            deliberate_strengthening:
                "The splitter is named structurally, presentation facts name exact streams and emissions, and the post-recovery release remains causally inert."
                    .into(),
        },
        initial_workspace: popup_gate_resize_workspace(),
        boundaries: vec![
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: popup_gate_expected(
                    1,
                    vec![],
                    0,
                    expected_native_contributions(ready),
                    vec![],
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: retained(),
                expected: popup_gate_expected(
                    2,
                    vec![],
                    2,
                    retained_outcomes(),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("settle-inactive-roster".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(first, 1, 1),
                        presented_observation(second, 1, 1),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: retained(),
                expected: ExpectedTransition {
                    surface_scene_deltas: [1, 2]
                        .into_iter()
                        .map(|surface| {
                            presentation_scene_delta(
                                surface,
                                scene_state(ExpectedSurfaceSceneStateKind::Ready),
                                presented_scene_state(
                                    ExpectedSurfaceSceneStateKind::Ready,
                                    headless_stream(surface),
                                    1,
                                    0,
                                ),
                            )
                        })
                        .collect(),
                    ..popup_gate_expected(
                        3,
                        vec![
                            expected_presented(first, 1, 1, 1),
                            expected_presented(second, 1, 1, 1),
                        ],
                        2,
                        retained_outcomes(),
                        vec![SurfaceKey(1), SurfaceKey(2)],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("begin-resize".into()),
                provider: Some(PointerProviderIngress::Activate {
                    scope: PointerProviderScopeIngress::SurfaceLocal {
                        surface: SurfaceKey(1),
                    },
                    committed_through: 0,
                }),
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 0,
                    through: 1,
                    edges: vec![local_edge(
                        1,
                        PointerEdgeKindSpec::PrimaryPressed,
                        320.0,
                        240.0,
                        Some(PointerDeliveryIngress::Splitter {
                            root: RootKey(1),
                            path: dockspace_core_protocol::StructuralPath(vec![]),
                            index: 0,
                        }),
                        None,
                    )],
                }],
                surface_contributions: retained(),
                expected: ExpectedTransition {
                    reduced_pointer_edges: vec![ExpectedPointerEdge {
                        cause: ExpectedPointerEdgeCause::PointerEdge,
                        provider_incarnation: 1,
                        stream_incarnation: 1,
                        sequence: 1,
                        outcomes: vec![ExpectedInteractionOutcome::ResizeBegan],
                    }],
                    interaction_events: vec![],
                    platform_effects: vec![],
                    focus_delta: Default::default(),
                    surface_scene_deltas: vec![],
                    interaction: ExpectedInteractionState::Resizing,
                    ..popup_gate_expected(
                        4,
                        vec![],
                        2,
                        retained_outcomes(),
                        vec![SurfaceKey(1), SurfaceKey(2)],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("open-popup-revokes-resize".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![
                    popup_open_event_for(SurfaceKey(2), RootKey(2), vec![]),
                    HostFrameEvent::PointerJournal {
                        previous: 1,
                        through: 1,
                        edges: vec![],
                    },
                ],
                surface_contributions: deferred_native_contributions(),
                expected: popup_open_expected(5, vec![]),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 1,
                    edges: vec![],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: ExpectedTransition {
                    surface_scene_deltas: two_surface_scene_deltas(
                        ExpectedSurfaceSceneStateKind::Stale,
                        ExpectedSurfaceSceneStateKind::Ready,
                    ),
                    ..popup_gate_expected(
                        6,
                        vec![],
                        0,
                        expected_native_contributions(ready),
                        vec![],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-active-popup".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 1,
                    edges: vec![],
                }],
                surface_contributions: retained(),
                expected: popup_gate_expected(
                    7,
                    vec![],
                    2,
                    retained_outcomes(),
                    vec![],
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("settle-active-popup-roster".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(first, 2, 4),
                        presented_observation(second, 2, 4),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 1,
                    edges: vec![],
                }],
                surface_contributions: retained(),
                expected: ExpectedTransition {
                    surface_scene_deltas: [1, 2]
                        .into_iter()
                        .map(|surface| {
                            presentation_scene_delta(
                                surface,
                                scene_state(ExpectedSurfaceSceneStateKind::Ready),
                                presented_scene_state(
                                    ExpectedSurfaceSceneStateKind::Ready,
                                    headless_stream(surface),
                                    4,
                                    0,
                                ),
                            )
                        })
                        .collect(),
                    ..popup_gate_expected(
                        8,
                        vec![
                            expected_presented(first, 2, 4, 3),
                            expected_presented(second, 2, 4, 3),
                        ],
                        2,
                        retained_outcomes(),
                        vec![SurfaceKey(1), SurfaceKey(2)],
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("release-cannot-revive-resize".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 1,
                    through: 2,
                    edges: vec![PointerEdgeIngress {
                        sequence: 2,
                        pointer: 7,
                        kind: PointerEdgeKindSpec::PrimaryReleased,
                        location: PointerLocationIngress::SurfaceLocal {
                            position: PointAuthorityIngress::Known {
                                point: PointFixture { x: 368.0, y: 240.0 },
                            },
                        },
                        delivery: PointerEventDeliveryIngress::ProviderEndpoint,
                        capture: PointerCaptureIngress::None,
                        receiver: PointerReceiverIngress::NotApplicable,
                    }],
                }],
                surface_contributions: retained(),
                expected: ExpectedTransition {
                    reduced_pointer_edges: vec![ExpectedPointerEdge {
                        cause: ExpectedPointerEdgeCause::PointerEdge,
                        provider_incarnation: 1,
                        stream_incarnation: 1,
                        sequence: 2,
                        outcomes: vec![],
                    }],
                    ..popup_gate_expected(
                        9,
                        vec![],
                        2,
                        retained_outcomes(),
                        vec![SurfaceKey(1), SurfaceKey(2)],
                        false,
                    )
                },
            },
        ],
        expected_final: popup_gate_resize_snapshot(),
    }
}

fn native_platform_snapshot() -> PlatformSnapshotFixture {
    PlatformSnapshotFixture {
        capability_generation: 1,
        focus_generation: 1,
        inventory_generation: 1,
        capabilities: PlatformCapabilitiesFixture {
            supported: vec![
                PlatformRequirementSpec::NativeWindowLifecycle,
                PlatformRequirementSpec::AuthoritativeInventory,
                PlatformRequirementSpec::HoveredWindow,
                PlatformRequirementSpec::DesktopPointerPosition,
                PlatformRequirementSpec::AuthoritativeButtonState,
                PlatformRequirementSpec::PointerHitTestObservation,
                PlatformRequirementSpec::PointerHitTestControl,
            ],
        },
        windows: vec![
            ObservedWindowFixture {
                surface: SurfaceKey(1),
                coordinate_observation_generation: 1,
                content_bounds: RectFixture {
                    x: 0.0,
                    y: 0.0,
                    width: 640.0,
                    height: 480.0,
                },
                outer_bounds: RectFixture {
                    x: 0.0,
                    y: 0.0,
                    width: 640.0,
                    height: 480.0,
                },
                native_scale_factor: 1.0,
                presentation_scale_factor: 1.0,
                input_observation_generation: 1,
                input_state: WindowInputStateSpec::ReceivesInput,
                presentation_generation: 1,
                presentation_state: WindowPresentationStateSpec::Visible,
                acknowledged_presentation_effect: None,
            },
            ObservedWindowFixture {
                surface: SurfaceKey(2),
                coordinate_observation_generation: 1,
                content_bounds: RectFixture {
                    x: 2_000.0,
                    y: 0.0,
                    width: 1_280.0,
                    height: 960.0,
                },
                outer_bounds: RectFixture {
                    x: 2_000.0,
                    y: 0.0,
                    width: 1_280.0,
                    height: 960.0,
                },
                native_scale_factor: 2.0,
                presentation_scale_factor: 2.0,
                input_observation_generation: 1,
                input_state: WindowInputStateSpec::ReceivesInput,
                presentation_generation: 1,
                presentation_state: WindowPresentationStateSpec::Visible,
                acknowledged_presentation_effect: None,
            },
        ],
        work_area_observation: WorkAreaRosterObservationFixture::Unknown {
            generation: 1,
            reason: AuthorityUnavailableReasonSpec::NotReported,
        },
    }
}

fn dock_route(
    surface: u64,
    desktop_x: f64,
    desktop_y: f64,
    _local_x: f64,
    _local_y: f64,
) -> PointerLocationIngress {
    PointerLocationIngress::Desktop {
        route: DesktopRouteIngress::DockFromDesktop {
            surface: SurfaceKey(surface),
            desktop_position: PointFixture {
                x: desktop_x,
                y: desktop_y,
            },
        },
    }
}

fn native_pointer_setup_suite() -> CoreProtocolTraceSuite {
    let mut suite = minimal_suite();
    suite.traces = vec![CoreProtocolTrace {
        id: CoreProtocolTraceId("desktop-global-cross-dpi".into()),
        provenance: CoreProtocolTraceProvenance {
            baseline: BaselineId("open-gpui".into()),
            path: "crates/gpui_docking/src/host_viewport_route_tests.rs".into(),
            test: "desktop_global_journal_uses_target_native_route_for_cross_surface_drop".into(),
            retained_behavior: "A pointer pressed in a 1x source viewport routes into an independent 2x target viewport without converting inside the core.".into(),
            deliberate_strengthening: "Bindings, coordinates, candidates, and receipts are all core-minted or derived from exact event-time facts.".into(),
        },
        initial_workspace: native_workspace(),
        boundaries: vec![
            CoreProtocolTraceBoundary {
                id: BoundaryId("register-native-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![
                    platform_event(
                        1,
                        CoreProtocolTraceInput::Lifecycle {
                            action: LifecycleIngress::RegisterViewport {
                                surface: SurfaceKey(1),
                                token: 10,
                                role: ViewportRoleSpec::Root,
                            },
                        },
                    ),
                    platform_event(
                        2,
                        CoreProtocolTraceInput::Lifecycle {
                            action: LifecycleIngress::RegisterViewport {
                                surface: SurfaceKey(2),
                                token: 20,
                                role: ViewportRoleSpec::Root,
                            },
                        },
                    ),
                ],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Unavailable {
                        reason: MeasurementUnavailableReasonSpec::Deferred,
                    },
                ),
                expected: native_expected(
                    1,
                    vec![platform_ref(1), platform_ref(2)],
                    vec![],
                    vec![],
                    0,
                    expected_native_contributions(unavailable),
                    ExpectedInteractionState::Idle,
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("publish-native-coordinates".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![platform_event(
                    3,
                    CoreProtocolTraceInput::PlatformObservation {
                        observation: PlatformObservationIngress::PublishSnapshot {
                            snapshot: native_platform_snapshot(),
                        },
                    },
                )],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Unavailable {
                        reason: MeasurementUnavailableReasonSpec::Deferred,
                    },
                ),
                expected: native_expected(
                    2,
                    vec![platform_ref(3)],
                    vec![],
                    vec![],
                    0,
                    expected_native_contributions(unavailable),
                    ExpectedInteractionState::Idle,
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-native-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: unavailable_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: native_expected(
                    3,
                    vec![],
                    vec![],
                    vec![],
                    0,
                    expected_native_contributions(ready),
                    ExpectedInteractionState::Idle,
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-native-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: native_expected(
                    4,
                    vec![],
                    vec![],
                    vec![],
                    2,
                    expected_native_contributions(retained_outcome),
                    ExpectedInteractionState::Idle,
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("settle-native-surfaces".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(native_root_stream(1), 1, 1),
                        presented_observation(native_root_stream(2), 1, 1),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: native_expected(
                    5,
                    vec![],
                    vec![],
                    vec![
                        expected_presented(native_root_stream(1), 1, 1, 1),
                        expected_presented(native_root_stream(2), 1, 1, 1),
                    ],
                    2,
                    expected_native_contributions(retained_outcome),
                    ExpectedInteractionState::Idle,
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("press-1x-move-2x".into()),
                provider: Some(PointerProviderIngress::Activate {
                    scope: PointerProviderScopeIngress::DesktopGlobal {},
                    committed_through: 0,
                }),
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 0,
                    through: 2,
                    edges: vec![
                        PointerEdgeIngress {
                            sequence: 1,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::PrimaryPressed,
                            location: dock_route(1, 96.0, 24.0, 96.0, 24.0),
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::Presented {
                                delivery: Some(PointerDeliveryIngress::Tab { item: ItemKey(1) }),
                                hover: None,
                            },
                        },
                        PointerEdgeIngress {
                            sequence: 2,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: dock_route(2, 2_640.0, 508.0, 320.0, 254.0),
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::Presented {
                                delivery: None,
                                hover: Some(center_hover(2, 2)),
                            },
                        },
                    ],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: ExpectedTransition {
                    interaction_events: vec![pointer_interaction_event(
                        2,
                        0,
                        ExpectedInteractionEventKind::PreviewPublished {
                            visual: ExpectedPreviewVisual::Dock {
                                surface: SurfaceKey(2),
                                target: PointerDropTargetIngress::Center {
                                    root: RootKey(2),
                                    path: dockspace_core_protocol::StructuralPath(vec![]),
                                },
                                rect: RectFixture {
                                    x: 0.0,
                                    y: 28.0,
                                    width: 640.0,
                                    height: 452.0,
                                },
                            },
                        },
                    )],
                    platform_effects: vec![ExpectedPlatformEffectEmission {
                        id: EffectKey(1),
                        epoch: 0,
                        inventory_generation: 1,
                        effect: ExpectedPlatformEffect::SetPointerPassthrough {
                            surface: SurfaceKey(1),
                            enabled: true,
                            after: None,
                        },
                    }],
                    focus_delta: ExpectedFocusDelta {
                        observe_only_activation: Some(ExpectedFocusValueChange {
                            before: None,
                            after: Some(ExpectedRecordedObserveOnlyActivation {
                                request: ExpectedViewportActivationRequest {
                                    target: SurfaceKey(1),
                                    pane: ExpectedPaneFocusDisposition::Set { item: ItemKey(1) },
                                    cause: ExpectedViewportActivationCause::PointerTabGesture,
                                },
                                observation_baseline: None,
                            }),
                        }),
                        ..Default::default()
                    },
                    ..native_expected(
                        6,
                        vec![],
                        vec![
                            ExpectedPointerEdge {
                                cause: ExpectedPointerEdgeCause::PointerEdge,
                                provider_incarnation: 1,
                                stream_incarnation: 1,
                                sequence: 1,
                                outcomes: vec![ExpectedInteractionOutcome::DragArmed],
                            },
                            ExpectedPointerEdge {
                                cause: ExpectedPointerEdgeCause::PointerEdge,
                                provider_incarnation: 1,
                                stream_incarnation: 1,
                                sequence: 2,
                                outcomes: vec![
                                    ExpectedInteractionOutcome::DragBegan,
                                    ExpectedInteractionOutcome::Preview {
                                        status: ExpectedPreviewResolutionStatus::Resolved,
                                    },
                                ],
                            },
                        ],
                        vec![],
                        2,
                        expected_native_contributions(retained_outcome),
                        ExpectedInteractionState::Dragging,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("classify-stale-foreign-none-and-unknown".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 2,
                    through: 6,
                    edges: vec![
                        PointerEdgeIngress {
                            sequence: 3,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::Dock {
                                    surface: SurfaceKey(2),
                                    desktop_position: PointFixture {
                                        x: 2_640.0,
                                        y: 508.0,
                                    },
                                    surface_position: PointFixture { x: 320.0, y: 254.0 },
                                    coordinate_generation: CoordinateGenerationIngress::Stale,
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            // A stale coordinate generation cannot authorize a local point.
                            // The core still classifies the route as unavailable without asking
                            // the adapter to manufacture a hover answer.
                            receiver: PointerReceiverIngress::NotApplicable,
                        },
                        PointerEdgeIngress {
                            sequence: 4,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::Foreign {
                                    desktop_position: PointAuthorityIngress::Known {
                                        point: PointFixture {
                                            x: 3_500.0,
                                            y: 100.0,
                                        },
                                    },
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::NotApplicable,
                        },
                        PointerEdgeIngress {
                            sequence: 5,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::OutsideAll {
                                    desktop_position: PointFixture {
                                        x: 3_500.0,
                                        y: 100.0,
                                    },
                                    work_area: None,
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::NotApplicable,
                        },
                        PointerEdgeIngress {
                            sequence: 6,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::Unknown {
                                    reason: AuthorityUnavailableReasonSpec::NotReported,
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::NotApplicable,
                        },
                    ],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: ExpectedTransition {
                    interaction_events: vec![pointer_interaction_event(
                        3,
                        0,
                        ExpectedInteractionEventKind::PreviewCleared,
                    )],
                    ..native_expected(
                        7,
                        vec![],
                        vec![
                        ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 3,
                            outcomes: vec![ExpectedInteractionOutcome::Preview {
                                status: ExpectedPreviewResolutionStatus::UnknownAuthority,
                            }],
                        },
                        ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 4,
                            outcomes: vec![ExpectedInteractionOutcome::Preview {
                                status: ExpectedPreviewResolutionStatus::OpaqueBlocker,
                            }],
                        },
                        ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 5,
                            outcomes: vec![ExpectedInteractionOutcome::Preview {
                                status:
                                    ExpectedPreviewResolutionStatus::NativePlacementUnavailable,
                            }],
                        },
                        ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 6,
                            outcomes: vec![ExpectedInteractionOutcome::Preview {
                                status: ExpectedPreviewResolutionStatus::UnknownAuthority,
                            }],
                        },
                        ],
                        vec![],
                        2,
                        expected_native_contributions(retained_outcome),
                        ExpectedInteractionState::Dragging,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("retarget-current-2x".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 6,
                    through: 7,
                    edges: vec![PointerEdgeIngress {
                        sequence: 7,
                        pointer: 7,
                        kind: PointerEdgeKindSpec::Moved,
                        location: dock_route(2, 2_640.0, 508.0, 320.0, 254.0),
                        delivery: PointerEventDeliveryIngress::Native {
                            surface: SurfaceKey(1),
                        },
                        capture: PointerCaptureIngress::Native {
                            surface: SurfaceKey(1),
                        },
                        receiver: PointerReceiverIngress::Presented {
                            delivery: None,
                            hover: Some(center_hover(2, 2)),
                        },
                    }],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: ExpectedTransition {
                    interaction_events: vec![pointer_interaction_event(
                        7,
                        0,
                        ExpectedInteractionEventKind::PreviewPublished {
                            visual: ExpectedPreviewVisual::Dock {
                                surface: SurfaceKey(2),
                                target: PointerDropTargetIngress::Center {
                                    root: RootKey(2),
                                    path: dockspace_core_protocol::StructuralPath(vec![]),
                                },
                                rect: RectFixture {
                                    x: 0.0,
                                    y: 28.0,
                                    width: 640.0,
                                    height: 452.0,
                                },
                            },
                        },
                    )],
                    ..native_expected(
                        8,
                        vec![],
                        vec![ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 7,
                            outcomes: vec![ExpectedInteractionOutcome::Preview {
                                status: ExpectedPreviewResolutionStatus::Resolved,
                            }],
                        }],
                        vec![],
                        2,
                        expected_native_contributions(retained_outcome),
                        ExpectedInteractionState::Dragging,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-preview-without-release".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(native_root_stream(1), 2, 5),
                        presented_observation(native_root_stream(2), 2, 5),
                    ],
                },
                presentation_dispositions: painted_native_surfaces(),
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 7,
                    through: 7,
                    edges: vec![],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Retained {},
                ),
                expected: native_expected(
                    9,
                    vec![],
                    vec![],
                    vec![
                        expected_presented(native_root_stream(1), 2, 5, 4),
                        expected_presented(native_root_stream(2), 2, 5, 4),
                    ],
                    2,
                    expected_native_contributions(retained_outcome),
                    ExpectedInteractionState::Dragging,
                    true,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("release-into-2x-target".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![
                        presented_observation(native_root_stream(1), 3, 6),
                        presented_observation(native_root_stream(2), 3, 6),
                    ],
                },
                presentation_dispositions: vec![painted_surface(1), painted_surface(2)],
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 7,
                    through: 8,
                    edges: vec![PointerEdgeIngress {
                        sequence: 8,
                        pointer: 7,
                        kind: PointerEdgeKindSpec::PrimaryReleased,
                        location: dock_route(2, 2_640.0, 508.0, 320.0, 254.0),
                        delivery: PointerEventDeliveryIngress::Native {
                            surface: SurfaceKey(1),
                        },
                        capture: PointerCaptureIngress::None,
                        receiver: PointerReceiverIngress::Presented {
                            delivery: None,
                            hover: Some(center_hover(2, 2)),
                        },
                    }],
                }],
                surface_contributions: native_contributions(
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                ),
                expected: ExpectedTransition {
                    after: VersionExpectation {
                        epoch: 0,
                        revision: 1,
                    },
                    interaction_events: vec![pointer_interaction_event(
                        8,
                        1,
                        ExpectedInteractionEventKind::Delivered {
                            delivery: ExpectedWorkspaceDeliveryKind::Dock,
                        },
                    )],
                    platform_effects: vec![ExpectedPlatformEffectEmission {
                        id: EffectKey(2),
                        epoch: 0,
                        inventory_generation: 1,
                        effect: ExpectedPlatformEffect::SetPointerPassthrough {
                            surface: SurfaceKey(1),
                            enabled: false,
                            after: Some(EffectKey(1)),
                        },
                    }],
                    focus_delta: ExpectedFocusDelta {
                        observe_only_activation: Some(ExpectedFocusValueChange {
                            before: Some(ExpectedRecordedObserveOnlyActivation {
                                request: ExpectedViewportActivationRequest {
                                    target: SurfaceKey(1),
                                    pane: ExpectedPaneFocusDisposition::Set { item: ItemKey(1) },
                                    cause: ExpectedViewportActivationCause::PointerTabGesture,
                                },
                                observation_baseline: None,
                            }),
                            after: None,
                        }),
                        ..Default::default()
                    },
                    surface_scene_deltas: [1, 2]
                        .into_iter()
                        .map(|surface| {
                            presentation_scene_delta(
                                surface,
                                presented_scene_state(
                                    ExpectedSurfaceSceneStateKind::Ready,
                                    native_root_stream(surface),
                                    5,
                                    surface + 2,
                                ),
                                scene_state(ExpectedSurfaceSceneStateKind::Ready),
                            )
                        })
                        .collect(),
                    interactive_surface_roster: vec![],
                    ..native_expected(
                        10,
                        vec![],
                        vec![ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 8,
                            outcomes: vec![
                                ExpectedInteractionOutcome::Preview {
                                    status: ExpectedPreviewResolutionStatus::Resolved,
                                },
                                ExpectedInteractionOutcome::DragDelivered,
                            ],
                        }],
                        vec![
                            expected_presented(native_root_stream(1), 3, 6, 1),
                            expected_presented(native_root_stream(2), 3, 6, 1),
                        ],
                        2,
                        expected_native_contributions(ready),
                        ExpectedInteractionState::Idle,
                        true,
                    )
                },
            },
        ],
        expected_final: native_snapshot(),
    }];
    let final_workspace = &mut suite.traces[0].expected_final.workspace;
    final_workspace.version.revision = 1;
    final_workspace.roots[0].node = NodeFixture::Tabs {
        items: vec![ItemKey(3)],
        selected: Some(ItemKey(3)),
        mru: vec![ItemKey(3)],
    };
    final_workspace.roots[1].node = NodeFixture::Tabs {
        items: vec![ItemKey(2), ItemKey(1)],
        selected: Some(ItemKey(1)),
        mru: vec![ItemKey(1), ItemKey(2)],
    };
    final_workspace.item_owners = vec![
        dockspace_core_protocol::ItemOwner {
            item: ItemKey(3),
            root: RootKey(1),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(1),
            },
        },
        dockspace_core_protocol::ItemOwner {
            item: ItemKey(2),
            root: RootKey(2),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(2),
            },
        },
        dockspace_core_protocol::ItemOwner {
            item: ItemKey(1),
            root: RootKey(2),
            path: dockspace_core_protocol::StructuralPath(vec![]),
            owner: RootOwner::Main {
                surface: SurfaceKey(2),
            },
        },
    ];
    suite
}

const NATIVE_CREATE_PLACEMENT: RectFixture = RectFixture {
    x: 904.0,
    y: 476.0,
    width: 640.0,
    height: 480.0,
};

const fn native_child_stream(surface: u64) -> PresentationStreamRef {
    PresentationStreamRef {
        surface: SurfaceKey(surface),
        endpoint: PresentationEndpointRef::Native {
            role: ViewportRoleSpec::Child,
        },
        sequence: 1,
    }
}

const fn painted_native_staging(surface: u64) -> PresentationDispositionIngress {
    PresentationDispositionIngress::NativeStaging {
        surface: SurfaceKey(surface),
        disposition: PresentationDispositionSpec::Painted {},
    }
}

fn native_create_workspace() -> InitialWorkspace {
    InitialWorkspace {
        policy: InitialPolicyFixture {
            allow_native_surfaces: true,
        },
        roots: vec![InitialRoot {
            id: RootKey(1),
            central_path: None,
            node: NodeFixture::Tabs {
                items: vec![ItemKey(1), ItemKey(2)],
                selected: Some(ItemKey(1)),
                mru: vec![ItemKey(1), ItemKey(2)],
            },
        }],
        surfaces: vec![SurfaceFixture {
            id: SurfaceKey(1),
            main_root: Some(RootKey(1)),
            contained: vec![],
        }],
    }
}

fn native_create_snapshot() -> ExpectedCanonicalSnapshot {
    ExpectedCanonicalSnapshot {
        workspace: CanonicalWorkspace {
            version: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            roots: vec![CanonicalRoot {
                id: RootKey(1),
                central_path: None,
                owner: RootOwner::Main {
                    surface: SurfaceKey(1),
                },
                node: NodeFixture::Tabs {
                    items: vec![ItemKey(1), ItemKey(2)],
                    selected: Some(ItemKey(1)),
                    mru: vec![ItemKey(1), ItemKey(2)],
                },
            }],
            surfaces: vec![CanonicalSurface {
                id: SurfaceKey(1),
                main_root: Some(RootKey(1)),
                contained: vec![],
            }],
            item_multiset: vec![
                ItemCount {
                    item: ItemKey(1),
                    count: 1,
                },
                ItemCount {
                    item: ItemKey(2),
                    count: 1,
                },
            ],
            item_owners: [1, 2]
                .into_iter()
                .map(|item| dockspace_core_protocol::ItemOwner {
                    item: ItemKey(item),
                    root: RootKey(1),
                    path: dockspace_core_protocol::StructuralPath(vec![]),
                    owner: RootOwner::Main {
                        surface: SurfaceKey(1),
                    },
                })
                .collect(),
        },
    }
}

fn native_create_platform_snapshot(include_staging: bool) -> PlatformSnapshotFixture {
    let generation = if include_staging { 2 } else { 1 };
    let mut windows = vec![ObservedWindowFixture {
        surface: SurfaceKey(1),
        coordinate_observation_generation: generation,
        content_bounds: BOUNDS,
        outer_bounds: BOUNDS,
        native_scale_factor: 1.0,
        presentation_scale_factor: 1.0,
        input_observation_generation: generation,
        input_state: WindowInputStateSpec::ReceivesInput,
        presentation_generation: generation,
        presentation_state: WindowPresentationStateSpec::Visible,
        acknowledged_presentation_effect: None,
    }];
    if include_staging {
        windows.push(ObservedWindowFixture {
            surface: SurfaceKey(2),
            coordinate_observation_generation: 1,
            content_bounds: NATIVE_CREATE_PLACEMENT,
            outer_bounds: NATIVE_CREATE_PLACEMENT,
            native_scale_factor: 1.0,
            presentation_scale_factor: 1.0,
            input_observation_generation: 1,
            input_state: WindowInputStateSpec::ReceivesInput,
            presentation_generation: 2,
            presentation_state: WindowPresentationStateSpec::Hidden,
            acknowledged_presentation_effect: Some(EffectKey(3)),
        });
    }
    PlatformSnapshotFixture {
        capability_generation: generation,
        focus_generation: generation,
        inventory_generation: generation,
        capabilities: PlatformCapabilitiesFixture {
            supported: vec![
                PlatformRequirementSpec::NativeWindowLifecycle,
                PlatformRequirementSpec::AuthoritativeInventory,
                PlatformRequirementSpec::HoveredWindow,
                PlatformRequirementSpec::DesktopPointerPosition,
                PlatformRequirementSpec::AuthoritativeButtonState,
                PlatformRequirementSpec::GlobalWindowPlacement,
                PlatformRequirementSpec::WorkArea,
                PlatformRequirementSpec::PointerHitTestObservation,
                PlatformRequirementSpec::PointerHitTestControl,
            ],
        },
        windows,
        work_area_observation: WorkAreaRosterObservationFixture::Known {
            generation,
            work_areas: vec![ObservedWorkAreaFixture {
                token: dockspace_core_protocol::WorkAreaKey(1),
                bounds: RectFixture {
                    x: 0.0,
                    y: 0.0,
                    width: 1_920.0,
                    height: 1_080.0,
                },
                scale_factor: 1.0,
            }],
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn native_create_expected(
    tick: u64,
    reduced: Vec<IngressRef>,
    pointer_edges: Vec<ExpectedPointerEdge>,
    observations: Vec<ExpectedPresentationObservationOutcome>,
    emissions: usize,
    contribution: ExpectedSurfaceContributionOutcome,
    interaction: ExpectedInteractionState,
    interactive: bool,
    changed: bool,
) -> ExpectedTransition {
    ExpectedTransition {
        tick: ReducerTick(tick),
        before: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        after: VersionExpectation {
            epoch: 0,
            revision: 0,
        },
        reduced,
        reduced_interaction_outcomes: vec![],
        reduced_pointer_edges: pointer_edges,
        presentation_observations: observations,
        presentation_emissions: emissions,
        surface_contributions: vec![contribution],
        interaction_events: vec![],
        platform_effects: vec![],
        focus_delta: Default::default(),
        surface_scene_deltas: vec![],
        interaction,
        interactive_surface_roster: interactive.then_some(SurfaceKey(1)).into_iter().collect(),
        published_state_changed: changed,
    }
}

fn empty_pointer_journal(previous: u64) -> HostFrameEvent {
    HostFrameEvent::PointerJournal {
        previous,
        through: previous,
        edges: vec![],
    }
}

fn native_create_pre_show_suite() -> CoreProtocolTraceSuite {
    let source_stream = native_root_stream(1);
    let staging_stream = native_child_stream(2);
    let mut suite = minimal_suite();
    suite.traces = vec![CoreProtocolTrace {
        id: CoreProtocolTraceId("native-create-pre-show".into()),
        provenance: CoreProtocolTraceProvenance {
            baseline: BaselineId("open-gpui".into()),
            path: "crates/gpui_docking/src/host_viewport_route_tests.rs".into(),
            test: "native_create_waits_for_exact_pre_show_presentation".into(),
            retained_behavior: "An outside-all tab release creates a hidden native child and only shows it after its exact staging output was presented.".into(),
            deliberate_strengthening: "Policy, work area, effect acknowledgement, dynamic binding, and presentation proof all enter through production protocol facts.".into(),
        },
        initial_workspace: native_create_workspace(),
        boundaries: vec![
            CoreProtocolTraceBoundary {
                id: BoundaryId("register-source".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![unavailable_surface(1)],
                events: vec![platform_event(
                    1,
                    CoreProtocolTraceInput::Lifecycle {
                        action: LifecycleIngress::RegisterViewport {
                            surface: SurfaceKey(1),
                            token: 10,
                            role: ViewportRoleSpec::Root,
                        },
                    },
                )],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Unavailable {
                        reason: MeasurementUnavailableReasonSpec::Deferred,
                    },
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![scene_delta(
                        1,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                    )],
                    ..native_create_expected(
                        1,
                        vec![platform_ref(1)],
                        vec![],
                        vec![],
                        0,
                        unavailable(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        false,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("publish-source-platform".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![unavailable_surface(1)],
                events: vec![platform_event(
                    2,
                    CoreProtocolTraceInput::PlatformObservation {
                        observation: PlatformObservationIngress::PublishSnapshot {
                            snapshot: native_create_platform_snapshot(false),
                        },
                    },
                )],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Unavailable {
                        reason: MeasurementUnavailableReasonSpec::Deferred,
                    },
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![scene_delta(
                        1,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                    )],
                    ..native_create_expected(
                        2,
                        vec![platform_ref(2)],
                        vec![],
                        vec![],
                        0,
                        unavailable(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        false,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("measure-source".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![unavailable_surface(1)],
                events: vec![],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![scene_delta(
                        1,
                        ExpectedSurfaceSceneStateKind::Bootstrap,
                        ExpectedSurfaceSceneStateKind::Ready,
                    )],
                    ..native_create_expected(
                        3,
                        vec![],
                        vec![],
                        vec![],
                        0,
                        ready(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        false,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-source".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![painted_surface(1)],
                events: vec![],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Retained {},
                )],
                expected: native_create_expected(
                    4,
                    vec![],
                    vec![],
                    vec![],
                    1,
                    retained_outcome(SurfaceKey(1)),
                    ExpectedInteractionState::Idle,
                    false,
                    false,
                ),
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("settle-source".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![presented_observation(source_stream, 1, 1)],
                },
                presentation_dispositions: vec![painted_surface(1)],
                events: vec![],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Retained {},
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![presentation_scene_delta(
                        1,
                        scene_state(ExpectedSurfaceSceneStateKind::Ready),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            1,
                            2,
                        ),
                    )],
                    ..native_create_expected(
                        5,
                        vec![],
                        vec![],
                        vec![expected_presented(source_stream, 1, 1, 1)],
                        1,
                        retained_outcome(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("press-and-preview-outside".into()),
                provider: Some(PointerProviderIngress::Activate {
                    scope: PointerProviderScopeIngress::DesktopGlobal {},
                    committed_through: 0,
                }),
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![painted_surface(1)],
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 0,
                    through: 2,
                    edges: vec![
                        PointerEdgeIngress {
                            sequence: 1,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::PrimaryPressed,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::DockFromDesktop {
                                    surface: SurfaceKey(1),
                                    desktop_position: PointFixture { x: 96.0, y: 24.0 },
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::Presented {
                                delivery: Some(PointerDeliveryIngress::Tab { item: ItemKey(1) }),
                                hover: None,
                            },
                        },
                        PointerEdgeIngress {
                            sequence: 2,
                            pointer: 7,
                            kind: PointerEdgeKindSpec::Moved,
                            location: PointerLocationIngress::Desktop {
                                route: DesktopRouteIngress::OutsideAll {
                                    desktop_position: PointFixture {
                                        x: 1_000.0,
                                        y: 500.0,
                                    },
                                    work_area: Some(dockspace_core_protocol::WorkAreaKey(1)),
                                },
                            },
                            delivery: PointerEventDeliveryIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            capture: PointerCaptureIngress::Native {
                                surface: SurfaceKey(1),
                            },
                            receiver: PointerReceiverIngress::NotApplicable,
                        },
                    ],
                }],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Retained {},
                )],
                expected: ExpectedTransition {
                    interaction_events: vec![pointer_interaction_event(
                        2,
                        0,
                        ExpectedInteractionEventKind::PreviewPublished {
                            visual: ExpectedPreviewVisual::Native {
                                host_surface: SurfaceKey(1),
                                target_surface: SurfaceKey(2),
                                placement: NATIVE_CREATE_PLACEMENT,
                            },
                        },
                    )],
                    platform_effects: vec![ExpectedPlatformEffectEmission {
                        id: EffectKey(1),
                        epoch: 0,
                        inventory_generation: 1,
                        effect: ExpectedPlatformEffect::SetPointerPassthrough {
                            surface: SurfaceKey(1),
                            enabled: true,
                            after: None,
                        },
                    }],
                    focus_delta: ExpectedFocusDelta {
                        observe_only_activation: Some(ExpectedFocusValueChange {
                            before: None,
                            after: Some(ExpectedRecordedObserveOnlyActivation {
                                request: ExpectedViewportActivationRequest {
                                    target: SurfaceKey(1),
                                    pane: ExpectedPaneFocusDisposition::Set { item: ItemKey(1) },
                                    cause: ExpectedViewportActivationCause::PointerTabGesture,
                                },
                                observation_baseline: None,
                            }),
                        }),
                        ..Default::default()
                    },
                    ..native_create_expected(
                        6,
                        vec![],
                        vec![
                            ExpectedPointerEdge {
                                cause: ExpectedPointerEdgeCause::PointerEdge,
                                provider_incarnation: 1,
                                stream_incarnation: 1,
                                sequence: 1,
                                outcomes: vec![ExpectedInteractionOutcome::DragArmed],
                            },
                            ExpectedPointerEdge {
                                cause: ExpectedPointerEdgeCause::PointerEdge,
                                provider_incarnation: 1,
                                stream_incarnation: 1,
                                sequence: 2,
                                outcomes: vec![
                                    ExpectedInteractionOutcome::DragBegan,
                                    ExpectedInteractionOutcome::Preview {
                                        status: ExpectedPreviewResolutionStatus::Resolved,
                                    },
                                ],
                            },
                        ],
                        vec![],
                        1,
                        retained_outcome(SurfaceKey(1)),
                        ExpectedInteractionState::Dragging,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("present-native-preview".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![presented_observation(source_stream, 2, 3)],
                },
                presentation_dispositions: vec![painted_surface(1)],
                events: vec![empty_pointer_journal(2)],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Retained {},
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![presentation_scene_delta(
                        1,
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            1,
                            2,
                        ),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            3,
                            2,
                        ),
                    )],
                    ..native_create_expected(
                        7,
                        vec![],
                        vec![],
                        vec![expected_presented(source_stream, 2, 3, 2)],
                        1,
                        retained_outcome(SurfaceKey(1)),
                        ExpectedInteractionState::Dragging,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("release-native-preview".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![painted_surface(1)],
                events: vec![HostFrameEvent::PointerJournal {
                    previous: 2,
                    through: 3,
                    edges: vec![PointerEdgeIngress {
                        sequence: 3,
                        pointer: 7,
                        kind: PointerEdgeKindSpec::PrimaryReleased,
                        location: PointerLocationIngress::Desktop {
                            route: DesktopRouteIngress::OutsideAll {
                                desktop_position: PointFixture {
                                    x: 1_000.0,
                                    y: 500.0,
                                },
                                work_area: Some(dockspace_core_protocol::WorkAreaKey(1)),
                            },
                        },
                        delivery: PointerEventDeliveryIngress::Native {
                            surface: SurfaceKey(1),
                        },
                        capture: PointerCaptureIngress::None,
                        receiver: PointerReceiverIngress::NotApplicable,
                    }],
                }],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Retained {},
                )],
                expected: ExpectedTransition {
                    interaction_events: vec![pointer_interaction_event(
                        3,
                        0,
                        ExpectedInteractionEventKind::NativePresentationRequested {
                            surface: SurfaceKey(2),
                            effect: EffectKey(3),
                        },
                    )],
                    platform_effects: vec![
                        ExpectedPlatformEffectEmission {
                            id: EffectKey(2),
                            epoch: 0,
                            inventory_generation: 1,
                            effect: ExpectedPlatformEffect::SetPointerPassthrough {
                                surface: SurfaceKey(1),
                                enabled: false,
                                after: Some(EffectKey(1)),
                            },
                        },
                        ExpectedPlatformEffectEmission {
                            id: EffectKey(3),
                            epoch: 0,
                            inventory_generation: 1,
                            effect: ExpectedPlatformEffect::CreateWindow {
                                surface: SurfaceKey(2),
                                placement: NATIVE_CREATE_PLACEMENT,
                                role: ViewportRoleSpec::Child,
                            },
                        },
                    ],
                    ..native_create_expected(
                        8,
                        vec![],
                        vec![ExpectedPointerEdge {
                            cause: ExpectedPointerEdgeCause::PointerEdge,
                            provider_incarnation: 1,
                            stream_incarnation: 1,
                            sequence: 3,
                            outcomes: vec![
                                ExpectedInteractionOutcome::Preview {
                                    status: ExpectedPreviewResolutionStatus::Resolved,
                                },
                                ExpectedInteractionOutcome::DragDelivered,
                            ],
                        }],
                        vec![],
                        1,
                        retained_outcome(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("ack-hidden-native-window".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![presented_observation(source_stream, 3, 5)],
                },
                presentation_dispositions: vec![
                    unavailable_surface(1),
                    unavailable_native_staging(2),
                ],
                events: vec![
                    platform_event(
                        3,
                        CoreProtocolTraceInput::PlatformObservation {
                            observation: PlatformObservationIngress::PublishSnapshot {
                                snapshot: native_create_platform_snapshot(true),
                            },
                        },
                    ),
                    empty_pointer_journal(3),
                ],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![presentation_scene_delta(
                        1,
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            3,
                            2,
                        ),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            5,
                            2,
                        ),
                    )],
                    ..native_create_expected(
                        9,
                        vec![platform_ref(3)],
                        vec![],
                        vec![expected_presented(source_stream, 3, 5, 2)],
                        0,
                        ready(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("paint-pre-show-staging".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::NoUpdate {},
                presentation_dispositions: vec![
                    unavailable_surface(1),
                    painted_native_staging(2),
                ],
                events: vec![empty_pointer_journal(3)],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                )],
                expected: ExpectedTransition {
                    surface_scene_deltas: vec![presentation_scene_delta(
                        1,
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            5,
                            2,
                        ),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            5,
                            2,
                        ),
                    )],
                    ..native_create_expected(
                        10,
                        vec![],
                        vec![],
                        vec![],
                        1,
                        ready(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        true,
                        true,
                    )
                },
            },
            CoreProtocolTraceBoundary {
                id: BoundaryId("present-pre-show-staging".into()),
                provider: None,
                presentation_observation: PresentationObservationIngress::Batch {
                    observations: vec![presented_observation(staging_stream, 1, 1)],
                },
                presentation_dispositions: vec![unavailable_surface(1)],
                events: vec![empty_pointer_journal(3)],
                surface_contributions: vec![native_contribution(
                    1,
                    SurfaceMeasurementIngress::Complete { bounds: BOUNDS },
                )],
                expected: ExpectedTransition {
                    platform_effects: vec![ExpectedPlatformEffectEmission {
                        id: EffectKey(4),
                        epoch: 0,
                        inventory_generation: 2,
                        effect: ExpectedPlatformEffect::ShowWindow {
                            surface: SurfaceKey(2),
                            after_hidden_generation: 2,
                            after_pre_show_stream: staging_stream,
                            after_pre_show_emission: 1,
                        },
                    }],
                    surface_scene_deltas: vec![presentation_scene_delta(
                        1,
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            5,
                            2,
                        ),
                        presented_scene_state(
                            ExpectedSurfaceSceneStateKind::Ready,
                            source_stream,
                            5,
                            2,
                        ),
                    )],
                    ..native_create_expected(
                        11,
                        vec![],
                        vec![],
                        vec![expected_presented(staging_stream, 1, 1, 1)],
                        0,
                        ready(SurfaceKey(1)),
                        ExpectedInteractionState::Idle,
                        true,
                        true,
                    )
                },
            },
        ],
        expected_final: native_create_snapshot(),
    }];
    suite
}

fn only_pointer_segment(boundary: &CoreProtocolTraceBoundary) -> &[PointerEdgeIngress] {
    match boundary.events.as_slice() {
        [HostFrameEvent::PointerJournal { edges, .. }] => edges,
        _ => panic!("boundary must contain exactly one pointer journal event"),
    }
}

fn only_pointer_segment_mut(
    boundary: &mut CoreProtocolTraceBoundary,
) -> (&mut u64, &mut u64, &mut Vec<PointerEdgeIngress>) {
    match boundary.events.as_mut_slice() {
        [
            HostFrameEvent::PointerJournal {
                previous,
                through,
                edges,
            },
        ] => (previous, through, edges),
        _ => panic!("boundary must contain exactly one pointer journal event"),
    }
}

fn ordered_host_event_interleaving_suite() -> CoreProtocolTraceSuite {
    let mut local = local_pointer_setup_suite();
    let mut trace = local.traces.remove(0);
    trace.boundaries.truncate(3);
    trace.id = CoreProtocolTraceId("ordered-host-events".into());
    trace.provenance.test = "pointer_segment_then_input_then_pointer_segment".into();
    trace.provenance.retained_behavior =
        "Multiple journal segments and semantic inputs reduce in their sole vector order.".into();
    trace.provenance.deliberate_strengthening =
        "Only the core mints causal ordinals; the trace carries no ordinal field.".into();
    trace.boundaries.push(CoreProtocolTraceBoundary {
        id: BoundaryId("pointer-input-pointer".into()),
        provider: Some(PointerProviderIngress::Activate {
            scope: PointerProviderScopeIngress::SurfaceLocal {
                surface: SurfaceKey(1),
            },
            committed_through: 0,
        }),
        presentation_observation: PresentationObservationIngress::NoUpdate {},
        presentation_dispositions: vec![painted_surface(1)],
        events: vec![
            HostFrameEvent::PointerJournal {
                previous: 0,
                through: 1,
                edges: vec![local_not_applicable_edge(
                    1,
                    PointerEdgeKindSpec::Moved,
                    320.0,
                    254.0,
                )],
            },
            HostFrameEvent::SemanticInput {
                producer: ProducerId("protocol".into()),
                source_sequence: 1,
                input: CoreProtocolTraceInput::ValidateWorkspace,
            },
            HostFrameEvent::PointerJournal {
                previous: 1,
                through: 2,
                edges: vec![local_not_applicable_edge(
                    2,
                    PointerEdgeKindSpec::Moved,
                    321.0,
                    254.0,
                )],
            },
        ],
        surface_contributions: vec![retained()],
        expected: ExpectedTransition {
            tick: ReducerTick(4),
            before: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            after: VersionExpectation {
                epoch: 0,
                revision: 0,
            },
            reduced: vec![IngressRef {
                producer: ProducerId("protocol".into()),
                source_sequence: 1,
            }],
            reduced_interaction_outcomes: vec![],
            reduced_pointer_edges: vec![
                ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 1,
                    outcomes: vec![],
                },
                ExpectedPointerEdge {
                    cause: ExpectedPointerEdgeCause::PointerEdge,
                    provider_incarnation: 1,
                    stream_incarnation: 1,
                    sequence: 2,
                    outcomes: vec![],
                },
            ],
            presentation_observations: vec![],
            presentation_emissions: 1,
            surface_contributions: vec![ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(1),
            }],
            interaction_events: vec![],
            platform_effects: vec![],
            focus_delta: Default::default(),
            surface_scene_deltas: vec![],
            interaction: ExpectedInteractionState::Idle,
            interactive_surface_roster: vec![SurfaceKey(1)],
            published_state_changed: false,
        },
    });
    trace.expected_final = minimal_suite()
        .traces
        .into_iter()
        .next()
        .expect("minimal suite has one trace")
        .expected_final;
    local.traces = vec![trace];
    local
}

fn popup_gate_suite() -> CoreProtocolTraceSuite {
    CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![
            popup_gate_partition_trace(1),
            popup_gate_partition_trace(2),
            popup_gate_unknown_retirement_trace(),
            popup_gate_revocation_during_resize_trace(),
        ],
    }
}

#[test]
fn partitioned_presentation_proves_popup_gate_in_both_callback_orders() {
    let encoded = serde_json::to_string(&popup_gate_suite()).expect("popup gate suite encodes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("popup gate suite decodes");
    for trace in &suite.traces[..2] {
        assert!(
            trace.boundaries[5]
                .expected
                .interactive_surface_roster
                .is_empty()
        );
        assert_eq!(
            trace.boundaries[6].expected.interactive_surface_roster,
            vec![SurfaceKey(1), SurfaceKey(2)]
        );
    }
    replay_core_protocol_trace_suite(&suite).expect("both callback orders replay");
}

#[test]
fn unknown_and_unavailable_retirement_keep_the_popup_gate_closed() {
    let trace = popup_gate_unknown_retirement_trace();
    assert!(
        trace.boundaries[5]
            .expected
            .interactive_surface_roster
            .is_empty()
    );
    assert!(
        trace.boundaries[6]
            .expected
            .interactive_surface_roster
            .is_empty()
    );
    assert!(
        trace.boundaries[7]
            .expected
            .interactive_surface_roster
            .is_empty()
    );
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![trace],
    };
    replay_core_protocol_trace_suite(&suite)
        .expect("Unknown and unavailable retirement replay fail closed");
}

#[test]
fn popup_gate_outcome_and_interactive_roster_are_exact_replay_assertions() {
    let mut wrong_outcome = popup_gate_suite();
    let ExpectedPresentationObservationOutcome::Presented {
        promotion_eligible, ..
    } = &mut wrong_outcome.traces[0].boundaries[5]
        .expected
        .presentation_observations[0]
    else {
        panic!("A-first trace begins with one Presented outcome");
    };
    *promotion_eligible = false;
    let error = replay_core_protocol_trace_suite(&wrong_outcome)
        .expect_err("wrong typed presentation outcome must fail replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");

    let mut wrong_roster = popup_gate_suite();
    wrong_roster.traces[0].boundaries[6]
        .expected
        .interactive_surface_roster = vec![SurfaceKey(1)];
    let error = replay_core_protocol_trace_suite(&wrong_roster)
        .expect_err("partial interactive roster must fail replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
}

#[test]
fn interaction_event_oracle_rejects_a_forged_publication() {
    let mut suite = local_pointer_setup_suite();
    suite.traces[0].boundaries[4].expected.interaction_events[0].event =
        ExpectedInteractionEventKind::PreviewCleared;

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a forged interaction event must fail exact replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
}

#[test]
fn platform_effect_oracle_rejects_a_forged_request() {
    let mut suite = native_pointer_setup_suite();
    let ExpectedPlatformEffect::SetPointerPassthrough { enabled, .. } =
        &mut suite.traces[0].boundaries[5].expected.platform_effects[0].effect
    else {
        panic!("native drag boundary must emit pointer pass-through");
    };
    *enabled = false;

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a forged platform effect must fail exact replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
}

#[test]
fn focus_delta_oracle_rejects_a_forged_publication() {
    let mut suite = native_pointer_setup_suite();
    let change = suite.traces[0].boundaries[5]
        .expected
        .focus_delta
        .observe_only_activation
        .as_mut()
        .expect("native drag boundary must publish observe-only activation");
    change
        .after
        .as_mut()
        .expect("activation must be published")
        .observation_baseline = Some(99);

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a forged focus delta must fail exact replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
}

#[test]
fn scene_delta_oracle_rejects_a_forged_authority_change() {
    let mut suite = minimal_suite();
    suite.traces[0].boundaries[0].expected.surface_scene_deltas[0]
        .after
        .as_mut()
        .expect("bootstrap boundary must publish a scene")
        .state = ExpectedSurfaceSceneStateKind::Stale;

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a forged scene delta must fail exact replay");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
}

#[test]
fn popup_gate_loss_cancels_resize_and_fresh_authority_cannot_revive_it() {
    let trace = popup_gate_revocation_during_resize_trace();
    assert_eq!(
        trace.boundaries[3].expected.interaction,
        ExpectedInteractionState::Resizing
    );
    assert_eq!(
        trace.boundaries[4].expected.interaction,
        ExpectedInteractionState::Idle
    );
    assert!(
        trace.boundaries[4]
            .expected
            .interactive_surface_roster
            .is_empty()
    );
    assert_eq!(
        trace.boundaries[7].expected.interactive_surface_roster,
        vec![SurfaceKey(1), SurfaceKey(2)]
    );
    assert!(
        trace.boundaries[8].expected.reduced_pointer_edges[0]
            .outcomes
            .is_empty()
    );

    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![trace],
    };
    replay_core_protocol_trace_suite(&suite)
        .expect("gate revocation and inert stale resize release replay");
}

#[test]
fn bootstrap_trace_round_trips_and_replays() {
    let encoded = serde_json::to_string(&minimal_suite()).expect("typed trace encodes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("new protocol trace decodes");
    let report = replay_core_protocol_trace_suite(&suite).expect("bootstrap trace replays");
    assert_eq!(report.traces(), 1);
    assert_eq!(report.boundaries(), 1);
}

#[test]
fn journal_close_release_round_trips_and_replays_an_exact_close_outcome() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_trace(
            "journal-close-release",
            JournalClickTerminal::MatchingRelease,
        )],
    };
    let encoded = serde_json::to_string(&suite).expect("close-click trace encodes");
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("close-click trace decodes");
    let release = decoded.traces[0]
        .boundaries
        .last()
        .expect("close trace has a release boundary");
    assert_eq!(
        release.expected.reduced_pointer_edges[0].outcomes,
        vec![expected_item_close(false)]
    );
    replay_core_protocol_trace_suite(&decoded).expect("close-click trace replays");
}

#[test]
fn journal_close_press_and_release_reduce_in_one_ordered_segment() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_same_segment_trace()],
    };
    let encoded = serde_json::to_string(&suite).expect("same-segment trace encodes");
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("same-segment trace decodes");
    let terminal = decoded.traces[0]
        .boundaries
        .last()
        .expect("same-segment trace has a terminal boundary");
    assert_eq!(
        terminal
            .expected
            .reduced_pointer_edges
            .iter()
            .map(|edge| edge.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        terminal.expected.reduced_pointer_edges[1].outcomes,
        vec![expected_item_close(false)]
    );
    replay_core_protocol_trace_suite(&decoded).expect("same-segment trace replays");
}

#[test]
fn journal_close_release_distinguishes_existing_plan_reuse() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_reuse_trace()],
    };
    let encoded = serde_json::to_string(&suite).expect("close reuse trace encodes");
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("close reuse trace decodes");
    let release = decoded.traces[0]
        .boundaries
        .last()
        .expect("reuse trace has a second release boundary");
    assert_eq!(
        release.expected.reduced_pointer_edges[0].outcomes,
        vec![expected_item_close(true)]
    );
    replay_core_protocol_trace_suite(&decoded).expect("close reuse trace replays");
}

#[test]
fn journal_close_receiver_mismatch_round_trips_and_replays_exact_cancellation() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_trace(
            "journal-close-receiver-mismatch",
            JournalClickTerminal::ReceiverMismatch,
        )],
    };
    let encoded = serde_json::to_string(&suite).expect("mismatch trace encodes");
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("mismatch trace decodes");
    let release = decoded.traces[0]
        .boundaries
        .last()
        .expect("mismatch trace has a release boundary");
    assert_eq!(
        release.expected.reduced_pointer_edges[0].outcomes,
        vec![ExpectedInteractionOutcome::Cancelled {
            reason: ExpectedInteractionCancelReason::ClickReceiverMismatch,
        }]
    );
    replay_core_protocol_trace_suite(&decoded).expect("mismatch trace replays");
}

#[test]
fn journal_close_escape_round_trips_and_replays_cause_bound_semantic_outcome() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_trace(
            "journal-close-escape",
            JournalClickTerminal::Escape,
        )],
    };
    let encoded = serde_json::to_string(&suite).expect("Escape trace encodes");
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("Escape trace decodes");
    let terminal = decoded.traces[0]
        .boundaries
        .last()
        .expect("Escape trace has a terminal boundary");
    assert_eq!(
        terminal.expected.reduced_interaction_outcomes,
        vec![ExpectedReducedInteractionOutcome {
            ingress: IngressRef {
                producer: ProducerId("keyboard".into()),
                source_sequence: 1,
            },
            outcome: ExpectedInteractionOutcome::Cancelled {
                reason: ExpectedInteractionCancelReason::Escape,
            },
        }]
    );
    replay_core_protocol_trace_suite(&decoded).expect("Escape trace replays");
}

#[test]
fn every_core_cancellation_reason_has_a_distinct_stable_trace_value() {
    let reasons = [
        ExpectedInteractionCancelReason::Escape,
        ExpectedInteractionCancelReason::ReleasedBeforeDrag,
        ExpectedInteractionCancelReason::CaptureLost,
        ExpectedInteractionCancelReason::CaptureAuthorityUnavailable,
        ExpectedInteractionCancelReason::PointerStreamCancelled,
        ExpectedInteractionCancelReason::UnknownButtonState,
        ExpectedInteractionCancelReason::UnknownTargetAuthority,
        ExpectedInteractionCancelReason::OpaquePointerBlocker,
        ExpectedInteractionCancelReason::ClickReceiverMismatch,
        ExpectedInteractionCancelReason::NativeCapabilityUnknown,
        ExpectedInteractionCancelReason::NativeCapabilityUnavailable,
        ExpectedInteractionCancelReason::NativePlacementUnavailable,
        ExpectedInteractionCancelReason::SourceVanished,
        ExpectedInteractionCancelReason::WorkspaceChanged,
        ExpectedInteractionCancelReason::PolicyChanged,
        ExpectedInteractionCancelReason::SurfaceClosed,
        ExpectedInteractionCancelReason::SceneUnavailable,
        ExpectedInteractionCancelReason::PointerProviderRetired,
        ExpectedInteractionCancelReason::ReplacedByNewGesture,
        ExpectedInteractionCancelReason::WorkspaceRestored,
    ];
    let encoded = serde_json::to_string(&reasons).expect("cancellation reasons encode");
    let decoded: Vec<ExpectedInteractionCancelReason> =
        serde_json::from_str(&encoded).expect("cancellation reasons decode");
    assert_eq!(decoded, reasons);
    let distinct = decoded
        .iter()
        .map(|reason| serde_json::to_string(reason).expect("one reason encodes"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(distinct.len(), reasons.len());
}

#[test]
fn pending_release_outcomes_have_distinct_stable_trace_values() {
    let outcomes = [
        ExpectedInteractionOutcome::PendingDragRelease,
        ExpectedInteractionOutcome::PendingContainedTransformRelease,
    ];

    let encoded = serde_json::to_value(&outcomes).expect("pending release outcomes encode");
    assert_eq!(
        encoded,
        serde_json::json!([
            { "kind": "pending_drag_release" },
            { "kind": "pending_contained_transform_release" }
        ])
    );

    let decoded: [ExpectedInteractionOutcome; 2] =
        serde_json::from_value(encoded).expect("pending release outcomes decode");
    assert_eq!(decoded, outcomes);
    assert_ne!(decoded[0], decoded[1]);
}

#[test]
fn source_vanished_expectation_is_observable_and_not_folded_into_receiver_mismatch() {
    let mut trace = journal_click_trace(
        "journal-close-distinct-source-vanished",
        JournalClickTerminal::ReceiverMismatch,
    );
    trace.boundaries[4].expected.reduced_pointer_edges[0].outcomes =
        vec![ExpectedInteractionOutcome::Cancelled {
            reason: ExpectedInteractionCancelReason::SourceVanished,
        }];
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![trace],
    };
    let encoded = serde_json::to_string(&suite).expect("SourceVanished expectation encodes");
    let decoded =
        decode_core_protocol_trace_suite(&encoded).expect("SourceVanished expectation decodes");
    let error = replay_core_protocol_trace_suite(&decoded)
        .expect_err("receiver mismatch must not satisfy SourceVanished");
    assert!(error.to_string().contains("transition mismatch"), "{error}");
    assert!(
        !error.to_string().contains("cannot canonically observe"),
        "{error}"
    );
}

#[test]
fn click_protocol_extensions_reject_adapter_forged_identity_fields() {
    let suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_trace(
            "journal-close-forged-fields",
            JournalClickTerminal::Escape,
        )],
    };

    let mut receiver = serde_json::to_value(&suite).expect("typed trace serializes");
    receiver["traces"][0]["boundaries"][3]["events"][0]["edges"][0]["receiver"]["delivery"]["region_id"] =
        serde_json::json!(99);
    assert!(decode_core_protocol_trace_suite(&receiver.to_string()).is_err());

    let mut escape = serde_json::to_value(&suite).expect("typed trace serializes");
    escape["traces"][0]["boundaries"][4]["events"][0]["input"]["session"] = serde_json::json!(99);
    assert!(decode_core_protocol_trace_suite(&escape.to_string()).is_err());

    let mut unbound_outcome = serde_json::to_value(&suite).expect("typed trace serializes");
    unbound_outcome["traces"][0]["boundaries"][4]["expected"]["reduced_interaction_outcomes"][0]
        ["ingress"]["source_sequence"] = serde_json::json!(99);
    let error = decode_core_protocol_trace_suite(&unbound_outcome.to_string())
        .expect_err("semantic interaction outcomes must bind to one reduced ingress");
    assert!(error.to_string().contains("unreduced ingress"), "{error}");

    let close_suite = CoreProtocolTraceSuite {
        schema: dockspace_core_protocol::CORE_PROTOCOL_TRACE_SCHEMA.into(),
        source_baselines: minimal_suite().source_baselines,
        traces: vec![journal_click_trace(
            "journal-close-forged-outcome",
            JournalClickTerminal::MatchingRelease,
        )],
    };
    let mut outcome = serde_json::to_value(close_suite).expect("typed trace serializes");
    outcome["traces"][0]["boundaries"][4]["expected"]["reduced_pointer_edges"][0]["outcomes"][0]
        ["plan"] = serde_json::json!(7);
    assert!(decode_core_protocol_trace_suite(&outcome.to_string()).is_err());
}

#[test]
fn ordered_events_replay_pointer_input_pointer_with_core_minted_ordinals() {
    let typed = ordered_host_event_interleaving_suite();
    let events = &typed.traces[0].boundaries[3].events;
    assert!(matches!(events[0], HostFrameEvent::PointerJournal { .. }));
    assert!(matches!(events[1], HostFrameEvent::SemanticInput { .. }));
    assert!(matches!(events[2], HostFrameEvent::PointerJournal { .. }));

    let encoded = serde_json::to_string(&typed).expect("ordered trace encodes");
    assert!(!encoded.contains("frame_ordinal"));
    let suite = decode_core_protocol_trace_suite(&encoded).expect("ordered trace decodes");
    replay_core_protocol_trace_suite(&suite).expect("ordered multi-segment trace replays");
}

#[test]
fn old_pointer_protocol_fields_are_rejected() {
    let mut value = serde_json::to_value(minimal_suite()).expect("typed trace serializes");
    value["traces"][0]["boundaries"][0]["pointer_journal"] = serde_json::json!(null);
    assert!(decode_core_protocol_trace_suite(&value.to_string()).is_err());

    let mut value = serde_json::to_value(minimal_suite()).expect("typed trace serializes");
    value["traces"][0]["boundaries"][0]["events"] = serde_json::json!([{
        "kind": "semantic_input",
        "frame_ordinal": 17,
        "producer": "renderer",
        "source_sequence": 1,
        "input": { "kind": "validate_workspace" }
    }]);
    assert!(decode_core_protocol_trace_suite(&value.to_string()).is_err());

    let mut value = serde_json::to_value(minimal_suite()).expect("typed trace serializes");
    value["traces"][0]["boundaries"][0]["events"] = serde_json::json!([{
        "kind": "semantic_input",
        "producer": "platform",
        "source_sequence": 1,
        "input": {
            "kind": "platform_observation",
            "observation": {
                "kind": "publish_snapshot",
                "snapshot": {
                    "focus_generation": 1,
                    "capabilities": { "supported": [] },
                    "windows": [],
                    "pointers": [],
                    "work_areas": []
                }
            }
        }
    }]);
    assert!(decode_core_protocol_trace_suite(&value.to_string()).is_err());
}

#[test]
fn presentation_disposition_roster_rejects_a_missing_slot() {
    let mut suite = minimal_suite();
    suite.traces[0].boundaries[0]
        .presentation_dispositions
        .clear();

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("an omitted physical presentation slot must fail replay");
    assert!(
        error
            .to_string()
            .contains("presentation disposition roster is not exact")
            && error.to_string().contains("missing"),
        "{error}"
    );
}

#[test]
fn presentation_disposition_roster_rejects_a_duplicate_slot() {
    let mut suite = minimal_suite();
    suite.traces[0].boundaries[0]
        .presentation_dispositions
        .push(unavailable_surface(1));
    let encoded = serde_json::to_string(&suite).expect("duplicate roster serializes");

    let error = decode_core_protocol_trace_suite(&encoded)
        .expect_err("a duplicate physical presentation slot must fail validation");
    assert!(
        error
            .to_string()
            .contains("repeats `surface` presentation slot"),
        "{error}"
    );
}

#[test]
fn presentation_disposition_roster_rejects_an_extra_slot() {
    let mut suite = minimal_suite();
    suite.traces[0].boundaries[0]
        .presentation_dispositions
        .push(unavailable_native_staging(1));

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("an extra physical presentation slot must fail replay");
    assert!(
        error
            .to_string()
            .contains("presentation disposition roster is not exact")
            && error.to_string().contains("extra"),
        "{error}"
    );
}

#[test]
fn presentation_disposition_roster_rejects_a_slot_role_mismatch() {
    let mut suite = minimal_suite();
    suite.traces[0].boundaries[0].presentation_dispositions = vec![unavailable_native_staging(1)];

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a native-staging answer cannot satisfy a live surface slot");
    assert!(
        error
            .to_string()
            .contains("presentation slot role mismatch"),
        "{error}"
    );
}

#[test]
fn expected_pointer_edge_requires_an_exact_journal_cause() {
    let mut missing =
        serde_json::to_value(local_pointer_setup_suite()).expect("typed trace serializes");
    let missing_edge = missing
        .pointer_mut("/traces/0/boundaries/3/expected/reduced_pointer_edges/0")
        .and_then(serde_json::Value::as_object_mut)
        .expect("press boundary has one expected pointer edge");
    assert!(missing_edge.remove("cause").is_some());
    let error = decode_core_protocol_trace_suite(&missing.to_string())
        .expect_err("an expected pointer edge without a cause must fail decoding");
    assert!(
        error.to_string().contains("missing field `cause`"),
        "{error}"
    );

    let mut forged =
        serde_json::to_value(local_pointer_setup_suite()).expect("typed trace serializes");
    forged["traces"][0]["boundaries"][3]["expected"]["reduced_pointer_edges"][0]["cause"] =
        serde_json::json!({ "kind": "input" });
    let error = decode_core_protocol_trace_suite(&forged.to_string())
        .expect_err("a non-journal cause must fail decoding");
    assert!(
        error.to_string().contains("unknown variant `input`"),
        "{error}"
    );
}

#[test]
fn expected_pointer_edge_incarnations_are_explicit_replay_assertions() {
    for field in ["provider_incarnation", "stream_incarnation"] {
        let mut forged =
            serde_json::to_value(local_pointer_setup_suite()).expect("typed trace serializes");
        forged["traces"][0]["boundaries"][3]["expected"]["reduced_pointer_edges"][0][field] =
            serde_json::json!(99);
        let suite = decode_core_protocol_trace_suite(&forged.to_string())
            .expect("forged incarnation remains a well-shaped expectation");
        let error = replay_core_protocol_trace_suite(&suite)
            .expect_err("a forged core-owned incarnation must fail replay");
        assert!(error.to_string().contains("transition mismatch"), "{error}");
    }
}

#[test]
fn pej_01_05_and_14_replay_ordered_edges_and_explicit_target_receipts() {
    let typed = local_pointer_setup_suite();
    let trace = &typed.traces[0];
    let no_release = trace
        .boundaries
        .iter()
        .find(|boundary| boundary.id.0 == "no-release-edge")
        .expect("PEJ-05 boundary exists");
    assert!(only_pointer_segment(no_release).is_empty());
    assert_eq!(
        no_release.expected.interaction,
        ExpectedInteractionState::Dragging
    );
    assert!(no_release.expected.reduced_pointer_edges.is_empty());

    let release_press = trace
        .boundaries
        .iter()
        .find(|boundary| boundary.id.0 == "release-a-then-press-b")
        .expect("PEJ-01 boundary exists");
    let kinds = only_pointer_segment(release_press)
        .iter()
        .map(|edge| edge.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            PointerEdgeKindSpec::PrimaryReleased,
            PointerEdgeKindSpec::PrimaryPressed,
        ]
    );

    let encoded = serde_json::to_string(&typed).expect("typed trace encodes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("journal trace decodes");
    replay_core_protocol_trace_suite(&suite).expect("journal trace replays");
}

#[test]
fn pej_04_and_06_replay_stale_route_classes_across_1x_and_2x_surfaces() {
    let typed = native_pointer_setup_suite();
    let classification = typed.traces[0]
        .boundaries
        .iter()
        .find(|boundary| boundary.id.0 == "classify-stale-foreign-none-and-unknown")
        .expect("PEJ-04/06 boundary exists");
    let statuses = classification
        .expected
        .reduced_pointer_edges
        .iter()
        .flat_map(|edge| edge.outcomes.iter())
        .filter_map(|outcome| match outcome {
            ExpectedInteractionOutcome::Preview { status } => Some(*status),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![
            ExpectedPreviewResolutionStatus::UnknownAuthority,
            ExpectedPreviewResolutionStatus::OpaqueBlocker,
            ExpectedPreviewResolutionStatus::NativePlacementUnavailable,
            ExpectedPreviewResolutionStatus::UnknownAuthority,
        ]
    );

    let encoded = serde_json::to_string(&typed).expect("native trace encodes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("native trace decodes");
    replay_core_protocol_trace_suite(&suite).expect("native trace replays");
}

#[test]
fn pej_03_rejects_gaps_replay_and_caller_supplied_incarnations() {
    let mut gap = local_pointer_setup_suite();
    let (_, _, edges) = only_pointer_segment_mut(&mut gap.traces[0].boundaries[3]);
    edges[0].sequence = 2;
    let error = decode_core_protocol_trace_suite(
        &serde_json::to_string(&gap).expect("invalid typed trace still serializes"),
    )
    .expect_err("a sequence gap must fail static validation");
    assert!(error.to_string().contains("gap or reordered edge"));

    let mut replayed = local_pointer_setup_suite();
    let boundary = replayed.traces[0]
        .boundaries
        .iter_mut()
        .find(|boundary| boundary.id.0 == "move")
        .expect("move boundary exists");
    let (previous, through, edges) = only_pointer_segment_mut(boundary);
    *previous = 0;
    *through = 1;
    edges[0].sequence = 1;
    let encoded = serde_json::to_string(&replayed).expect("replayed trace serializes");
    let suite = decode_core_protocol_trace_suite(&encoded)
        .expect("cross-boundary replay is a core ledger concern");
    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("the core watermark must reject a replayed journal");
    assert!(error.to_string().contains("cannot submit pointer journal"));

    let mut value =
        serde_json::to_value(local_pointer_setup_suite()).expect("typed trace serializes");
    value["traces"][0]["boundaries"][3]["provider"]["incarnation"] = serde_json::json!(7);
    assert!(decode_core_protocol_trace_suite(&value.to_string()).is_err());
}

#[test]
fn pej_04_and_14_reject_authority_upgrade_and_contradictory_delivery_shape() {
    let mut stale = native_pointer_setup_suite();
    let boundary = stale.traces[0]
        .boundaries
        .iter_mut()
        .find(|boundary| boundary.id.0 == "classify-stale-foreign-none-and-unknown")
        .expect("classification boundary exists");
    let (_, _, edges) = only_pointer_segment_mut(boundary);
    let edge = edges.first_mut().expect("stale edge exists");
    edge.receiver = PointerReceiverIngress::Presented {
        delivery: Some(PointerDeliveryIngress::NoReceiver),
        hover: None,
    };
    let encoded = serde_json::to_string(&stale).expect("forged trace serializes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("forged trace is well-shaped");
    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("stale route cannot be upgraded to presented authority");
    assert!(
        error
            .to_string()
            .contains("cannot submit core-minted receiver receipts"),
        "{error}"
    );

    let mut value =
        serde_json::to_value(local_pointer_setup_suite()).expect("typed trace serializes");
    let receiver = &mut value["traces"][0]["boundaries"][3]["events"][0]["edges"][0]["receiver"];
    receiver["drag_delivery"] = serde_json::json!({ "kind": "canvas" });
    receiver["candidate"] = serde_json::json!(99);
    assert!(decode_core_protocol_trace_suite(&value.to_string()).is_err());
}

#[test]
fn pej_09_replays_unknown_and_authoritative_capture_changes() {
    let typed = capture_change_suite();
    let unknown = typed
        .traces
        .iter()
        .find(|trace| trace.id.0 == "capture-unknown")
        .expect("PEJ-09 Unknown trace exists")
        .boundaries
        .last()
        .expect("PEJ-09 Unknown trace has a terminal boundary");
    assert_eq!(
        unknown.expected.interaction,
        ExpectedInteractionState::Dragging
    );
    assert!(
        unknown
            .expected
            .reduced_pointer_edges
            .iter()
            .flat_map(|edge| &edge.outcomes)
            .all(|outcome| !matches!(outcome, ExpectedInteractionOutcome::Cancelled { .. }))
    );

    for trace_id in ["capture-none", "capture-foreign"] {
        let terminal = typed
            .traces
            .iter()
            .find(|trace| trace.id.0 == trace_id)
            .unwrap_or_else(|| panic!("PEJ-09 {trace_id} trace exists"))
            .boundaries
            .last()
            .expect("PEJ-09 known-loss trace has a terminal boundary");
        assert_eq!(
            terminal.expected.interaction,
            ExpectedInteractionState::Idle
        );
        assert_eq!(
            terminal
                .expected
                .reduced_pointer_edges
                .iter()
                .flat_map(|edge| &edge.outcomes)
                .filter(|outcome| matches!(
                    outcome,
                    ExpectedInteractionOutcome::Cancelled {
                        reason: ExpectedInteractionCancelReason::CaptureLost,
                    }
                ))
                .count(),
            1,
            "{trace_id} must cancel the active drag exactly once"
        );
    }

    let encoded = serde_json::to_string(&typed).expect("capture trace encodes");
    let suite = decode_core_protocol_trace_suite(&encoded).expect("capture trace decodes");
    replay_core_protocol_trace_suite(&suite).expect("PEJ-09 capture traces replay");
}

#[test]
fn explicit_receiver_identity_and_delivery_payload_fail_closed() {
    let mut wrong_surface = native_pointer_setup_suite();
    let boundary = wrong_surface.traces[0]
        .boundaries
        .iter_mut()
        .find(|boundary| boundary.id.0 == "retarget-current-2x")
        .expect("retarget boundary exists");
    let (_, _, edges) = only_pointer_segment_mut(boundary);
    let PointerReceiverIngress::Presented {
        hover: Some(PointerHoverIngress::DockTarget { surface, target: _ }),
        ..
    } = &mut edges[0].receiver
    else {
        panic!("retarget edge must declare one exact dock target");
    };
    *surface = SurfaceKey(1);
    let encoded = serde_json::to_string(&wrong_surface).expect("wrong receiver serializes");
    let suite = decode_core_protocol_trace_suite(&encoded)
        .expect("wrong but known receiver surface remains well-shaped");
    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("a receiver from a different routed surface must fail replay");
    assert!(
        error.to_string().contains("differs from routed surface"),
        "{error}"
    );

    let mut wrong_delivery = local_pointer_setup_suite();
    let (_, _, edges) = only_pointer_segment_mut(&mut wrong_delivery.traces[0].boundaries[3]);
    let PointerReceiverIngress::Presented {
        delivery: Some(PointerDeliveryIngress::Tab { item }),
        ..
    } = &mut edges[0].receiver
    else {
        panic!("press edge must declare one exact tab delivery");
    };
    *item = ItemKey(99);
    let encoded = serde_json::to_string(&wrong_delivery).expect("wrong delivery serializes");
    let suite = decode_core_protocol_trace_suite(&encoded)
        .expect("unknown application item remains a well-shaped semantic payload");
    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("an absent tab receiver must fail replay");
    assert!(
        error.to_string().contains("no visible tab receiver"),
        "{error}"
    );
}

#[test]
fn scroll_trace_round_trips_explicit_receipts_and_preserves_unknown_sequence_ownership() {
    let expected = scroll_trace_suite();
    let encoded = serde_json::to_string_pretty(&expected).expect("scroll trace encodes");
    assert!(encoded.contains("\"tab_strip_scroll\""));
    assert!(encoded.contains("\"event_correlation_unavailable\""));
    assert!(encoded.contains("\"momentum\""));
    assert!(encoded.contains("\"lines\""));
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("scroll trace decodes");
    assert_eq!(decoded, expected);
    replay_core_protocol_trace_suite(&decoded).expect("ordered scroll trace replays");
}

#[test]
fn scroll_receipt_never_falls_back_from_an_invalid_structural_symbol() {
    let mut suite = scroll_trace_suite();
    let HostFrameEvent::PointerJournal { edges, .. } = &mut suite.traces[0].boundaries[3].events[0]
    else {
        panic!("scroll begin boundary must contain one journal")
    };
    let PointerReceiverIngress::Presented {
        delivery:
            Some(PointerDeliveryIngress::TabStripScroll {
                path,
                root: RootKey(1),
            }),
        ..
    } = &mut edges[0].receiver
    else {
        panic!("scroll begin must declare one structural tab-strip receipt")
    };
    *path = dockspace_core_protocol::StructuralPath(vec![0]);

    let error = replay_core_protocol_trace_suite(&suite)
        .expect_err("an invalid structural receiver symbol must not select another strip");
    assert!(
        error.to_string().contains("traverses a tabs leaf")
            || error.to_string().contains("path")
            || error.to_string().contains("no tab-strip scroll receiver"),
        "{error}"
    );
}

#[test]
fn scroll_token_rejection_rolls_back_the_frame_and_completed_tokens_cannot_resurrect() {
    let suite = scroll_trace_suite();
    let trace = &suite.traces[0];
    let mut harness = CoreProtocolHarness::new(&trace.initial_workspace)
        .expect("scroll protocol harness initializes");
    for boundary in &trace.boundaries[..=3] {
        harness
            .replay_boundary(trace, boundary)
            .expect("scroll setup and Begin replay");
    }

    let version_before = harness.engine().version();
    let mut replacement = trace.boundaries[4].clone();
    replacement.id = BoundaryId("scroll-reject-active-token-replacement".into());
    let HostFrameEvent::PointerJournal { edges, .. } = &mut replacement.events[0] else {
        panic!("replacement boundary must contain one journal")
    };
    let PointerEdgeKindSpec::Scrolled { scroll } = &mut edges[0].kind else {
        panic!("replacement boundary must contain one scroll edge")
    };
    scroll.phase = ScrollPhaseSpec::Begin;
    scroll.sequence = Some(42);
    scroll.delta = None;
    let error = harness
        .replay_boundary(trace, &replacement)
        .expect_err("one device lane cannot replace its active sequence");
    assert!(error.to_string().contains("receiver receipts"), "{error}");
    assert_eq!(harness.engine().version(), version_before);

    harness
        .replay_boundary(trace, &trace.boundaries[4])
        .expect("the rejected frame leaves the exact update watermark replayable");
    for boundary in &trace.boundaries[5..] {
        harness
            .replay_boundary(trace, boundary)
            .expect("the original sequence completes after rollback");
    }

    let mut aba = trace.boundaries[6].clone();
    aba.id = BoundaryId("scroll-reject-completed-token-aba".into());
    aba.expected.tick = ReducerTick(8);
    let HostFrameEvent::PointerJournal {
        previous,
        through,
        edges,
    } = &mut aba.events[0]
    else {
        panic!("ABA boundary must contain one journal")
    };
    *previous = 4;
    *through = 5;
    edges[0].sequence = 5;
    let PointerEdgeKindSpec::Scrolled { scroll } = &mut edges[0].kind else {
        panic!("ABA boundary must contain one scroll edge")
    };
    scroll.phase = ScrollPhaseSpec::Begin;
    scroll.sequence = Some(41);
    scroll.delta = None;
    let error = harness
        .replay_boundary(trace, &aba)
        .expect_err("a completed device token must not mint a new session");
    assert!(error.to_string().contains("receiver receipts"), "{error}");
}

#[test]
fn provider_retirement_is_one_complete_transition_with_an_exact_tick() {
    let expected = provider_retirement_suite();
    let encoded = serde_json::to_string_pretty(&expected).expect("provider retirement encodes");
    let decoded =
        decode_core_protocol_trace_suite(&encoded).expect("provider retirement trace decodes");
    assert_eq!(decoded, expected);
    replay_core_protocol_trace_suite(&decoded).expect("provider retirement trace replays");

    let trace = &decoded.traces[0];
    let mut harness = CoreProtocolHarness::new(&trace.initial_workspace)
        .expect("provider retirement harness initializes");
    for boundary in &trace.boundaries {
        harness
            .replay_boundary(trace, boundary)
            .expect("each provider boundary replays");
        assert_eq!(
            harness.engine().last_reducer_tick().get(),
            boundary.expected.tick.0,
            "one trace boundary must publish exactly one reducer tick"
        );
    }
    assert!(harness.engine().pointer_provider().is_none());
}

#[test]
fn provider_retirement_with_an_extra_event_fails_before_retiring_authority() {
    let extra_event = HostFrameEvent::SemanticInput {
        producer: ProducerId("retirement-extra-event".into()),
        source_sequence: 1,
        input: CoreProtocolTraceInput::ValidateWorkspace,
    };
    let mut malformed = provider_retirement_suite();
    malformed.traces[0]
        .boundaries
        .last_mut()
        .expect("retirement boundary exists")
        .events
        .push(extra_event.clone());
    let encoded = serde_json::to_string(&malformed).expect("malformed trace still serializes");
    let error = decode_core_protocol_trace_suite(&encoded)
        .expect_err("retirement and host-frame events are mutually exclusive");
    assert!(
        error
            .to_string()
            .contains("cannot contain host-frame events")
    );

    let suite = provider_retirement_suite();
    let trace = &suite.traces[0];
    let mut harness = CoreProtocolHarness::new(&trace.initial_workspace)
        .expect("provider retirement harness initializes");
    for boundary in &trace.boundaries[..4] {
        harness
            .replay_boundary(trace, boundary)
            .expect("provider setup replays");
    }
    let tick_before = harness.engine().last_reducer_tick();
    let provider_before = harness.engine().pointer_provider();
    let mut invalid_retirement = trace.boundaries[4].clone();
    invalid_retirement.events.push(extra_event);
    let error = harness
        .replay_boundary(trace, &invalid_retirement)
        .expect_err("invalid retirement must fail before publishing");
    assert!(
        error
            .to_string()
            .contains("cannot contain host-frame events")
    );
    assert_eq!(harness.engine().last_reducer_tick(), tick_before);
    assert_eq!(harness.engine().pointer_provider(), provider_before);

    harness
        .replay_boundary(trace, &trace.boundaries[4])
        .expect("the valid retirement remains replayable");
    assert_eq!(harness.engine().last_reducer_tick().get(), 5);
    assert!(harness.engine().pointer_provider().is_none());
}

#[test]
fn native_create_waits_for_the_exact_pre_show_presentation() {
    let expected = native_create_pre_show_suite();
    let encoded = serde_json::to_string_pretty(&expected).expect("native-create trace encodes");
    assert!(encoded.contains("\"allow_native_surfaces\": true"));
    assert!(encoded.contains("\"acknowledged_presentation_effect\": 3"));
    assert!(encoded.contains("\"work_area\": 1"));
    let decoded = decode_core_protocol_trace_suite(&encoded).expect("native-create trace decodes");
    assert_eq!(decoded, expected);
    replay_core_protocol_trace_suite(&decoded).expect(
        "native create reaches ShowWindow only after the exact staging output is presented",
    );
}

#[test]
fn native_create_rejects_mismatched_pre_show_stream_or_emission() {
    let suite = native_create_pre_show_suite();

    for mutate in [
        |effect: &mut ExpectedPlatformEffect| {
            let ExpectedPlatformEffect::ShowWindow {
                after_pre_show_stream,
                ..
            } = effect
            else {
                panic!("last native-create effect must show the window")
            };
            *after_pre_show_stream = native_root_stream(1);
        },
        |effect: &mut ExpectedPlatformEffect| {
            let ExpectedPlatformEffect::ShowWindow {
                after_pre_show_emission,
                ..
            } = effect
            else {
                panic!("last native-create effect must show the window")
            };
            *after_pre_show_emission += 1;
        },
    ] {
        let mut forged = suite.clone();
        let effect = &mut forged.traces[0]
            .boundaries
            .last_mut()
            .expect("native-create trace has a show boundary")
            .expected
            .platform_effects[0]
            .effect;
        mutate(effect);

        let encoded = serde_json::to_string(&forged).expect("forged trace still encodes");
        let decoded = decode_core_protocol_trace_suite(&encoded)
            .expect("a forged but well-shaped pre-show proof still decodes");
        let error = replay_core_protocol_trace_suite(&decoded)
            .expect_err("the harness must derive the pre-show proof from the real output");
        assert!(error.to_string().contains("transition mismatch"), "{error}");
    }
}

#[test]
fn checked_in_pointer_journal_fixtures_match_and_replay() {
    let fixtures = [
        (
            include_str!("fixtures/core_protocol/pointer_journal_core_protocol_trace.json"),
            local_pointer_setup_suite(),
        ),
        (
            include_str!(
                "fixtures/core_protocol/cross_dpi_pointer_journal_core_protocol_trace.json"
            ),
            native_pointer_setup_suite(),
        ),
        (
            include_str!(
                "fixtures/core_protocol/capture_change_pointer_journal_core_protocol_trace.json"
            ),
            capture_change_suite(),
        ),
        (
            include_str!("fixtures/core_protocol/scroll_pointer_journal_core_protocol_trace.json"),
            scroll_trace_suite(),
        ),
        (
            include_str!("fixtures/core_protocol/provider_retirement_core_protocol_trace.json"),
            provider_retirement_suite(),
        ),
    ];
    for (encoded, expected) in fixtures {
        let suite = decode_core_protocol_trace_suite(encoded).expect("fixture decodes");
        assert_eq!(
            suite, expected,
            "checked-in fixture drifted from typed trace"
        );
        replay_core_protocol_trace_suite(&suite).expect("fixture replays");
    }
}

#[test]
#[ignore = "writes checked-in core protocol fixtures"]
fn update_checked_in_pointer_journal_fixtures() {
    let fixtures = [
        (
            "tests/fixtures/core_protocol/pointer_journal_core_protocol_trace.json",
            local_pointer_setup_suite(),
        ),
        (
            "tests/fixtures/core_protocol/cross_dpi_pointer_journal_core_protocol_trace.json",
            native_pointer_setup_suite(),
        ),
        (
            "tests/fixtures/core_protocol/capture_change_pointer_journal_core_protocol_trace.json",
            capture_change_suite(),
        ),
        (
            "tests/fixtures/core_protocol/scroll_pointer_journal_core_protocol_trace.json",
            scroll_trace_suite(),
        ),
        (
            "tests/fixtures/core_protocol/provider_retirement_core_protocol_trace.json",
            provider_retirement_suite(),
        ),
    ];
    for (relative_path, suite) in fixtures {
        let mut encoded = serde_json::to_string_pretty(&suite).expect("typed fixture encodes");
        encoded.push('\n');
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
        std::fs::write(path, encoded).expect("fixture update succeeds");
    }
}
