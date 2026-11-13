//! Custom guide drawing retains canonical input and accessibility behavior.

use std::collections::HashMap;

use super::*;
use crate::{DockspaceDropEligibility, DockspaceGuideScope, DropGuide};

#[derive(Clone, Copy)]
enum GuideDrawing {
    Hidden,
    Replacement,
    ForeignLayer,
}

struct CustomPanes {
    panes: MagneticPanes,
    guides: HashMap<egui::Id, DropGuide>,
    drawing: GuideDrawing,
    #[cfg(feature = "motion")]
    preview: Option<(Rect, Rect)>,
}

impl PaneView for CustomPanes {
    #[cfg(feature = "motion")]
    fn drop_preview_ui(&mut self, ui: &Ui, preview: &crate::DropPreview) {
        self.preview = Some((preview.rect, preview.presentation_rect));
        preview.paint(ui);
    }
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        self.panes.ui(item, ui)
    }

    fn drop_guide_ui(&mut self, ui: &Ui, guide: &DropGuide) {
        assert!(guide.rect.contains_rect(ui.clip_rect()));
        self.guides.insert(guide.id, *guide);
        match self.drawing {
            GuideDrawing::Hidden => {}
            GuideDrawing::Replacement => {
                ui.painter().circle_filled(
                    guide.rect.center(),
                    guide.rect.width().min(guide.rect.height()) * 0.4,
                    egui::Color32::GOLD,
                );
            }
            GuideDrawing::ForeignLayer => {
                ui.ctx()
                    .layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        guide.id.with("foreign-layer"),
                    ))
                    .circle_filled(guide.rect.center(), 3.0, egui::Color32::RED);
            }
        }
    }
}

fn begin_drag(context: &Context, dock: &mut Dockspace, panes: &mut Box<CustomPanes>) -> Pos2 {
    let (source, target) = settle_until(8, || {
        let frame = run_frame(context, dock, panes, Vec::new());
        try_tab_rect(&frame.output, "Second")
            .map(|rect| rect.center())
            .zip(panes.panes.target_rect.get().map(|rect| rect.center()))
    });
    run_frame(
        context,
        dock,
        panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    run_frame(
        context,
        dock,
        panes,
        vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
    );
    target
}

fn custom_panes(drawing: GuideDrawing) -> Box<CustomPanes> {
    Box::new(CustomPanes {
        panes: MagneticPanes::new(),
        guides: HashMap::new(),
        drawing,
        #[cfg(feature = "motion")]
        preview: None,
    })
}

#[cfg(feature = "motion")]
#[test]
fn animated_drop_decoration_retains_canonical_candidate_and_reduced_motion_settles_it() {
    use crate::motion::MotionPreference;

    let context = Context::default();
    context.enable_accesskit();
    let mut dock = Dockspace::builder("animated-preview", magnetic_layout())
        .build()
        .unwrap();
    dock.set_motion_preference(MotionPreference::Full);
    let mut panes = custom_panes(GuideDrawing::Hidden);
    let target = begin_drag(&context, &mut dock, &mut panes);
    let feedback = settle_until(8, || {
        let frame = run_frame(
            &context,
            &mut dock,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        (!frame.drop_targets.is_empty()).then_some(frame)
    });
    let center_id = feedback
        .drop_targets
        .iter()
        .find(|target| target.direction == DockspaceDropDirection::Center)
        .unwrap()
        .widget_id;
    let center = panes.guides[&center_id].rect.center();
    let right_id = feedback
        .drop_targets
        .iter()
        .find(|target| target.direction == DockspaceDropDirection::Right)
        .unwrap()
        .widget_id;
    let right = panes.guides[&right_id].rect.center();
    for _ in 0..16 {
        run_frame(
            &context,
            &mut dock,
            &mut panes,
            vec![Event::PointerMoved(center)],
        );
    }
    let before = panes.preview.unwrap().0;
    panes.preview = None;
    let moved = run_frame(
        &context,
        &mut dock,
        &mut panes,
        vec![Event::PointerMoved(right)],
    );
    let (candidate, presentation) = panes
        .preview
        .expect("the new semantic target paints immediately");
    assert_ne!(candidate, before);
    assert_ne!(
        presentation, candidate,
        "only decoration follows the target over time"
    );
    dock.set_motion_preference(MotionPreference::Reduced);
    panes.preview = None;
    let reduced = run_frame(
        &context,
        &mut dock,
        &mut panes,
        vec![Event::PointerMoved(right)],
    );
    let (candidate, presentation) = panes.preview.unwrap();
    assert_eq!(candidate, presentation);
    assert_eq!(moved.drop_targets.len(), reduced.drop_targets.len());
}

#[test]
fn boxed_custom_guides_keep_stable_receivers_accessibility_and_docking() {
    for (case, drawing) in [GuideDrawing::Hidden, GuideDrawing::Replacement]
        .into_iter()
        .enumerate()
    {
        let context = Context::default();
        context.enable_accesskit();
        let mut policy = dockspace::policy::DockPolicy::default();
        policy.set_allow_edge_split(false);
        let mut dock = Dockspace::builder(("custom-guides", case), magnetic_layout())
            .policy(policy)
            .build()
            .unwrap();
        let mut panes = custom_panes(drawing);
        let target = begin_drag(&context, &mut dock, &mut panes);
        let preview = settle_until(8, || {
            let frame = run_frame(
                &context,
                &mut dock,
                &mut panes,
                vec![Event::PointerMoved(target)],
            );
            (!frame.drop_targets.is_empty()).then_some(frame)
        });
        assert!(
            panes.guides.values().any(|guide| {
                guide.eligibility == DockspaceDropEligibility::Rejected
                    && guide.direction != DockspaceDropDirection::Center
            }),
            "rejected targets also reach the custom painter",
        );
        let center = preview
            .drop_targets
            .iter()
            .find(|target| target.direction == DockspaceDropDirection::Center)
            .unwrap();
        let guide = panes.guides[&center.widget_id];
        assert_eq!(guide.scope, DockspaceGuideScope::Inner);
        assert_eq!(guide.root, ROOT);
        assert_eq!(guide.eligibility, DockspaceDropEligibility::Eligible);
        assert!(guide.rect.contains_rect(guide.cue_rect));
        assert!(guide.cue_rect.contains_rect(guide.emphasis_rect));
        let labels = preview
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        assert!(labels.nodes.iter().any(|(_, node)| {
            node.role() == Role::Label && node.label() == Some("Dockspace center drop guide")
        }));
        for receiver in &preview.drop_targets {
            let painted = panes.guides[&receiver.widget_id];
            assert_eq!(painted.direction, receiver.direction);
            assert_eq!(painted.root, ROOT);
        }
        let point = guide.rect.center();
        run_frame(
            &context,
            &mut dock,
            &mut panes,
            vec![Event::PointerMoved(point)],
        );
        assert_eq!(panes.guides[&guide.id].rect, guide.rect);
        assert!(panes.guides[&guide.id].active);
        run_frame(
            &context,
            &mut dock,
            &mut panes,
            vec![Event::PointerMoved(point), pointer_button(point, false)],
        );
        settle_until(8, || {
            run_frame(&context, &mut dock, &mut panes, Vec::new());
            dock.view()
                .item(FIRST)
                .unwrap()
                .tabs()
                .items()
                .contains(&SECOND)
                .then_some(())
        });
        assert!(!dock.session.has_active_gesture());
    }
}

#[test]
fn guide_callback_cannot_present_pixels_on_a_foreign_layer() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dock = Dockspace::builder("foreign-guide-output", magnetic_layout())
        .build()
        .unwrap();
    let mut panes = custom_panes(GuideDrawing::ForeignLayer);
    let target = begin_drag(&context, &mut dock, &mut panes);
    let version = dock.version();
    let mut rejected = false;
    for _ in 0..3 {
        let frame = run_frame(
            &context,
            &mut dock,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        rejected |= frame.presentation_capability
            == crate::DockspaceCapability::Unavailable(
                crate::DockspaceUnavailableReason::PresentationOutputMismatch,
            );
    }
    assert!(!panes.guides.is_empty());
    assert!(
        rejected,
        "cross-layer guide drawing cannot authorize the new output"
    );
    assert_eq!(dock.version(), version);
}
