use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use egui::{Context, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    ContainedPresentationIds, Dockspace, DockspaceCapability, DockspaceUnavailableReason, PaneView,
    TearOffMode,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM: ItemId = ItemId::new(3);

struct TestPane;

impl PaneView for TestPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Document".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
    builder.build().expect("fixture workspace is valid")
}

fn capability(dockspace: &mut Dockspace, context: &Context) -> DockspaceCapability {
    let mut pane = TestPane;
    let mut observed = None;
    let _ = context.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
            ..RawInput::default()
        },
        |ui| {
            observed = Some(
                dockspace
                    .show(SURFACE, ui, &mut pane)
                    .expect("fixture frame advances")
                    .contained_capability(),
            );
        },
    );
    observed.expect("egui executes one or more passes")
}

#[test]
fn default_mode_explicitly_disables_contained_tear_off() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("disabled-tear-off", workspace())
        .build()
        .expect("fixture facade builds");

    assert_eq!(
        capability(&mut dockspace, &context),
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::TearOffModeDisabled)
    );
}

#[test]
fn contained_mode_reports_each_missing_prerequisite() {
    let context = Context::default();
    let mut missing_source = Dockspace::builder("missing-source", workspace())
        .tear_off_mode(TearOffMode::Contained)
        .build()
        .expect("fixture facade builds");
    assert_eq!(
        capability(&mut missing_source, &context),
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::PresentationIdSourceMissing)
    );

    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut policy_disabled = Dockspace::builder("policy-disabled", workspace())
        .policy(policy)
        .tear_off_mode(TearOffMode::Contained)
        .presentation_ids(|| {
            Some(ContainedPresentationIds::new(
                RootId::new(10),
                FloatingPresentationId::new(11),
            ))
        })
        .build()
        .expect("fixture facade builds");
    assert_eq!(
        capability(&mut policy_disabled, &context),
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::ContainedPolicyDisabled)
    );
}

#[test]
fn contained_mode_is_supported_only_after_a_ready_scene_exists() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("contained-tear-off", workspace())
        .tear_off_mode(TearOffMode::Contained)
        .presentation_ids(|| {
            Some(ContainedPresentationIds::new(
                RootId::new(10),
                FloatingPresentationId::new(11),
            ))
        })
        .build()
        .expect("fixture facade builds");

    assert_eq!(
        capability(&mut dockspace, &context),
        DockspaceCapability::Supported
    );
}
