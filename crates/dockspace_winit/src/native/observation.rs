use dockspace::geometry::{GeometryError, PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::runtime::{NativeWindowFacts, NativeWindowPresentationState};
use winit::dpi::{PhysicalPosition, PhysicalSize as WindowSize};
use winit::window::Window;

/// Samples winit's current window geometry and visible/minimized observations.
///
/// `presentation_scale` is the renderer's actual pixels per logical point,
/// including application zoom. It is independent of the native DPI scale.
/// Missing desktop positions, visibility and zero-sized drawables remain unknown.
/// This does not infer input pass-through, create readiness, close state, focus,
/// usable monitor work areas or presentation from a platform command.
///
/// Capture and submit observations at the appropriate event-loop boundary;
/// sampling later cannot reconstruct an earlier pointer event's geometry.
///
/// # Errors
///
/// Returns a geometry error for invalid native scale or non-finite geometry.
pub fn observe_window(
    window: &Window,
    presentation_scale: ScaleFactor,
) -> Result<NativeWindowFacts, GeometryError> {
    facts(
        Sample {
            client_size: window.inner_size(),
            client_position: window.inner_position().ok(),
            outer_size: window.outer_size(),
            outer_position: window.outer_position().ok(),
            native_scale: window.scale_factor(),
            visible: window.is_visible(),
            minimized: window.is_minimized(),
        },
        presentation_scale,
    )
}

struct Sample {
    client_size: WindowSize<u32>,
    client_position: Option<PhysicalPosition<i32>>,
    outer_size: WindowSize<u32>,
    outer_position: Option<PhysicalPosition<i32>>,
    native_scale: f64,
    visible: Option<bool>,
    minimized: Option<bool>,
}

fn facts(
    sample: Sample,
    presentation_scale: ScaleFactor,
) -> Result<NativeWindowFacts, GeometryError> {
    let mut facts = NativeWindowFacts::live()
        .with_native_scale_factor(ScaleFactor::new(sample.native_scale)?)
        .with_presentation_scale_factor(presentation_scale);
    if sample.client_size.width > 0 && sample.client_size.height > 0 {
        facts = facts.with_content_size(PhysicalSize::new(
            f64::from(sample.client_size.width),
            f64::from(sample.client_size.height),
        )?);
        if let Some(position) = sample.client_position {
            facts = facts.with_content_bounds(rect(position, sample.client_size)?);
        }
    }
    if sample.outer_size.width > 0
        && sample.outer_size.height > 0
        && let Some(position) = sample.outer_position
    {
        facts = facts.with_outer_bounds(rect(position, sample.outer_size)?);
    }
    let presentation = match (sample.visible, sample.minimized) {
        (_, Some(true)) => Some(NativeWindowPresentationState::Minimized),
        (Some(false), _) => Some(NativeWindowPresentationState::Hidden),
        (Some(true), Some(false)) => Some(NativeWindowPresentationState::Visible),
        _ => None,
    };
    if let Some(presentation) = presentation {
        facts = facts.with_presentation(presentation);
    }
    Ok(facts)
}

fn rect(
    position: PhysicalPosition<i32>,
    size: WindowSize<u32>,
) -> Result<PhysicalRect, GeometryError> {
    PhysicalRect::new(
        f64::from(position.x),
        f64::from(position.y),
        f64::from(size.width),
        f64::from(size.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            client_size: WindowSize::new(800, 600),
            client_position: Some(PhysicalPosition::new(-780, 48)),
            outer_size: WindowSize::new(816, 636),
            outer_position: Some(PhysicalPosition::new(-788, 20)),
            native_scale: 2.0,
            visible: Some(true),
            minimized: Some(false),
        }
    }

    #[test]
    fn observations_preserve_physical_origins_and_independent_renderer_scale() {
        let native = ScaleFactor::new(2.0).unwrap();
        let renderer = ScaleFactor::new(2.5).unwrap();
        assert_eq!(
            facts(sample(), renderer).unwrap(),
            NativeWindowFacts::live()
                .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
                .with_content_bounds(PhysicalRect::new(-780.0, 48.0, 800.0, 600.0).unwrap())
                .with_outer_bounds(PhysicalRect::new(-788.0, 20.0, 816.0, 636.0).unwrap())
                .with_native_scale_factor(native)
                .with_presentation_scale_factor(renderer)
                .with_presentation(NativeWindowPresentationState::Visible)
        );
    }

    #[test]
    fn unavailable_desktop_positions_leave_client_size_available() {
        let mut observation = sample();
        observation.client_position = None;
        observation.outer_position = None;
        observation.visible = None;
        observation.minimized = None;
        let scale = ScaleFactor::new(2.0).unwrap();
        assert_eq!(
            facts(observation, scale).unwrap(),
            NativeWindowFacts::live()
                .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
                .with_native_scale_factor(scale)
                .with_presentation_scale_factor(scale)
        );
    }

    #[test]
    fn zero_sized_drawable_and_partial_visibility_do_not_manufacture_ready_geometry() {
        let mut observation = sample();
        observation.client_size = WindowSize::new(0, 600);
        observation.outer_size = WindowSize::new(816, 0);
        observation.minimized = None;
        let scale = ScaleFactor::new(2.0).unwrap();
        assert_eq!(
            facts(observation, scale).unwrap(),
            NativeWindowFacts::live()
                .with_native_scale_factor(scale)
                .with_presentation_scale_factor(scale)
        );
    }

    #[test]
    fn known_hidden_or_minimized_states_do_not_require_both_queries() {
        let scale = ScaleFactor::new(2.0).unwrap();
        for (visible, minimized, expected) in [
            (None, Some(true), NativeWindowPresentationState::Minimized),
            (Some(false), None, NativeWindowPresentationState::Hidden),
            (
                Some(false),
                Some(true),
                NativeWindowPresentationState::Minimized,
            ),
        ] {
            let mut observation = sample();
            observation.client_size = WindowSize::new(0, 0);
            observation.outer_size = WindowSize::new(0, 0);
            observation.visible = visible;
            observation.minimized = minimized;
            assert_eq!(
                facts(observation, scale).unwrap(),
                NativeWindowFacts::live()
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_presentation(expected)
            );
        }
    }

    #[test]
    fn invalid_native_dpi_is_rejected() {
        let mut observation = sample();
        observation.native_scale = f64::NAN;
        assert!(facts(observation, ScaleFactor::new(1.0).unwrap()).is_err());
    }
}
