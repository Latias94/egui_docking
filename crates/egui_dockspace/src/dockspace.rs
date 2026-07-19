//! Authoritative egui facade over [`dockspace::engine::DockEngine`].

use std::collections::BTreeMap;
use std::{fmt::Debug, hash::Hash};

use dockspace::RootPresentationOwner;
use dockspace::command::{MovePayload, NodeSource, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::{CommandError, ReferenceRole};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Node, Workspace};
use dockspace::ids::{FloatingPresentationId, InputSequence, ItemId, RootId, SurfaceId};
use dockspace::intent::{
    ContainedTearOffProposal, RendererIntent, TargetAuthority, TearOffRequest,
};
use dockspace::interaction::{
    ContainedTransformSessionId, DragSessionId, InteractionCancelReason, InteractionStatus,
    ResizeSessionId,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene, SceneStamp, SealedScene, SurfaceScene};
use dockspace::transition::{EngineTransition, InputOutcome, WorkspaceVersion};
use egui::{Id, Rect, Ui, ViewportId};

use crate::builder::DockspaceBuilder;
use crate::error::DockspaceError;
use crate::pane::{PaneCloseResponse, PaneView};
use crate::presentation::{PresentationIdSource, TearOffMode};
use crate::projection::{
    ProjectionError, ProjectionFingerprint, SurfacePlan, build_surface_plan,
    floating_minimum_for_payload,
};
use crate::renderer::{ContainedMoveCandidate, RenderAction, RenderOutput, paint_surface};
use crate::response::{
    DockspaceCapability, DockspaceInputRejection, DockspaceResponse, DockspaceSurfaceStatus,
    DockspaceUnavailableReason,
};
use crate::style::DockStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameKey {
    viewport: ViewportId,
    frame: u64,
}

#[derive(Clone, Copy)]
struct FrameRequest {
    key: FrameKey,
    surface: SurfaceId,
    bounds: Rect,
    pass: usize,
}

#[derive(Clone, Copy)]
struct RenderAuthority {
    workspace: WorkspaceVersion,
    scene: SceneStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureIdentity {
    Drag(DragSessionId),
    Resize(ResizeSessionId),
    ContainedTransform(ContainedTransformSessionId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContainedReservation {
    session: DragSessionId,
    root: RootId,
    floating: FloatingPresentationId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainedAllocation {
    Reserved(ContainedReservation),
    Unavailable(DragSessionId),
}

struct FrameState {
    key: FrameKey,
    surface: SurfaceId,
    bounds: Rect,
    pass: usize,
    plan: Option<SurfacePlan>,
    authority: Option<RenderAuthority>,
    style: DockStyle,
    actions: Vec<RenderAction>,
    capture_errors: Vec<CommandError>,
}

struct PublishedProjection {
    workspace: WorkspaceVersion,
    scene: SceneStamp,
    ready: BTreeMap<SurfaceId, ReadySurfaceScene>,
}

#[derive(Default)]
struct BoundaryOutput {
    transitions: Vec<EngineTransition>,
    capture_errors: Vec<CommandError>,
    input_rejections: Vec<DockspaceInputRejection>,
}

struct PaintOutput {
    missing_panes: Vec<ItemId>,
    interactions_current: bool,
    surface_status: DockspaceSurfaceStatus,
}

/// Stateful egui adapter whose [`DockEngine`] is the sole docking authority.
pub struct Dockspace {
    pub(crate) id: Id,
    pub(crate) engine: DockEngine,
    pub(crate) style: DockStyle,
    tear_off_mode: TearOffMode,
    pub(crate) presentation_ids: Option<Box<dyn PresentationIdSource>>,
    frames: BTreeMap<SurfaceId, FrameState>,
    viewport_frames: BTreeMap<ViewportId, (u64, SurfaceId)>,
    surface_bounds: BTreeMap<SurfaceId, Rect>,
    surface_plans: BTreeMap<SurfaceId, SurfacePlan>,
    previous_projections: BTreeMap<SurfaceId, ProjectionFingerprint>,
    published_projection: Option<PublishedProjection>,
    last_contained_unavailable: Option<DockspaceUnavailableReason>,
    contained_allocation: Option<ContainedAllocation>,
}

impl Dockspace {
    /// Starts a builder for a stable egui instance and renderer-neutral workspace.
    pub fn builder(id_salt: impl Hash + Debug, workspace: Workspace) -> DockspaceBuilder {
        DockspaceBuilder::new(id_salt, workspace)
    }

    pub(crate) fn from_parts(
        id: Id,
        workspace: Workspace,
        policy: DockPolicy,
        style: DockStyle,
        tear_off_mode: TearOffMode,
        presentation_ids: Option<Box<dyn PresentationIdSource>>,
    ) -> Result<Self, DockspaceError> {
        Ok(Self {
            id,
            engine: DockEngine::new(workspace, policy)?,
            style,
            tear_off_mode,
            presentation_ids,
            frames: BTreeMap::new(),
            viewport_frames: BTreeMap::new(),
            surface_bounds: BTreeMap::new(),
            surface_plans: BTreeMap::new(),
            previous_projections: BTreeMap::new(),
            published_projection: None,
            last_contained_unavailable: None,
            contained_allocation: None,
        })
    }

    /// Returns the stable egui identity used to scope every adapter widget.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Returns the renderer-neutral authority for advanced read-only inspection.
    #[must_use]
    pub const fn engine(&self) -> &DockEngine {
        &self.engine
    }

    /// Returns current fixed renderer geometry and colors.
    #[must_use]
    pub const fn style(&self) -> &DockStyle {
        &self.style
    }

    /// Replaces style after complete deterministic validation.
    ///
    /// A repeated egui pass keeps painting the style frozen by its first pass;
    /// the replacement becomes visible at the next complete frame boundary.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError::Style`] and preserves the prior style when invalid.
    pub fn set_style(&mut self, style: DockStyle) -> Result<(), DockspaceError> {
        style.validate()?;
        self.style = style;
        Ok(())
    }

    /// Queues one exact checked workspace command for the next frame boundary.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError::Engine`] if the input sequence is exhausted.
    pub fn enqueue_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<InputSequence, DockspaceError> {
        self.engine.enqueue_command(command).map_err(Into::into)
    }

    /// Queues an epoch-advancing complete workspace replacement.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError::Engine`] if the input sequence is exhausted.
    pub fn replace_workspace(
        &mut self,
        workspace: Workspace,
    ) -> Result<InputSequence, DockspaceError> {
        self.engine
            .enqueue_workspace_replacement(workspace)
            .map_err(Into::into)
    }

    /// Queues a complete policy replacement against the current engine version.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError::Engine`] if the input sequence is exhausted.
    pub fn set_policy(&mut self, policy: DockPolicy) -> Result<InputSequence, DockspaceError> {
        self.engine
            .enqueue(EngineInput::ReplacePolicy {
                expected: self.engine.version(),
                policy,
            })
            .map_err(Into::into)
    }

    /// Paints one logical surface and advances inputs from the preceding egui frame.
    ///
    /// Renderer actions are never reduced while they are being painted. The
    /// first pass of the next complete egui frame reduces them, publishes a new
    /// sealed scene, and freezes one projection for every repeated pass.
    /// Drag and contained-transform release commits only the last proposal that
    /// completed a paint/acknowledgement cycle. A pointer observation produced
    /// by the release pass itself is not an unseen replacement proposal.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError`] when projection, scene publication, or the
    /// atomic engine boundary fails. An error never applies a partial command.
    pub fn show(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<DockspaceResponse, DockspaceError> {
        let request = FrameRequest {
            key: FrameKey {
                viewport: ui.ctx().viewport_id(),
                frame: ui.ctx().cumulative_frame_nr(),
            },
            surface,
            bounds: ui.available_rect_before_wrap(),
            pass: ui.ctx().current_pass_index(),
        };
        self.validate_viewport_surface(request)?;
        let is_new_frame = self
            .frames
            .get(&surface)
            .is_none_or(|frame| frame.key != request.key);
        let mut boundary = BoundaryOutput::default();

        if is_new_frame {
            self.begin_frame(request, ui, panes, &mut boundary)?;
        } else {
            self.validate_repeated_pass(
                request.surface,
                request.bounds,
                request.key,
                request.pass,
            )?;
        }

        let painted = self.paint_frame(surface, ui, panes)?;
        ui.advance_cursor_after_rect(request.bounds);

        let frame = self
            .frames
            .get(&surface)
            .ok_or(DockspaceError::FrameStateUnavailable)?;
        boundary
            .capture_errors
            .extend(frame.capture_errors.iter().cloned());
        Ok(DockspaceResponse {
            transitions: boundary.transitions,
            missing_panes: painted.missing_panes,
            capture_errors: boundary.capture_errors,
            input_rejections: boundary.input_rejections,
            interactions_current: painted.interactions_current,
            surface_status: painted.surface_status,
            contained_capability: self.contained_capability(surface),
        })
    }

    fn begin_frame(
        &mut self,
        request: FrameRequest,
        ui: &Ui,
        panes: &mut dyn PaneView,
        output: &mut BoundaryOutput,
    ) -> Result<(), DockspaceError> {
        let previous = self.frames.remove(&request.surface);
        self.reduce_if_pending(&mut output.transitions)?;
        if let Some(previous) = previous.filter(|frame| !frame.actions.is_empty()) {
            let authority = previous
                .authority
                .ok_or(DockspaceError::FrameStateUnavailable)?;
            if let Some(rejection) =
                self.renderer_input_rejection(authority, previous.actions.len())
            {
                let reason = match rejection {
                    DockspaceInputRejection::StaleWorkspace { .. } => {
                        InteractionCancelReason::WorkspaceChanged
                    }
                    DockspaceInputRejection::StaleScene { .. } => {
                        InteractionCancelReason::SceneUnavailable
                    }
                };
                output.input_rejections.push(rejection);
                self.cancel_stale_interaction(reason, &mut output.transitions)?;
            } else {
                self.reduce_render_actions(
                    previous.actions,
                    panes,
                    &mut output.transitions,
                    &mut output.capture_errors,
                )?;
            }
        }
        if self.engine.interaction().active_drag_view().is_none() {
            self.contained_allocation = None;
            self.last_contained_unavailable = None;
        }

        let style = self.style.clone();
        let plan = if self.engine.workspace().surface(request.surface).is_some() {
            self.surface_bounds.insert(request.surface, request.bounds);
            self.prepare_surfaces(ui, panes, &mut output.transitions)?;
            self.surface_plans.get(&request.surface).cloned()
        } else {
            self.surface_bounds.remove(&request.surface);
            self.surface_plans.remove(&request.surface);
            self.previous_projections.remove(&request.surface);
            None
        };
        let authority = plan
            .as_ref()
            .map(|_| self.current_render_authority(request.surface))
            .transpose()?;
        self.frames.insert(
            request.surface,
            FrameState {
                key: request.key,
                surface: request.surface,
                bounds: request.bounds,
                pass: request.pass,
                plan,
                authority,
                style,
                actions: Vec::new(),
                capture_errors: Vec::new(),
            },
        );
        Ok(())
    }

    fn paint_frame(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<PaintOutput, DockspaceError> {
        let (plan, style) = self
            .frames
            .get(&surface)
            .map(|frame| (frame.plan.clone(), frame.style.clone()))
            .ok_or(DockspaceError::FrameStateUnavailable)?;
        let Some(plan) = plan else {
            self.previous_projections.remove(&surface);
            return Ok(PaintOutput {
                missing_panes: Vec::new(),
                interactions_current: false,
                surface_status: DockspaceSurfaceStatus::Absent,
            });
        };
        let interactions_current = self
            .previous_projections
            .get(&surface)
            .is_some_and(|previous| previous == &plan.fingerprint);
        if !interactions_current {
            // Geometry-changing actions require a fresh egui pass before the
            // new hit graph can become authoritative.
            ui.ctx().request_discard("dockspace projection changed");
        }
        let output = paint_surface(
            ui,
            self.id,
            &plan,
            self.engine.workspace(),
            panes,
            &style,
            self.engine.interaction(),
            interactions_current,
        );
        self.record_render_output(surface, output)?;
        self.previous_projections
            .insert(surface, plan.fingerprint.clone());
        if !interactions_current
            || self
                .frames
                .get(&surface)
                .is_some_and(|frame| !frame.actions.is_empty())
        {
            ui.ctx().request_repaint();
        }
        Ok(PaintOutput {
            missing_panes: plan.missing_items,
            interactions_current,
            surface_status: DockspaceSurfaceStatus::Ready,
        })
    }

    fn current_render_authority(
        &self,
        surface: SurfaceId,
    ) -> Result<RenderAuthority, DockspaceError> {
        let scene = self
            .engine
            .scene()
            .filter(|scene| matches!(scene.surface(surface), Some(SurfaceScene::Ready(_))))
            .ok_or(DockspaceError::ReadySceneUnavailable { surface })?;
        Ok(RenderAuthority {
            workspace: self.engine.version(),
            scene: scene.stamp(),
        })
    }

    fn renderer_input_rejection(
        &self,
        authority: RenderAuthority,
        dropped_actions: usize,
    ) -> Option<DockspaceInputRejection> {
        let current_workspace = self.engine.version();
        if authority.workspace != current_workspace {
            return Some(DockspaceInputRejection::StaleWorkspace {
                expected: authority.workspace,
                current: current_workspace,
                dropped_actions,
            });
        }
        let current_scene = self.engine.scene().map(SealedScene::stamp);
        (current_scene != Some(authority.scene)).then_some(DockspaceInputRejection::StaleScene {
            expected: authority.scene,
            current: current_scene,
            dropped_actions,
        })
    }

    fn validate_viewport_surface(&mut self, request: FrameRequest) -> Result<(), DockspaceError> {
        if let Some((frame, expected)) = self.viewport_frames.get(&request.key.viewport)
            && *frame == request.key.frame
            && *expected != request.surface
        {
            return Err(DockspaceError::SurfaceChangedWithinFrame {
                expected: *expected,
                actual: request.surface,
            });
        }
        self.viewport_frames
            .insert(request.key.viewport, (request.key.frame, request.surface));
        Ok(())
    }

    fn cancel_stale_interaction(
        &mut self,
        reason: InteractionCancelReason,
        transitions: &mut Vec<EngineTransition>,
    ) -> Result<(), DockspaceError> {
        let intent = match self.engine.interaction().status() {
            InteractionStatus::Armed { session } | InteractionStatus::Dragging { session } => {
                Some(RendererIntent::CancelDrag { session, reason })
            }
            InteractionStatus::Resizing { session } => {
                Some(RendererIntent::CancelResize { session, reason })
            }
            InteractionStatus::ContainedTransforming { session } => {
                Some(RendererIntent::CancelContainedTransform { session, reason })
            }
            InteractionStatus::Idle => None,
        };
        if let Some(intent) = intent {
            self.engine.enqueue_renderer_intent(intent)?;
            self.reduce_if_pending(transitions)?;
        }
        Ok(())
    }

    fn validate_repeated_pass(
        &mut self,
        surface: SurfaceId,
        bounds: Rect,
        key: FrameKey,
        pass: usize,
    ) -> Result<(), DockspaceError> {
        let frame = self
            .frames
            .get_mut(&surface)
            .ok_or(DockspaceError::FrameStateUnavailable)?;
        if frame.surface != surface {
            return Err(DockspaceError::SurfaceChangedWithinFrame {
                expected: frame.surface,
                actual: surface,
            });
        }
        if frame.bounds != bounds {
            return Err(DockspaceError::BoundsChangedWithinFrame {
                expected: frame.bounds,
                actual: bounds,
            });
        }
        if frame.pass == pass {
            return Err(DockspaceError::DuplicateShowInPass {
                frame: key.frame,
                pass,
            });
        }
        frame.pass = pass;
        Ok(())
    }

    fn record_render_output(
        &mut self,
        surface: SurfaceId,
        output: RenderOutput,
    ) -> Result<(), DockspaceError> {
        let frame = self
            .frames
            .get_mut(&surface)
            .ok_or(DockspaceError::FrameStateUnavailable)?;
        for action in output.actions {
            if let Some(existing) = frame
                .actions
                .iter_mut()
                .find(|existing| replaces_observation(existing, &action))
            {
                *existing = action;
            } else if !frame.actions.contains(&action) {
                frame.actions.push(action);
            }
        }
        for error in output.capture_errors {
            if !frame.capture_errors.contains(&error) {
                frame.capture_errors.push(error);
            }
        }
        Ok(())
    }

    fn reduce_if_pending(
        &mut self,
        transitions: &mut Vec<EngineTransition>,
    ) -> Result<(), DockspaceError> {
        if !self.engine.pending_inputs().is_empty() {
            transitions.push(self.engine.reduce_pending()?);
        }
        Ok(())
    }

    fn reduce_render_actions(
        &mut self,
        actions: Vec<RenderAction>,
        panes: &mut dyn PaneView,
        transitions: &mut Vec<EngineTransition>,
        capture_errors: &mut Vec<CommandError>,
    ) -> Result<(), DockspaceError> {
        let actions = normalize_render_actions(actions);
        self.reduce_cancellations(&actions, transitions)?;
        self.reduce_application_actions(&actions, panes, transitions, capture_errors)?;
        for action in actions {
            self.queue_gesture_action(action, panes)?;
        }
        self.reduce_if_pending(transitions)
    }

    fn reduce_cancellations(
        &mut self,
        actions: &[RenderAction],
        transitions: &mut Vec<EngineTransition>,
    ) -> Result<(), DockspaceError> {
        for action in actions {
            let intent = match action {
                RenderAction::CancelDrag { session, reason } => Some(RendererIntent::CancelDrag {
                    session: *session,
                    reason: *reason,
                }),
                RenderAction::CancelResize { session, reason } => {
                    Some(RendererIntent::CancelResize {
                        session: *session,
                        reason: *reason,
                    })
                }
                RenderAction::CancelContainedTransform { session, reason } => {
                    Some(RendererIntent::CancelContainedTransform {
                        session: *session,
                        reason: *reason,
                    })
                }
                _ => None,
            };
            if let Some(intent) = intent {
                self.engine.enqueue_renderer_intent(intent)?;
            }
        }
        self.reduce_if_pending(transitions)
    }

    fn reduce_application_actions(
        &mut self,
        actions: &[RenderAction],
        panes: &mut dyn PaneView,
        transitions: &mut Vec<EngineTransition>,
        capture_errors: &mut Vec<CommandError>,
    ) -> Result<(), DockspaceError> {
        for action in actions {
            let command = match action {
                RenderAction::Select(source) => Some(WorkspaceCommand::Select {
                    source: source.clone(),
                }),
                RenderAction::CloseRequested(source) => {
                    self.close_item_command(source, panes, capture_errors)
                }
                RenderAction::CloseRootRequested(source) => {
                    self.close_root_command(source, panes, capture_errors)
                }
                RenderAction::AdjustResize { split, weights } => {
                    Some(WorkspaceCommand::ResizeSplit {
                        split: split.clone(),
                        weights: weights.clone(),
                    })
                }
                RenderAction::RaiseContained {
                    surface,
                    root,
                    floating,
                    expected_z_order,
                    expected_frontmost,
                } => Some(WorkspaceCommand::RaiseContained {
                    surface: *surface,
                    root: *root,
                    floating: *floating,
                    expected_z_order: *expected_z_order,
                    expected_frontmost: *expected_frontmost,
                }),
                _ => None,
            };
            if let Some(command) = command {
                self.engine.enqueue_command(command)?;
            }
        }
        self.reduce_if_pending(transitions)
    }

    fn queue_gesture_action(
        &mut self,
        action: RenderAction,
        panes: &dyn PaneView,
    ) -> Result<(), DockspaceError> {
        let intent = match action {
            action @ (RenderAction::ArmDrag(_)
            | RenderAction::BeginDrag { .. }
            | RenderAction::UpdateDrag { .. }
            | RenderAction::ReleaseDrag { .. }
            | RenderAction::AcknowledgePreview(_)) => self.drag_intent(action, panes)?,
            action @ (RenderAction::BeginResize { .. }
            | RenderAction::UpdateResize { .. }
            | RenderAction::ReleaseResize { .. }) => resize_intent(action),
            action @ (RenderAction::BeginContainedTransform { .. }
            | RenderAction::UpdateContainedTransform { .. }
            | RenderAction::ReleaseContainedTransform { .. }
            | RenderAction::AcknowledgeContainedTransformPreview(_)) => {
                contained_transform_intent(action)
            }
            RenderAction::Select(_)
            | RenderAction::CloseRequested(_)
            | RenderAction::CloseRootRequested(_)
            | RenderAction::AdjustResize { .. }
            | RenderAction::RaiseContained { .. }
            | RenderAction::CancelDrag { .. }
            | RenderAction::CancelResize { .. }
            | RenderAction::CancelContainedTransform { .. } => None,
        };
        if let Some(intent) = intent {
            self.engine.enqueue_renderer_intent(intent)?;
        }
        Ok(())
    }

    fn drag_intent(
        &mut self,
        action: RenderAction,
        panes: &dyn PaneView,
    ) -> Result<Option<RendererIntent>, DockspaceError> {
        Ok(match action {
            RenderAction::ArmDrag(payload) => Some(RendererIntent::ArmDrag {
                pointer: crate::renderer::PRIMARY_POINTER,
                button: crate::renderer::PRIMARY_BUTTON,
                payload,
            }),
            RenderAction::BeginDrag {
                session,
                pointer,
                button,
            } => Some(RendererIntent::BeginDrag {
                session,
                pointer,
                button,
            }),
            RenderAction::UpdateDrag {
                session,
                target,
                pointer_position,
                contained_move,
            } => Some(RendererIntent::UpdateDrag {
                session,
                tear_off: self.non_docking_request_for_update(
                    session,
                    &target,
                    pointer_position,
                    contained_move,
                    panes,
                )?,
                target,
            }),
            RenderAction::ReleaseDrag {
                session,
                pointer,
                button,
                button_state,
                target,
            } => {
                let active = self
                    .engine
                    .interaction()
                    .active_drag_view()
                    .filter(|view| view.session() == session);
                let target = active
                    .and_then(|view| view.target().cloned())
                    .unwrap_or(target);
                let tear_off = active.and_then(|view| view.tear_off().cloned());
                Some(RendererIntent::ReleaseDrag {
                    session,
                    pointer,
                    button,
                    button_state,
                    target,
                    tear_off,
                })
            }
            RenderAction::AcknowledgePreview(acknowledgement) => {
                Some(RendererIntent::AcknowledgePreview(acknowledgement))
            }
            _ => None,
        })
    }

    fn close_item_command(
        &self,
        source: &dockspace::command::ItemSource,
        panes: &mut dyn PaneView,
        capture_errors: &mut Vec<CommandError>,
    ) -> Option<WorkspaceCommand> {
        let current = match self.engine.workspace().capture_item_source(
            source.root(),
            source.tabs(),
            source.item(),
        ) {
            Ok(current) if current == *source => current,
            Ok(current) => {
                push_unique(
                    capture_errors,
                    CommandError::StaleNode {
                        role: ReferenceRole::Source,
                        node: source.tabs(),
                        expected: source.fingerprint().clone(),
                        actual: current.fingerprint().clone(),
                    },
                );
                return None;
            }
            Err(error) => {
                push_unique(capture_errors, error);
                return None;
            }
        };
        (panes.closeable(current.item()) && panes.close(current.item()) == PaneCloseResponse::Allow)
            .then_some(WorkspaceCommand::Close { source: current })
    }

    fn close_root_command(
        &self,
        source: &NodeSource,
        panes: &mut dyn PaneView,
        capture_errors: &mut Vec<CommandError>,
    ) -> Option<WorkspaceCommand> {
        let workspace = self.engine.workspace();
        let current = match workspace.capture_node_source(source.root(), source.node()) {
            Ok(current) if current == *source => current,
            Ok(current) => {
                push_unique(
                    capture_errors,
                    CommandError::StaleNode {
                        role: ReferenceRole::Source,
                        node: source.node(),
                        expected: source.fingerprint().clone(),
                        actual: current.fingerprint().clone(),
                    },
                );
                return None;
            }
            Err(error) => {
                push_unique(capture_errors, error);
                return None;
            }
        };
        let Some(root) = workspace.root(current.root()) else {
            push_unique(
                capture_errors,
                CommandError::MissingRoot {
                    root: current.root(),
                },
            );
            return None;
        };
        if root.node != current.node() {
            push_unique(
                capture_errors,
                CommandError::NodeIsNotRoot {
                    root: current.root(),
                    node: current.node(),
                },
            );
            return None;
        }
        let items = collect_subtree_items(workspace, root.node);
        if items.is_empty() || items.iter().any(|item| !panes.closeable(*item)) {
            return None;
        }
        let all_allowed = items.into_iter().fold(true, |all_allowed, item| {
            let allowed = panes.close(item) == PaneCloseResponse::Allow;
            all_allowed && allowed
        });
        if all_allowed {
            Some(WorkspaceCommand::CloseRoot { source: current })
        } else {
            None
        }
    }

    fn prepare_surfaces(
        &mut self,
        ui: &Ui,
        panes: &dyn PaneView,
        transitions: &mut Vec<EngineTransition>,
    ) -> Result<(), DockspaceError> {
        self.surface_bounds
            .retain(|surface, _| self.engine.workspace().surface(*surface).is_some());
        let correction_limit = self.engine.workspace().contained_floatings().count();
        for correction_index in 0..=correction_limit {
            let plans = self.project_known_surfaces(ui, panes)?;
            self.publish_surface_scene(&plans, transitions)?;

            let correction = plans.iter().find_map(|(surface, plan)| {
                plan.contained_placements
                    .iter()
                    .copied()
                    .find(|request| {
                        self.engine
                            .contained_placement(
                                *surface,
                                request.expected_rect,
                                request.minimum_size,
                            )
                            .is_ok_and(|placement| {
                                placement.clamped_rect() != request.expected_rect
                            })
                    })
                    .map(|request| (*surface, request))
            });
            let Some((surface, request)) = correction else {
                self.surface_plans = plans;
                return Ok(());
            };
            if correction_index == correction_limit {
                return Err(DockspaceError::ContainedPlacementRejected {
                    floating: request.floating,
                });
            }
            let Ok(placement) = self.engine.contained_placement(
                surface,
                request.expected_rect,
                request.minimum_size,
            ) else {
                self.last_contained_unavailable =
                    Some(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
                self.surface_plans = plans;
                return Ok(());
            };
            self.engine
                .enqueue_renderer_intent(RendererIntent::ApplyContainedPlacement {
                    root: request.root,
                    floating: request.floating,
                    expected_rect: request.expected_rect,
                    placement,
                })?;
            self.reduce_if_pending(transitions)?;
        }
        unreachable!("the bounded correction loop always returns")
    }

    fn project_known_surfaces(
        &self,
        ui: &Ui,
        panes: &dyn PaneView,
    ) -> Result<BTreeMap<SurfaceId, SurfacePlan>, DockspaceError> {
        self.engine
            .workspace()
            .surfaces()
            .filter_map(|(surface, _)| {
                self.surface_bounds
                    .get(&surface)
                    .copied()
                    .map(|bounds| (surface, bounds))
            })
            .map(|(surface, bounds)| {
                self.project_surface(surface, bounds, ui, panes)
                    .map(|plan| (surface, plan))
            })
            .collect()
    }

    fn project_surface(
        &self,
        surface: SurfaceId,
        bounds: Rect,
        ui: &Ui,
        panes: &dyn PaneView,
    ) -> Result<SurfacePlan, DockspaceError> {
        let resize = self
            .engine
            .interaction()
            .resize_weights()
            .map(|(_, split, weights)| (split, weights));
        build_surface_plan(
            ui,
            self.engine.workspace(),
            surface,
            bounds,
            panes,
            &self.style,
            resize,
        )
        .map_err(Into::into)
    }

    fn publish_surface_scene(
        &mut self,
        plans: &BTreeMap<SurfaceId, SurfacePlan>,
        transitions: &mut Vec<EngineTransition>,
    ) -> Result<(), DockspaceError> {
        let ready = plans
            .iter()
            .map(|(surface, plan)| (*surface, plan.ready.clone()))
            .collect::<BTreeMap<_, _>>();
        let reusable = self.published_projection.as_ref().is_some_and(|published| {
            published.workspace == self.engine.version()
                && published.ready == ready
                && self
                    .engine
                    .scene()
                    .is_some_and(|scene| scene.stamp() == published.scene)
        });
        if reusable {
            return Ok(());
        }

        let roster = self
            .engine
            .workspace()
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<Vec<_>>();
        let mut scene = BuildingScene::new(roster)?;
        for plan in plans.values() {
            scene.insert_ready(plan.ready.clone())?;
        }
        self.engine.enqueue_scene(scene)?;
        let transition = self.engine.reduce_pending()?;
        if let Some(error) = transition.reduced_inputs().iter().find_map(|input| {
            if let InputOutcome::SceneRejected { error } = input.outcome() {
                Some(error.clone())
            } else {
                None
            }
        }) {
            return Err(DockspaceError::SceneRejected(error));
        }
        transitions.push(transition);
        let published = self
            .engine
            .scene()
            .ok_or(DockspaceError::FrameStateUnavailable)?;
        for surface in plans.keys() {
            if !matches!(published.surface(*surface), Some(SurfaceScene::Ready(_))) {
                return Err(DockspaceError::ReadySceneUnavailable { surface: *surface });
            }
        }
        self.published_projection = Some(PublishedProjection {
            workspace: self.engine.version(),
            scene: published.stamp(),
            ready,
        });
        Ok(())
    }

    fn non_docking_request_for_update(
        &mut self,
        session: DragSessionId,
        target: &TargetAuthority,
        position: Option<LogicalPoint>,
        contained_move: Option<ContainedMoveCandidate>,
        panes: &dyn PaneView,
    ) -> Result<Option<TearOffRequest>, DockspaceError> {
        let Some(active) = self
            .engine
            .interaction()
            .active_drag_view()
            .filter(|active| active.session() == session)
        else {
            return Ok(None);
        };
        let payload = active.payload().clone();
        if let Some(candidate) = contained_move {
            return self.contained_move_for_update(session, target, &payload, candidate, panes);
        }
        let existing = active.tear_off().and_then(|request| match request {
            TearOffRequest::Contained(proposal) => Some(*proposal),
            TearOffRequest::Native { .. } => None,
        });
        let Some(position) = position else {
            return Ok(None);
        };
        let TargetAuthority::Local(local) = target else {
            return Ok(None);
        };
        if !matches!(local.target().known(), Some(None)) {
            return Ok(None);
        }
        if self.tear_off_mode != TearOffMode::Contained {
            self.last_contained_unavailable = Some(DockspaceUnavailableReason::TearOffModeDisabled);
            return Ok(None);
        }
        if !self.engine.policy().allows_contained_floating() {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::ContainedPolicyDisabled);
            return Ok(None);
        }
        let complete_root = complete_root_id(self.engine.workspace(), &payload);
        let complete_owner = complete_root.and_then(|root| {
            self.engine
                .workspace()
                .presentation_for_root(root)
                .map(|owner| (root, owner))
        });
        if complete_owner.is_some_and(|(_, owner)| match owner {
            RootPresentationOwner::Main { surface }
            | RootPresentationOwner::Contained { surface, .. } => surface == local.observer(),
        }) {
            // Moving inside the current host is a contained transform, while
            // rehoming a host's main root would remove the destination itself.
            return Ok(None);
        }

        let z_order = match existing.filter(|proposal| proposal.surface() == local.observer()) {
            Some(proposal) => proposal.z_order(),
            None => self.next_contained_z_order(local.observer())?,
        };
        let Some((root, floating)) = self.contained_presentation_for_update(
            session,
            complete_root,
            complete_owner,
            existing,
        ) else {
            return Ok(None);
        };

        let minimum =
            floating_minimum_for_payload(self.engine.workspace(), &payload, panes, &self.style)?;
        let origin = LogicalPoint::new(
            position.x() + f64::from(self.style.ghost_offset.x),
            position.y() + f64::from(self.style.ghost_offset.y),
        )
        .map_err(ProjectionError::from)?;
        let requested =
            LogicalRect::from_min_size(origin, minimum).map_err(ProjectionError::from)?;
        let Ok(placement) = self
            .engine
            .contained_placement(local.observer(), requested, minimum)
        else {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
            return Ok(None);
        };
        self.last_contained_unavailable = None;
        Ok(Some(TearOffRequest::Contained(
            ContainedTearOffProposal::new(root, floating, placement, z_order),
        )))
    }

    fn contained_move_for_update(
        &mut self,
        session: DragSessionId,
        target: &TargetAuthority,
        payload: &MovePayload,
        candidate: ContainedMoveCandidate,
        panes: &dyn PaneView,
    ) -> Result<Option<TearOffRequest>, DockspaceError> {
        let TargetAuthority::Local(local) = target else {
            return Ok(None);
        };
        if local.observer() != candidate.surface {
            return Ok(None);
        }
        if !self.engine.policy().allows_contained_floating() {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::ContainedPolicyDisabled);
            return Ok(None);
        }
        if complete_root_id(self.engine.workspace(), payload) != Some(candidate.root)
            || self
                .engine
                .workspace()
                .presentation_for_root(candidate.root)
                != Some(RootPresentationOwner::Contained {
                    surface: candidate.surface,
                    floating: candidate.floating,
                })
        {
            return Ok(None);
        }
        let Some(record) = self
            .engine
            .workspace()
            .contained_floating(candidate.floating)
            .copied()
            .filter(|record| {
                record.root == candidate.root
                    && record.surface == candidate.surface
                    && record.rect == candidate.expected_rect
            })
        else {
            return Ok(None);
        };
        let minimum =
            floating_minimum_for_payload(self.engine.workspace(), payload, panes, &self.style)?;
        if minimum != candidate.minimum_size {
            return Ok(None);
        }
        let Ok(placement) = self.engine.contained_placement(
            candidate.surface,
            candidate.requested_rect,
            candidate.minimum_size,
        ) else {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
            return Ok(None);
        };
        let active_matches = self
            .engine
            .interaction()
            .active_drag_view()
            .is_some_and(|active| active.session() == session && active.payload() == payload);
        if !active_matches {
            return Ok(None);
        }
        self.last_contained_unavailable = None;
        Ok(Some(TearOffRequest::Contained(
            ContainedTearOffProposal::new(
                candidate.root,
                candidate.floating,
                placement,
                record.z_order,
            ),
        )))
    }

    fn contained_presentation_for_update(
        &mut self,
        session: DragSessionId,
        complete_root: Option<RootId>,
        complete_owner: Option<(RootId, RootPresentationOwner)>,
        existing: Option<ContainedTearOffProposal>,
    ) -> Option<(RootId, dockspace::ids::FloatingPresentationId)> {
        if let Some((root, RootPresentationOwner::Contained { floating, .. })) = complete_owner {
            return Some((root, floating));
        }
        if let Some(proposal) = existing {
            self.contained_allocation = Some(ContainedAllocation::Reserved(ContainedReservation {
                session,
                root: proposal.root(),
                floating: proposal.floating(),
            }));
            return Some((proposal.root(), proposal.floating()));
        }
        match self.contained_allocation {
            Some(ContainedAllocation::Reserved(reservation)) if reservation.session == session => {
                return Some((reservation.root, reservation.floating));
            }
            Some(ContainedAllocation::Unavailable(attempt)) if attempt == session => return None,
            Some(_) | None => {}
        }
        self.contained_allocation = Some(ContainedAllocation::Unavailable(session));
        let Some(source) = self.presentation_ids.as_mut() else {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::PresentationIdSourceMissing);
            return None;
        };
        let Some(ids) = source.next_contained() else {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::PresentationIdsExhausted);
            return None;
        };
        let root = complete_root.unwrap_or(ids.root);
        let root_collision =
            complete_root.is_none() && self.engine.workspace().root(root).is_some();
        let floating_collision = self
            .engine
            .workspace()
            .contained_floating(ids.floating)
            .is_some();
        if root_collision || floating_collision {
            self.last_contained_unavailable =
                Some(DockspaceUnavailableReason::PresentationIdentityCollision);
            return None;
        }
        self.contained_allocation = Some(ContainedAllocation::Reserved(ContainedReservation {
            session,
            root,
            floating: ids.floating,
        }));
        Some((root, ids.floating))
    }

    fn next_contained_z_order(&self, surface: SurfaceId) -> Result<u64, DockspaceError> {
        self.engine
            .workspace()
            .contained_floatings()
            .filter(|(_, floating)| floating.surface == surface)
            .map(|(_, floating)| floating.z_order)
            .max()
            .map_or(Ok(1), |z_order| {
                z_order
                    .checked_add(1)
                    .ok_or(DockspaceError::ContainedZOrderExhausted { surface })
            })
    }

    fn contained_capability(&self, surface: SurfaceId) -> DockspaceCapability {
        if self.engine.workspace().surface(surface).is_none() {
            return DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceAbsent);
        }
        if self.tear_off_mode != TearOffMode::Contained {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::TearOffModeDisabled,
            );
        }
        if !self.engine.policy().allows_contained_floating() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::ContainedPolicyDisabled,
            );
        }
        if self.presentation_ids.is_none() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationIdSourceMissing,
            );
        }
        if let Some(reason) = self.last_contained_unavailable {
            return DockspaceCapability::Unavailable(reason);
        }
        if !matches!(
            self.engine.scene().and_then(|scene| scene.surface(surface)),
            Some(SurfaceScene::Ready(_))
        ) {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        DockspaceCapability::Supported
    }
}

fn push_unique(errors: &mut Vec<CommandError>, error: CommandError) {
    if !errors.contains(&error) {
        errors.push(error);
    }
}

fn resize_intent(action: RenderAction) -> Option<RendererIntent> {
    match action {
        RenderAction::BeginResize {
            pointer,
            button,
            split,
        } => Some(RendererIntent::BeginResize {
            pointer,
            button,
            split,
        }),
        RenderAction::UpdateResize { session, weights } => {
            Some(RendererIntent::UpdateResize { session, weights })
        }
        RenderAction::ReleaseResize {
            session,
            pointer,
            button,
            button_state,
        } => Some(RendererIntent::ReleaseResize {
            session,
            pointer,
            button,
            button_state,
        }),
        _ => None,
    }
}

fn contained_transform_intent(action: RenderAction) -> Option<RendererIntent> {
    match action {
        RenderAction::BeginContainedTransform {
            surface,
            root,
            floating,
            pointer,
            button,
            initial_pointer,
            kind,
            minimum_size,
        } => Some(RendererIntent::BeginContainedTransform {
            surface,
            root,
            floating,
            pointer,
            button,
            initial_pointer,
            kind,
            minimum_size,
        }),
        RenderAction::UpdateContainedTransform {
            session,
            current_pointer,
        } => Some(RendererIntent::UpdateContainedTransform {
            session,
            current_pointer,
        }),
        RenderAction::ReleaseContainedTransform {
            session,
            pointer,
            button,
            button_state,
        } => Some(RendererIntent::ReleaseContainedTransform {
            session,
            pointer,
            button,
            button_state,
        }),
        RenderAction::AcknowledgeContainedTransformPreview(acknowledgement) => Some(
            RendererIntent::AcknowledgeContainedTransformPreview(acknowledgement),
        ),
        _ => None,
    }
}

fn replaces_observation(existing: &RenderAction, incoming: &RenderAction) -> bool {
    match (existing, incoming) {
        (
            RenderAction::UpdateDrag {
                session: existing, ..
            },
            RenderAction::UpdateDrag {
                session: incoming, ..
            },
        )
        | (
            RenderAction::ReleaseDrag {
                session: existing, ..
            },
            RenderAction::ReleaseDrag {
                session: incoming, ..
            },
        ) => existing == incoming,
        (
            RenderAction::UpdateResize {
                session: existing, ..
            },
            RenderAction::UpdateResize {
                session: incoming, ..
            },
        ) => existing == incoming,
        (
            RenderAction::UpdateContainedTransform {
                session: existing, ..
            },
            RenderAction::UpdateContainedTransform {
                session: incoming, ..
            },
        ) => existing == incoming,
        _ => false,
    }
}

fn normalize_render_actions(actions: Vec<RenderAction>) -> Vec<RenderAction> {
    let mut normalized = Vec::with_capacity(actions.len());
    for incoming in actions {
        let Some((identity, incoming_priority)) = terminal_action(&incoming) else {
            if !normalized.contains(&incoming) {
                normalized.push(incoming);
            }
            continue;
        };

        if let Some((index, existing_priority)) =
            normalized.iter().enumerate().find_map(|(index, existing)| {
                terminal_action(existing)
                    .filter(|(existing, _)| *existing == identity)
                    .map(|(_, priority)| (index, priority))
            })
        {
            if incoming_priority >= existing_priority {
                normalized[index] = incoming;
            }
        } else {
            normalized.push(incoming);
        }
    }

    let cancelled = normalized
        .iter()
        .filter_map(cancelled_gesture)
        .collect::<Vec<_>>();
    normalized.retain(|action| {
        let Some(identity) = gesture_action_identity(action) else {
            return true;
        };
        !cancelled.contains(&identity) || cancelled_gesture(action) == Some(identity)
    });

    // Release can only consume the last proposal that completed a paint/ack
    // cycle. Same-pass observations are deliberately excluded from proof.
    let released = normalized
        .iter()
        .filter_map(terminal_release_identity)
        .collect::<Vec<_>>();
    if !released.is_empty() {
        normalized.retain(|action| {
            let Some(identity) = gesture_action_identity(action) else {
                return true;
            };
            !released.contains(&identity)
                || !matches!(
                    action,
                    RenderAction::UpdateDrag { .. } | RenderAction::UpdateContainedTransform { .. }
                )
        });
    }
    normalized
}

fn terminal_release_identity(action: &RenderAction) -> Option<GestureIdentity> {
    match action {
        RenderAction::ReleaseDrag { session, .. } => Some(GestureIdentity::Drag(*session)),
        RenderAction::ReleaseContainedTransform { session, .. } => {
            Some(GestureIdentity::ContainedTransform(*session))
        }
        _ => None,
    }
}

fn terminal_action(action: &RenderAction) -> Option<(GestureIdentity, u8)> {
    match action {
        RenderAction::ReleaseDrag { session, .. } => Some((GestureIdentity::Drag(*session), 3)),
        RenderAction::CancelDrag { session, reason } => Some((
            GestureIdentity::Drag(*session),
            cancellation_priority(*reason),
        )),
        RenderAction::ReleaseResize { session, .. } => Some((GestureIdentity::Resize(*session), 3)),
        RenderAction::CancelResize { session, reason } => Some((
            GestureIdentity::Resize(*session),
            cancellation_priority(*reason),
        )),
        RenderAction::ReleaseContainedTransform { session, .. } => {
            Some((GestureIdentity::ContainedTransform(*session), 3))
        }
        RenderAction::CancelContainedTransform { session, reason } => Some((
            GestureIdentity::ContainedTransform(*session),
            cancellation_priority(*reason),
        )),
        _ => None,
    }
}

const fn cancellation_priority(reason: InteractionCancelReason) -> u8 {
    match reason {
        InteractionCancelReason::Escape => 4,
        InteractionCancelReason::ReleasedBeforeDrag => 3,
        InteractionCancelReason::FocusLost => 2,
        InteractionCancelReason::CaptureLost => 1,
        InteractionCancelReason::UnknownButtonState
        | InteractionCancelReason::UnknownTargetAuthority
        | InteractionCancelReason::NativeCapabilityUnknown
        | InteractionCancelReason::NativeCapabilityUnavailable
        | InteractionCancelReason::NativePlacementUnavailable
        | InteractionCancelReason::SourceVanished
        | InteractionCancelReason::WorkspaceChanged
        | InteractionCancelReason::PolicyChanged
        | InteractionCancelReason::SurfaceClosed
        | InteractionCancelReason::SceneUnavailable
        | InteractionCancelReason::ReplacedByNewGesture
        | InteractionCancelReason::WorkspaceRestored => 0,
    }
}

fn cancelled_gesture(action: &RenderAction) -> Option<GestureIdentity> {
    match action {
        RenderAction::CancelDrag { session, .. } => Some(GestureIdentity::Drag(*session)),
        RenderAction::CancelResize { session, .. } => Some(GestureIdentity::Resize(*session)),
        RenderAction::CancelContainedTransform { session, .. } => {
            Some(GestureIdentity::ContainedTransform(*session))
        }
        _ => None,
    }
}

fn gesture_action_identity(action: &RenderAction) -> Option<GestureIdentity> {
    match action {
        RenderAction::BeginDrag { session, .. }
        | RenderAction::UpdateDrag { session, .. }
        | RenderAction::ReleaseDrag { session, .. }
        | RenderAction::CancelDrag { session, .. } => Some(GestureIdentity::Drag(*session)),
        RenderAction::AcknowledgePreview(acknowledgement) => {
            Some(GestureIdentity::Drag(acknowledgement.token().session()))
        }
        RenderAction::UpdateResize { session, .. }
        | RenderAction::ReleaseResize { session, .. }
        | RenderAction::CancelResize { session, .. } => Some(GestureIdentity::Resize(*session)),
        RenderAction::UpdateContainedTransform { session, .. }
        | RenderAction::ReleaseContainedTransform { session, .. }
        | RenderAction::CancelContainedTransform { session, .. } => {
            Some(GestureIdentity::ContainedTransform(*session))
        }
        RenderAction::AcknowledgeContainedTransformPreview(acknowledgement) => Some(
            GestureIdentity::ContainedTransform(acknowledgement.token().session()),
        ),
        RenderAction::Select(_)
        | RenderAction::CloseRequested(_)
        | RenderAction::CloseRootRequested(_)
        | RenderAction::ArmDrag(_)
        | RenderAction::BeginResize { .. }
        | RenderAction::AdjustResize { .. }
        | RenderAction::RaiseContained { .. }
        | RenderAction::BeginContainedTransform { .. } => None,
    }
}

fn collect_subtree_items(workspace: &Workspace, root: dockspace::ids::NodeId) -> Vec<ItemId> {
    let mut items = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match workspace.node(node) {
            Some(Node::Tabs {
                items: tab_items, ..
            }) => items.extend(tab_items.iter().copied()),
            Some(Node::Split { children, .. }) => stack.extend(children.iter().rev().copied()),
            None => {}
        }
    }
    items
}

fn complete_root_id(workspace: &Workspace, payload: &MovePayload) -> Option<RootId> {
    let root = match payload {
        MovePayload::Item(source) => source.root(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
    };
    let record = workspace.root(root)?;
    match payload {
        MovePayload::Item(_) => (record.central.is_none()
            && collect_subtree_items(workspace, record.node).len() == 1)
            .then_some(root),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            (source.node() == record.node).then_some(root)
        }
    }
}
