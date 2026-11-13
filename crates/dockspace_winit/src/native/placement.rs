use dockspace::geometry::PhysicalRect;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::{Window, WindowAttributes};

/// A requested physical placement cannot be represented by a winit window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WindowPlacementError {
    /// A rounded desktop position is outside the signed platform pixel range.
    #[error("window position exceeds the signed platform pixel range")]
    PositionOutOfRange,
    /// A rounded size is outside the positive platform pixel range.
    #[error("window size exceeds the positive platform pixel range")]
    SizeOutOfRange,
    /// The requested outer size leaves no client area after observed decorations.
    #[error(
        "requested outer size {requested:?} cannot contain decorations sampled from client {client:?} and outer {outer:?}"
    )]
    DecorationsExceedBounds {
        /// Requested physical outer size.
        requested: PhysicalSize<u32>,
        /// Observed physical client size.
        client: PhysicalSize<u32>,
        /// Observed physical outer size.
        outer: PhysicalSize<u32>,
    },
    /// The window changed size while its decoration extents were sampled.
    #[error("window size changed while sampling decorations: {before:?} -> {after:?}")]
    GeometryChanged {
        /// Client size before reading the outer size.
        before: PhysicalSize<u32>,
        /// Client size after reading the outer size.
        after: PhysicalSize<u32>,
    },
}

/// Applies a core physical placement to initial winit window attributes.
///
/// Winit creates a client size before decoration extents can be observed. After
/// the first window events, call [`request_outer_bounds`] to compensate observed
/// extents once the window manager has established its decorations.
/// Fractional physical pixels round to the nearest integer. Other application
/// attributes, including decorations, visibility and title, are preserved.
///
/// # Errors
///
/// Returns an error before changing attributes if the placement is outside
/// winit's integer position or positive size range.
pub fn window_attributes(
    attributes: WindowAttributes,
    bounds: PhysicalRect,
) -> Result<WindowAttributes, WindowPlacementError> {
    let (position, size) = placement(bounds)?;
    Ok(attributes.with_position(position).with_inner_size(size))
}

/// Requests physical outer bounds, compensating currently observed decorations.
///
/// The backend may constrain or asynchronously apply this request. The returned
/// optional client size is winit's immediate resize result, not a placement,
/// readiness or presentation acknowledgement. Report subsequent real geometry
/// through [`super::observe_window`]. Platform placement support remains a host
/// capability; this helper cannot add it to an unsupported window backend.
///
/// # Errors
///
/// Returns an error before issuing either command if the requested placement is
/// unrepresentable, geometry changed during sampling, or observed decorations
/// leave no positive client area. Retry `GeometryChanged` after processing the
/// next window events; do not busy-loop inside the event callback. In particular,
/// defer initial compensation until the window manager has handled creation.
pub fn request_outer_bounds(
    window: &Window,
    bounds: PhysicalRect,
) -> Result<Option<PhysicalSize<u32>>, WindowPlacementError> {
    let (position, outer) = placement(bounds)?;
    let before = window.inner_size();
    let observed_outer = window.outer_size();
    let after = window.inner_size();
    let client = stable_client_size(outer, before, observed_outer, after)?;
    window.set_outer_position(position);
    Ok(window.request_inner_size(client))
}

fn stable_client_size(
    requested: PhysicalSize<u32>,
    before: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
    after: PhysicalSize<u32>,
) -> Result<PhysicalSize<u32>, WindowPlacementError> {
    if before != after {
        return Err(WindowPlacementError::GeometryChanged { before, after });
    }
    client_size(requested, before, outer)
}

fn placement(
    bounds: PhysicalRect,
) -> Result<(PhysicalPosition<i32>, PhysicalSize<u32>), WindowPlacementError> {
    let coordinate = |value: f64| {
        let rounded = value.round();
        if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
            return Err(WindowPlacementError::PositionOutOfRange);
        }
        Ok(rounded as i32)
    };
    let extent = |value: f64| {
        let rounded = value.round();
        if rounded < 1.0 || rounded > f64::from(u32::MAX) {
            return Err(WindowPlacementError::SizeOutOfRange);
        }
        Ok(rounded as u32)
    };
    Ok((
        PhysicalPosition::new(coordinate(bounds.x())?, coordinate(bounds.y())?),
        PhysicalSize::new(extent(bounds.width())?, extent(bounds.height())?),
    ))
}

fn client_size(
    requested: PhysicalSize<u32>,
    client: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
) -> Result<PhysicalSize<u32>, WindowPlacementError> {
    let width = requested
        .width
        .checked_sub(outer.width.saturating_sub(client.width));
    let height = requested
        .height
        .checked_sub(outer.height.saturating_sub(client.height));
    match (width, height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Ok(PhysicalSize::new(width, height))
        }
        _ => Err(WindowPlacementError::DecorationsExceedBounds {
            requested,
            client,
            outer,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asynchronous_resize_cannot_be_mistaken_for_window_decorations() {
        let before = PhysicalSize::new(420, 300);
        let after = PhysicalSize::new(960, 640);
        assert_eq!(
            stable_client_size(
                PhysicalSize::new(420, 300),
                before,
                PhysicalSize::new(962, 665),
                after
            ),
            Err(WindowPlacementError::GeometryChanged { before, after }),
        );
        assert_eq!(
            stable_client_size(
                PhysicalSize::new(420, 300),
                after,
                PhysicalSize::new(962, 665),
                after
            ),
            Ok(PhysicalSize::new(418, 275)),
        );
    }

    #[test]
    fn outer_placement_compensates_real_physical_frame_extents() {
        assert_eq!(
            client_size(
                PhysicalSize::new(420, 300),
                PhysicalSize::new(600, 400),
                PhysicalSize::new(612, 436),
            ),
            Ok(PhysicalSize::new(408, 264))
        );
        assert_eq!(
            client_size(
                PhysicalSize::new(10, 20),
                PhysicalSize::new(600, 400),
                PhysicalSize::new(612, 436),
            ),
            Err(WindowPlacementError::DecorationsExceedBounds {
                requested: PhysicalSize::new(10, 20),
                client: PhysicalSize::new(600, 400),
                outer: PhysicalSize::new(612, 436),
            })
        );
    }

    #[test]
    fn initial_attributes_preserve_application_choices_and_physical_units() {
        use winit::dpi::{Position, Size};
        use winit::window::WindowLevel;

        let attributes = window_attributes(
            Window::default_attributes()
                .with_title("Application palette")
                .with_decorations(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop),
            PhysicalRect::new(-320.4, 40.6, 420.4, 299.6).unwrap(),
        )
        .unwrap();
        assert_eq!(
            attributes.position,
            Some(Position::Physical(PhysicalPosition::new(-320, 41)))
        );
        assert_eq!(
            attributes.inner_size,
            Some(Size::Physical(PhysicalSize::new(420, 300)))
        );
        assert_eq!(attributes.title, "Application palette");
        assert!(!attributes.visible);
        assert!(!attributes.decorations);
        assert_eq!(attributes.window_level, WindowLevel::AlwaysOnTop);
    }

    #[test]
    fn unrepresentable_placements_are_rejected_without_saturation() {
        for (bounds, expected) in [
            (
                (f64::from(i32::MAX) + 1.0, 0.0, 10.0, 10.0),
                WindowPlacementError::PositionOutOfRange,
            ),
            (
                (0.0, f64::from(i32::MIN) - 1.0, 10.0, 10.0),
                WindowPlacementError::PositionOutOfRange,
            ),
            (
                (0.0, 0.0, f64::from(u32::MAX) + 1.0, 10.0),
                WindowPlacementError::SizeOutOfRange,
            ),
            ((0.0, 0.0, 0.49, 10.0), WindowPlacementError::SizeOutOfRange),
            ((0.0, 0.0, 10.0, 0.0), WindowPlacementError::SizeOutOfRange),
        ] {
            let (x, y, width, height) = bounds;
            assert_eq!(
                window_attributes(
                    Window::default_attributes(),
                    PhysicalRect::new(x, y, width, height).unwrap()
                )
                .unwrap_err(),
                expected
            );
        }
    }
}
