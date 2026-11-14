//! Completed renderer output setup shared by native input regressions.

use dockspace::runtime::{NativeSurfaceBinding, PaintedSurfaceOutput};
use eframe::egui;

use crate::host_frame::NativeSurfacePaint;

use super::{
    NativeReceiverPass, NativeReceiverStageError, NativeReceiverStore, PendingReceiverSet,
};

/// Records a real completed pass without promoting its receivers before core commit.
pub(crate) fn present_completed_output(
    store: &mut NativeReceiverStore,
    context: &egui::Context,
    pixels_per_point: f32,
    binding: NativeSurfaceBinding,
    output: &PaintedSurfaceOutput,
    paint: &NativeSurfacePaint,
) -> Result<(), NativeReceiverStageError> {
    let viewport = paint.viewport_id();
    let pass = NativeReceiverPass::capture(context, viewport, pixels_per_point)?;
    let pending = PendingReceiverSet::from_paint(&pass, binding, output, paint)?;
    store.finish_renderer_presented(Some(pending), viewport, binding);
    Ok(())
}
