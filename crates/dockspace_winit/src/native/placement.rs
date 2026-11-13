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
    #[error("requested outer bounds cannot contain the observed window decorations")]
    DecorationsExceedBounds,
}

/// Applies a core physical placement to initial winit window attributes.
///
/// Winit creates a client size before decoration extents can be observed. After
/// creation, call [`request_outer_bounds`] to compensate those actual extents.
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
/// unrepresentable or the observed decorations leave no positive client area.
pub fn request_outer_bounds(
    window: &Window,
    bounds: PhysicalRect,
) -> Result<Option<PhysicalSize<u32>>, WindowPlacementError> {
    let (position, outer) = placement(bounds)?;
    let client = client_size(outer, window.inner_size(), window.outer_size())?;
    window.set_outer_position(position);
    Ok(window.request_inner_size(client))
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
        _ => Err(WindowPlacementError::DecorationsExceedBounds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Err(WindowPlacementError::DecorationsExceedBounds)
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
