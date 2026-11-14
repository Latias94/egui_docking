//! Drag-source validation, frozen payloads and contained rehome candidates.

use crate::engine::*;

impl DockEngine {
    pub(in crate::engine) fn prepare_drag_source(
        &self,
        payload: &MovePayload,
    ) -> Result<PreparedDragSource, InteractionRejection> {
        let index = self
            .presentation_authority
            .workspace_index_for(self.workspace.as_arc(), self.version);
        self.validate_payload_with_index(payload, index)
            .map_err(InteractionRejection::CommandRejected)?;
        let complete_root = match index {
            Some(index) => {
                index.capture_complete_root_source(&self.workspace, self.version, payload)
            }
            None => self.complete_root_source(payload),
        }
        .map_err(InteractionRejection::CommandRejected)?;
        let partial_detachable =
            complete_root.is_some() || self.partial_payload_is_detachable(payload);
        let source_surface =
            self.payload_surface(payload)
                .ok_or(InteractionRejection::CommandRejected(
                    CommandError::Invariant {
                        stage: "freeze drag source surface",
                    },
                ))?;
        Ok(PreparedDragSource {
            source_surface,
            complete_root,
            partial_detachable,
        })
    }

    pub(in crate::engine) fn native_drag_source_input(
        &self,
        drag: &crate::interaction::ActiveDrag,
    ) -> crate::platform::NativeDragSourceInput {
        use crate::platform::NativeDragSourceInput;
        if let Some(input) = self.viewport.drag_source_input(drag.owner.pointer()) {
            return input;
        }
        let Some(record) = self.viewport.viewport(drag.source_surface) else {
            return NativeDragSourceInput::Preserve;
        };
        if record.ownership() != crate::platform::registry::ViewportOwnership::RuntimeOwned
            || record.role() != crate::platform::viewport::ViewportRole::Child
        {
            // An external window may contain application UI outside this dockspace.
            return NativeDragSourceInput::Preserve;
        }
        let Some(source) = drag.complete_root.as_ref() else {
            return NativeDragSourceInput::Preserve;
        };
        let sibling_has_content = self
            .viewport
            .registry()
            .bindings_for_window(record.binding().window())
            .filter(|binding| binding.surface() != drag.source_surface)
            .any(|binding| {
                self.workspace
                    .surface(binding.surface())
                    .is_some_and(|surface| {
                        surface
                            .main_root
                            .into_iter()
                            .chain(surface.contained.iter().filter_map(|floating| {
                                self.workspace
                                    .contained_floating(*floating)
                                    .map(|floating| floating.root)
                            }))
                            .any(|root| {
                                self.workspace
                                    .root(root)
                                    .is_some_and(|root| self.workspace.subtree_has_items(root.node))
                            })
                    })
            });
        if sibling_has_content {
            return NativeDragSourceInput::Preserve;
        }
        if self
            .workspace
            .surface(drag.source_surface)
            .is_some_and(|surface| {
                surface.main_root == Some(source.root()) && surface.contained.is_empty()
            })
        {
            NativeDragSourceInput::PassThrough
        } else {
            NativeDragSourceInput::Preserve
        }
    }

    pub(in crate::engine) fn native_create_reserves_root(&self, root: crate::ids::RootId) -> bool {
        self.viewport.native_create_sagas().any(|(_, saga)| {
            let prepared = saga.prepared();
            let source_root = match prepared.payload() {
                MovePayload::Item(source) => source.root(),
                MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
            };
            prepared.proposal().root() == root || source_root == root
        })
    }

    pub(in crate::engine) fn root_has_pending_presentation_transition(
        &self,
        root: crate::ids::RootId,
    ) -> bool {
        self.pending_mutation
            .as_ref()
            .and_then(PendingMutation::rehome)
            .is_some()
            || self.native_create_reserves_root(root)
    }

    pub(in crate::engine) fn first_pending_presentation_source_mismatch(
        &self,
        candidate: &Workspace,
        authority: WorkspacePublicationAuthority,
    ) -> Option<SurfaceId> {
        if let Some(pending) = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::rehome)
            && !matches!(
                authority,
                WorkspacePublicationAuthority::PendingPresentationRehome { root }
                    | WorkspacePublicationAuthority::PresentationRehome { root, .. }
                    if root == pending.root
            )
            && candidate
                .capture_node_source(pending.source.root(), pending.source.node())
                .ok()
                .as_ref()
                != Some(&pending.source)
        {
            return Some(pending.source_surface);
        }
        self.viewport.native_create_sagas().find_map(|(saga_id, saga)| {
            if matches!(authority, WorkspacePublicationAuthority::NativeCommit { saga } if saga == saga_id)
            {
                return None;
            }
            let prepared = saga.prepared();
            // Preserve the full frozen root proof, including selection and ownership.
            let source_preserved = match prepared.payload() {
                MovePayload::Item(source) => candidate
                    .capture_item_source(source.root(), source.tabs(), source.item())
                    .as_ref()
                    == Ok(source),
                MovePayload::Tabs(source) | MovePayload::Subtree(source) => candidate
                    .capture_node_source(source.root(), source.node())
                    .as_ref()
                    == Ok(source),
            };
            (!source_preserved).then_some(prepared.source_surface())
        })
    }

    pub(in crate::engine) fn native_create_reserves_floating(
        &self,
        floating: crate::ids::FloatingPresentationId,
    ) -> bool {
        self.viewport
            .native_create_sagas()
            .any(|(_, saga)| saga.prepared().proposal().converted_main().floating() == floating)
    }

    pub(in crate::engine) fn drag_source_is_current(
        &mut self,
        session: crate::interaction::DragSessionId,
        input: InputSequence,
    ) -> Result<bool, EngineError> {
        let (payload, complete_root, origin) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            if drag.source_validated_at == self.version {
                return Ok(true);
            }
            (
                drag.payload.clone(),
                drag.complete_root.clone(),
                drag.origin.clone(),
            )
        };
        let Ok(prepared) = self.prepare_drag_source(&payload) else {
            return Ok(false);
        };
        let valid =
            prepared.complete_root == complete_root && self.frozen_drag_origin_is_current(origin);
        if valid {
            let drag = self.interaction.active_drag_mut(session).map_err(|_| {
                EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                }
            })?;
            drag.source_validated_at = self.version;
            drag.partial_detachable = prepared.partial_detachable;
        }
        Ok(valid)
    }

    pub(in crate::engine) fn drag_source_presentation_allows_targeting(
        &self,
        drag: &crate::interaction::ActiveDrag,
    ) -> bool {
        if !matches!(drag.origin, FrozenDragOrigin::Workspace) {
            return true;
        }
        let surface = drag.source_surface;
        let captured_coordinates = drag.continuation.as_ref().and_then(|continuation| {
            match (&continuation.session, &continuation.draft.source) {
                (
                    SceneGestureSession::Drag(session),
                    SceneGestureContinuationSource::Drag {
                        source_surface,
                        coordinates,
                        ..
                    },
                ) if *session == drag.session && *source_surface == surface => Some(*coordinates),
                _ => None,
            }
        });
        if captured_coordinates.is_some_and(|coordinates| {
            Self::coordinate_capture_matches_current(
                coordinates,
                self.viewport.viewport(surface),
                self.viewport.surface_coordinate_authority(surface),
            )
        }) {
            return true;
        }
        self.presentation_authority
            .scene
            .interaction_projection(surface)
            .is_some_and(|projection| {
                Self::coordinate_capture_matches_current(
                    projection.output().coordinate_capture(),
                    self.viewport.viewport(surface),
                    self.viewport.surface_coordinate_authority(surface),
                )
            })
    }

    pub(in crate::engine) fn frozen_drag_origin_is_current(
        &self,
        origin: FrozenDragOrigin,
    ) -> bool {
        let FrozenDragOrigin::Contained(origin) = origin else {
            return true;
        };
        self.workspace.presentation_for_root(origin.root)
            == Some(crate::graph::workspace::RootPresentationOwner::Contained {
                surface: origin.surface,
                floating: origin.floating,
            })
            && self
                .workspace
                .contained_floating(origin.floating)
                .is_some_and(|record| {
                    record.root == origin.root && record.rect == origin.source_rect
                })
    }

    pub(in crate::engine) fn valid_core_contained_command(
        &self,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::input::intent::ContainedTearOffProposal,
        command: &WorkspaceCommand,
        authority: ContainedPlacementAuthority,
    ) -> bool {
        let placement_is_current = match authority {
            ContainedPlacementAuthority::PresentedCoordinates => {
                self.validate_contained_placement(proposal.placement())
            }
            ContainedPlacementAuthority::CurrentCandidate => self
                .semantic_candidate(proposal.placement().scene())
                .map_err(|_| ContainedPlacementUnavailable::StaleScene {
                    expected: proposal.placement().scene(),
                    current: self
                        .presentation_authority
                        .scene
                        .surface(proposal.surface())
                        .map(SurfaceScene::stamp),
                })
                .and_then(|candidate| {
                    Self::validate_contained_placement_geometry(proposal.placement(), candidate)
                }),
        };
        if placement_is_current.is_err() {
            return false;
        }
        self.valid_contained_command_structure(drag, proposal, command)
    }

    pub(in crate::engine) fn valid_contained_command_structure(
        &self,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::input::intent::ContainedTearOffProposal,
        command: &WorkspaceCommand,
    ) -> bool {
        match command {
            WorkspaceCommand::UpdateContainedPresentation {
                source,
                floating,
                expected_rect,
                expected_roster,
                rect,
                position,
            } => {
                let FrozenDragOrigin::Contained(origin) = &drag.origin else {
                    return false;
                };
                let source_matches = drag.complete_root.as_ref().is_some_and(|source| {
                    source.root() == origin.root
                        && match &drag.payload {
                            MovePayload::Item(item) => {
                                item.root() == source.root() && item.tabs() == source.node()
                            }
                            MovePayload::Tabs(payload) | MovePayload::Subtree(payload) => {
                                payload == source
                            }
                        }
                });
                let owner_matches = self.workspace.presentation_for_root(origin.root)
                    == Some(crate::graph::workspace::RootPresentationOwner::Contained {
                        surface: origin.surface,
                        floating: origin.floating,
                    });
                let record_matches = self
                    .workspace
                    .contained_floating(origin.floating)
                    .is_some_and(|record| {
                        record.root == origin.root && record.rect == origin.source_rect
                    });
                drag.source_validated_at == self.version
                    && source_matches
                    && owner_matches
                    && record_matches
                    && proposal.surface() == origin.surface
                    && proposal.root() == origin.root
                    && proposal.floating() == origin.floating
                    && drag.complete_root.as_ref() == Some(source)
                    && *floating == origin.floating
                    && *expected_rect == origin.source_rect
                    && expected_roster == &origin.source_roster
                    && *rect == proposal.rect()
                    && *position == proposal.position()
            }
            WorkspaceCommand::CreateContainedRoot {
                surface,
                root,
                floating,
                rect,
                position,
                content: RootContent::Move(payload),
            } => {
                drag.complete_root.is_none()
                    && payload == &drag.payload
                    && *surface == proposal.surface()
                    && *root == proposal.root()
                    && *floating == proposal.floating()
                    && *rect == proposal.rect()
                    && *position == proposal.position()
                    && self.workspace.surface(*surface).is_some()
                    && self.workspace.root(*root).is_none()
                    && self.workspace.contained_floating(*floating).is_none()
                    && drag.partial_detachable
            }
            WorkspaceCommand::RehomeRoot { source, target } => {
                drag.complete_root.as_ref() == Some(source)
                    && self.core_contained_rehome_is_valid(source, *target, proposal)
            }
            WorkspaceCommand::CreateContainedRoot { .. }
            | WorkspaceCommand::UpdateContainedRect { .. }
            | WorkspaceCommand::Select { .. }
            | WorkspaceCommand::Reorder { .. }
            | WorkspaceCommand::Open { .. }
            | WorkspaceCommand::Move { .. }
            | WorkspaceCommand::ResizeSplits { .. }
            | WorkspaceCommand::AttachEmptySurface { .. }
            | WorkspaceCommand::RetireEmptySurface { .. }
            | WorkspaceCommand::CreateSurfaceRoot { .. }
            | WorkspaceCommand::InstallMainRoot { .. }
            | WorkspaceCommand::PromoteContained { .. }
            | WorkspaceCommand::RaiseContained { .. }
            | WorkspaceCommand::RemoveEmptyRoot { .. } => false,
        }
    }

    pub(in crate::engine) fn partial_payload_is_detachable(&self, payload: &MovePayload) -> bool {
        #[cfg(test)]
        PARTIAL_DETACHABILITY_EVALUATIONS.with(|evaluations| {
            evaluations.set(evaluations.get().saturating_add(1));
        });
        let (MovePayload::Tabs(source) | MovePayload::Subtree(source)) = payload else {
            return true;
        };
        let Some(root) = self.workspace.root(source.root()) else {
            return false;
        };
        root.central
            .is_none_or(|central| !self.workspace.subtree_contains(source.node(), central))
    }

    pub(in crate::engine) fn core_contained_rehome_is_valid(
        &self,
        source: &NodeSource,
        target: RootPresentationTarget,
        proposal: crate::input::intent::ContainedTearOffProposal,
    ) -> bool {
        let RootPresentationTarget::Contained {
            surface,
            floating,
            rect,
            position,
        } = target
        else {
            return false;
        };
        if source.root() != proposal.root()
            || surface != proposal.surface()
            || floating != proposal.floating()
            || rect != proposal.rect()
            || position != proposal.position()
            || self.workspace.surface(surface).is_none()
        {
            return false;
        }
        match self.workspace.presentation_for_root(source.root()) {
            Some(crate::graph::workspace::RootPresentationOwner::Main {
                surface: source_surface,
            }) => {
                self.workspace.surface(source_surface).is_some()
                    && self.workspace.contained_floating(floating).is_none()
            }
            Some(crate::graph::workspace::RootPresentationOwner::Contained {
                surface: source_surface,
                floating: source_floating,
            }) => {
                source_floating == floating
                    && (source_surface != surface
                        || self
                            .workspace
                            .contained_floating(floating)
                            .is_some_and(|record| record.rect == rect))
            }
            None => false,
        }
    }

    pub(in crate::engine) fn core_contained_candidate(
        &self,
        drag: &crate::interaction::ActiveDrag,
        current_pointer: SurfacePointer,
        local_candidate: Option<&crate::presentation::scene::SurfacePlanScene>,
    ) -> CoreContainedCandidate {
        let (root, floating, source_rect, initial_pointer, minimum_size, position) =
            match &drag.origin {
                FrozenDragOrigin::Contained(_) => return CoreContainedCandidate::Rejected,
                FrozenDragOrigin::Workspace => {
                    let Some(offer) = drag.contained_offer else {
                        return CoreContainedCandidate::None;
                    };
                    if current_pointer.surface() != offer.anchor().surface() {
                        return CoreContainedCandidate::None;
                    }
                    (
                        offer.root(),
                        offer.floating(),
                        offer.requested_rect(),
                        offer.anchor().position(),
                        offer.minimum_size(),
                        offer.position(),
                    )
                }
            };
        let Ok(requested_rect) =
            translated_contained_rect(source_rect, initial_pointer, current_pointer.position())
        else {
            return CoreContainedCandidate::Rejected;
        };
        let placement = match local_candidate.map_or_else(
            || self.contained_placement(current_pointer.surface(), requested_rect, minimum_size),
            |candidate| {
                Self::contained_placement_from_local_candidate(
                    candidate,
                    self.version,
                    current_pointer.surface(),
                    requested_rect,
                    minimum_size,
                )
            },
        ) {
            Ok(placement) => placement,
            Err(
                ContainedPlacementUnavailable::UnrepresentableGeometry { .. }
                | ContainedPlacementUnavailable::ProofMismatch { .. },
            ) => {
                return CoreContainedCandidate::Rejected;
            }
            Err(
                ContainedPlacementUnavailable::SceneUnavailable
                | ContainedPlacementUnavailable::StaleScene { .. }
                | ContainedPlacementUnavailable::MissingSurface { .. }
                | ContainedPlacementUnavailable::BootstrapSurface { .. },
            ) => {
                return CoreContainedCandidate::Cancel(InteractionCancelReason::SceneUnavailable);
            }
            Err(
                ContainedPlacementUnavailable::StaleSurface { .. }
                | ContainedPlacementUnavailable::PendingPaintSurface { .. },
            ) => {
                return CoreContainedCandidate::None;
            }
        };
        CoreContainedCandidate::Proposal(crate::input::intent::ContainedTearOffProposal::new(
            root, floating, placement, position,
        ))
    }
}

impl DockEngine {
    pub(in crate::engine) fn validate_payload(
        &self,
        payload: &MovePayload,
    ) -> Result<(), crate::graph::error::CommandError> {
        self.validate_payload_with_index(payload, None)
    }

    pub(in crate::engine) fn validate_payload_with_index(
        &self,
        payload: &MovePayload,
        index: Option<&crate::graph::workspace::WorkspaceIndex>,
    ) -> Result<(), crate::graph::error::CommandError> {
        use crate::graph::error::ReferenceRole;

        let verify = |root, node, expected: &crate::graph::command::RootReference| match index {
            Some(index) => index.verify_reference(
                &self.workspace,
                self.version,
                root,
                node,
                expected,
                ReferenceRole::Source,
            ),
            None => self
                .workspace
                .verify_reference(root, node, expected, ReferenceRole::Source),
        };
        match payload {
            MovePayload::Item(source) => {
                verify(source.root(), source.tabs(), source.reference())?;
                if let Some(index) = index {
                    index.capture_item_source(
                        &self.workspace,
                        self.version,
                        source.root(),
                        source.tabs(),
                        source.item(),
                    )?;
                } else {
                    self.workspace.capture_item_source(
                        source.root(),
                        source.tabs(),
                        source.item(),
                    )?;
                }
            }
            MovePayload::Tabs(source) => {
                verify(source.root(), source.node(), source.reference())?;
                match self.workspace.node(source.node()) {
                    Some(crate::graph::Node::Tabs { items, .. }) if !items.is_empty() => {}
                    Some(crate::graph::Node::Tabs { .. }) => {
                        return Err(crate::graph::error::CommandError::EmptyPayload {
                            node: source.node(),
                        });
                    }
                    Some(crate::graph::Node::Split { .. }) => {
                        return Err(crate::graph::error::CommandError::NodeIsNotTabs {
                            node: source.node(),
                        });
                    }
                    None => {
                        return Err(crate::graph::error::CommandError::MissingNode {
                            role: ReferenceRole::Source,
                            node: source.node(),
                        });
                    }
                }
            }
            MovePayload::Subtree(source) => {
                verify(source.root(), source.node(), source.reference())?;
                if self
                    .workspace
                    .collect_items_in_subtree(source.node())
                    .is_empty()
                {
                    return Err(crate::graph::error::CommandError::EmptyPayload {
                        node: source.node(),
                    });
                }
            }
        }
        Ok(())
    }

    pub(in crate::engine) fn validate_node_source(
        &self,
        source: &crate::graph::command::NodeSource,
    ) -> Result<(), crate::graph::error::CommandError> {
        use crate::graph::error::ReferenceRole;

        self.workspace.verify_reference(
            source.root(),
            source.node(),
            source.reference(),
            ReferenceRole::Source,
        )
    }

    pub(in crate::engine) fn tear_off_command_from_complete_root(
        payload: &MovePayload,
        target: RootPresentationTarget,
        new_root: crate::ids::RootId,
        complete_root: Option<NodeSource>,
    ) -> WorkspaceCommand {
        match (complete_root, target) {
            (Some(source), target) => WorkspaceCommand::RehomeRoot { source, target },
            (None, RootPresentationTarget::NewSurface { surface }) => {
                WorkspaceCommand::CreateSurfaceRoot {
                    surface,
                    root: new_root,
                    content: RootContent::Move(payload.clone()),
                }
            }
            (None, RootPresentationTarget::Main { surface }) => WorkspaceCommand::InstallMainRoot {
                surface,
                root: new_root,
                content: RootContent::Move(payload.clone()),
            },
            (
                None,
                RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    position,
                },
            ) => WorkspaceCommand::CreateContainedRoot {
                surface,
                root: new_root,
                floating,
                rect,
                position,
                content: RootContent::Move(payload.clone()),
            },
        }
    }

    pub(in crate::engine) fn contained_presentation_command(
        &self,
        payload: &MovePayload,
        proposal: crate::input::intent::ContainedTearOffProposal,
        complete_root: Option<NodeSource>,
        frozen_origin: Option<&FrozenContainedDragOrigin>,
    ) -> Option<WorkspaceCommand> {
        if let Some(source) = complete_root.as_ref()
            && let Some(current) = self.workspace.contained_floating(proposal.floating())
            && current.root == source.root()
            && self.workspace.presentation_for_root(source.root())
                == Some(crate::graph::workspace::RootPresentationOwner::Contained {
                    surface: proposal.surface(),
                    floating: proposal.floating(),
                })
        {
            let expected_roster = match frozen_origin {
                Some(origin)
                    if origin.surface == proposal.surface()
                        && origin.root == source.root()
                        && origin.floating == proposal.floating()
                        && origin.source_rect == current.rect =>
                {
                    origin.source_roster.clone()
                }
                Some(_) => return None,
                None => self
                    .workspace
                    .capture_contained_roster(proposal.surface())
                    .ok()?,
            };
            return Some(WorkspaceCommand::UpdateContainedPresentation {
                source: source.clone(),
                floating: proposal.floating(),
                expected_rect: current.rect,
                expected_roster,
                rect: proposal.rect(),
                position: proposal.position(),
            });
        }

        Some(Self::tear_off_command_from_complete_root(
            payload,
            RootPresentationTarget::Contained {
                surface: proposal.surface(),
                floating: proposal.floating(),
                rect: proposal.rect(),
                position: proposal.position(),
            },
            proposal.root(),
            complete_root,
        ))
    }

    pub(in crate::engine) fn complete_root_source(
        &self,
        payload: &MovePayload,
    ) -> Result<Option<crate::graph::command::NodeSource>, crate::graph::error::CommandError> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        let record = self
            .workspace
            .root(root)
            .ok_or(crate::graph::error::CommandError::MissingRoot { root })?;
        let complete = match payload {
            MovePayload::Item(_) => self.workspace.item_move_removes_root(
                root,
                self.workspace.collect_items_in_subtree(record.node).len(),
            ),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                source.node() == record.node
            }
        };
        if complete {
            self.workspace
                .capture_node_source(root, record.node)
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub(in crate::engine) fn payload_surface(
        &self,
        payload: &MovePayload,
    ) -> Option<crate::ids::SurfaceId> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        self.root_surface(root)
    }

    pub(in crate::engine) fn root_surface(
        &self,
        root: crate::ids::RootId,
    ) -> Option<crate::ids::SurfaceId> {
        match self.workspace.presentation_for_root(root)? {
            crate::graph::workspace::RootPresentationOwner::Main { surface }
            | crate::graph::workspace::RootPresentationOwner::Contained { surface, .. } => {
                Some(surface)
            }
        }
    }
}
