//! Exact conversion from fork-owned window snapshots into core native facts.

use dockspace::geometry::{PhysicalRect, ScaleFactor};
use dockspace::runtime::{NativeSurfaceBinding, NativeWindowFacts, NativeWindowPresentationState};
use dockspace::runtime::NativePresentationEffectAcknowledgement;
use eframe::{NativePhysicalRect, NativeWindowSnapshot};

use crate::error::NativeHostProtocolError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CompiledWindowObservation {
    binding: NativeSurfaceBinding,
    facts: NativeWindowFacts,
    presentation_acknowledged: bool,
}

impl CompiledWindowObservation {
    pub(crate) const fn new(binding: NativeSurfaceBinding, facts: NativeWindowFacts) -> Self {
        Self {
            binding,
            facts,
            presentation_acknowledged: false,
        }
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn facts(self) -> NativeWindowFacts {
        self.facts
    }

    pub(crate) const fn presentation_acknowledged(self) -> bool {
        self.presentation_acknowledged
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SnapshotParts {
    inner_rect: Option<NativePhysicalRect>,
    outer_rect: Option<NativePhysicalRect>,
    native_scale_factor: f64,
    presentation_scale_factor: f64,
    visible: Option<bool>,
    minimized: Option<bool>,
}

impl From<NativeWindowSnapshot> for SnapshotParts {
    fn from(snapshot: NativeWindowSnapshot) -> Self {
        Self {
            inner_rect: snapshot.inner_rect(),
            outer_rect: snapshot.outer_rect(),
            native_scale_factor: snapshot.native_scale_factor(),
            presentation_scale_factor: snapshot.presentation_scale_factor(),
            visible: snapshot.visible(),
            minimized: snapshot.minimized(),
        }
    }
}

pub(crate) fn compile_window_observation(
    binding: NativeSurfaceBinding,
    snapshot: NativeWindowSnapshot,
    acknowledgement: Option<NativePresentationEffectAcknowledgement>,
) -> Result<CompiledWindowObservation, NativeHostProtocolError> {
    compile_window_facts(snapshot.into(), acknowledgement).map(|facts| CompiledWindowObservation {
        binding,
        facts,
        presentation_acknowledged: acknowledgement.is_some(),
    })
}

fn compile_window_facts(
    snapshot: SnapshotParts,
    acknowledgement: Option<NativePresentationEffectAcknowledgement>,
) -> Result<NativeWindowFacts, NativeHostProtocolError> {
    let native_scale_factor = ScaleFactor::new(snapshot.native_scale_factor)
        .map_err(|_| NativeHostProtocolError::InvalidWindowSnapshot)?;
    let presentation_scale_factor = ScaleFactor::new(snapshot.presentation_scale_factor)
        .map_err(|_| NativeHostProtocolError::InvalidWindowSnapshot)?;
    let mut facts = NativeWindowFacts::live()
        .with_native_scale_factor(native_scale_factor)
        .with_presentation_scale_factor(presentation_scale_factor);
    if let Some(rect) = snapshot.inner_rect {
        facts = facts.with_content_bounds(physical_rect(rect)?);
    }
    if let Some(rect) = snapshot.outer_rect {
        facts = facts.with_outer_bounds(physical_rect(rect)?);
    }
    if let Some(presentation) = presentation_state(snapshot.visible, snapshot.minimized) {
        facts = facts.with_presentation(presentation, acknowledgement);
    } else if acknowledgement.is_some() {
        return Err(NativeHostProtocolError::PresentationAcknowledgementWithoutState);
    }
    Ok(facts)
}

fn physical_rect(rect: NativePhysicalRect) -> Result<PhysicalRect, NativeHostProtocolError> {
    PhysicalRect::new(
        f64::from(rect.x()),
        f64::from(rect.y()),
        f64::from(rect.width()),
        f64::from(rect.height()),
    )
    .map_err(|_| NativeHostProtocolError::InvalidWindowSnapshot)
}

const fn presentation_state(
    visible: Option<bool>,
    minimized: Option<bool>,
) -> Option<NativeWindowPresentationState> {
    match minimized {
        Some(true) => Some(NativeWindowPresentationState::Minimized),
        Some(false) | None => match visible {
            Some(true) => Some(NativeWindowPresentationState::Visible),
            Some(false) => Some(NativeWindowPresentationState::Hidden),
            None => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostWindowToken, NativePointerRoster, SurfaceUnavailableReason,
    };

    use super::*;

    fn binding() -> NativeSurfaceBinding {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("managed native host enrolls");
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .expect("root registration records");
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("registration frame settles the surface");
        let report = frame.commit().expect("registration frame commits");
        report
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                dockspace::runtime::HostInputOutcome::NativeSurfaceRegistered { binding } => {
                    Some(*binding)
                }
                _ => None,
            })
            .expect("registration emits one binding")
    }

    fn parts() -> SnapshotParts {
        SnapshotParts {
            inner_rect: Some(NativePhysicalRect::new(10, 20, 800, 600)),
            outer_rect: Some(NativePhysicalRect::new(2, -10, 816, 638)),
            native_scale_factor: 2.0,
            presentation_scale_factor: 2.5,
            visible: Some(true),
            minimized: Some(false),
        }
    }

    #[test]
    fn exact_snapshot_fields_compile_without_inference() {
        let binding = binding();
        let expected = NativeWindowFacts::live()
            .with_content_bounds(
                PhysicalRect::new(10.0, 20.0, 800.0, 600.0).expect("content rect validates"),
            )
            .with_outer_bounds(
                PhysicalRect::new(2.0, -10.0, 816.0, 638.0).expect("outer rect validates"),
            )
            .with_native_scale_factor(ScaleFactor::new(2.0).expect("scale validates"))
            .with_presentation_scale_factor(ScaleFactor::new(2.5).expect("scale validates"))
            .with_presentation(NativeWindowPresentationState::Visible, None);
        let observation = CompiledWindowObservation::new(
            binding,
            compile_window_facts(parts(), None).expect("snapshot compiles"),
        );

        assert_eq!(observation.binding(), binding);
        assert_eq!(observation.facts(), expected);
    }

    #[test]
    fn missing_snapshot_fields_remain_unknown() {
        let facts = compile_window_facts(SnapshotParts {
            inner_rect: None,
            outer_rect: None,
            native_scale_factor: 1.5,
            presentation_scale_factor: 1.25,
            visible: None,
            minimized: None,
        }, None)
        .expect("partial snapshot compiles");

        assert_eq!(
            facts,
            NativeWindowFacts::live()
                .with_native_scale_factor(ScaleFactor::new(1.5).expect("scale validates"))
                .with_presentation_scale_factor(
                    ScaleFactor::new(1.25).expect("scale validates"),
                )
        );
    }

    #[test]
    fn minimized_state_precedes_visibility() {
        let mut snapshot = parts();
        snapshot.minimized = Some(true);
        let facts = compile_window_facts(snapshot, None).expect("minimized snapshot compiles");
        let expected = NativeWindowFacts::live()
            .with_content_bounds(
                PhysicalRect::new(10.0, 20.0, 800.0, 600.0).expect("content rect validates"),
            )
            .with_outer_bounds(
                PhysicalRect::new(2.0, -10.0, 816.0, 638.0).expect("outer rect validates"),
            )
            .with_native_scale_factor(ScaleFactor::new(2.0).expect("scale validates"))
            .with_presentation_scale_factor(ScaleFactor::new(2.5).expect("scale validates"))
            .with_presentation(NativeWindowPresentationState::Minimized, None);

        assert_eq!(facts, expected);
    }

    #[test]
    fn invalid_scale_is_rejected_without_fallback() {
        for scale in [0.0, f64::NAN, f64::INFINITY] {
            let mut snapshot = parts();
            snapshot.native_scale_factor = scale;
            assert_eq!(
                compile_window_facts(snapshot, None),
                Err(NativeHostProtocolError::InvalidWindowSnapshot)
            );

            let mut snapshot = parts();
            snapshot.presentation_scale_factor = scale;
            assert_eq!(
                compile_window_facts(snapshot, None),
                Err(NativeHostProtocolError::InvalidWindowSnapshot)
            );
        }
    }
}
