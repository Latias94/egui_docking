use std::collections::BTreeMap;

use dockspace::geometry::LogicalRect;
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::validation::{PresentationLocation, WorkspaceValidationError};

fn logical_rect() -> LogicalRect {
    LogicalRect::new(12.0, 18.0, 320.0, 240.0).expect("test rectangle is finite and non-negative")
}

fn contains_error(
    errors: &[WorkspaceValidationError],
    predicate: impl Fn(&WorkspaceValidationError) -> bool,
) -> bool {
    errors.iter().any(predicate)
}

#[test]
fn valid_workspace_preserves_items_and_unique_presentations() {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([]));
    let editor = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let main_node = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [central, editor]).expect("valid split"));
    let tools_node = builder.insert_node(Node::tabs([ItemId::new(3)]));

    let main_root = RootId::new(10);
    let tools_root = RootId::new(11);
    let surface = SurfaceId::new(20);
    let floating = FloatingPresentationId::new(30);
    builder.set_root(main_root, RootRecord::new(main_node).with_central(central));
    builder.set_root(tools_root, RootRecord::new(tools_node));
    builder.set_surface(surface, SurfacePresentation::with_main(main_root));
    builder.set_contained_floating(floating, ContainedFloating::new(tools_root, logical_rect()));
    builder
        .attach_contained(surface, floating)
        .expect("surface exists");

    let workspace = builder.build().expect("workspace is valid and canonical");
    assert_eq!(
        workspace.item_multiset(),
        BTreeMap::from([
            (ItemId::new(1), 1),
            (ItemId::new(2), 1),
            (ItemId::new(3), 1),
        ])
    );
    assert_eq!(workspace.roots().count(), 2);
    assert_eq!(workspace.surfaces().count(), 1);
    assert_eq!(workspace.contained_floatings().count(), 1);
    workspace
        .validate()
        .expect("published workspace remains valid");
}

#[test]
fn strict_validation_rejects_non_positive_contained_geometry() {
    let mut builder = Workspace::builder();
    let floating_node = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let floating_root = RootId::new(1);
    let surface = SurfaceId::new(1);
    let floating = FloatingPresentationId::new(1);
    builder.set_root(floating_root, RootRecord::new(floating_node));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            floating_root,
            LogicalRect::new(12.0, 18.0, 0.0, 240.0)
                .expect("zero-width logical rectangles are representable drafts"),
        ),
    );
    builder
        .attach_contained(surface, floating)
        .expect("surface exists");

    let errors = builder
        .validate()
        .expect_err("zero-area contained geometry must fail strict validation")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::NonPositiveContainedRect {
            floating: id,
            width,
            height,
        } if *id == floating
            && width.to_bits() == 0.0_f64.to_bits()
            && height.to_bits() == 240.0_f64.to_bits()
    )));
}

#[test]
fn strict_validation_reports_cycle_shared_node_and_orphan() {
    let mut builder = Workspace::builder();
    let cycle_entry = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let second_leaf = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [cycle_entry, second_leaf])
            .expect("valid initial split"),
    );
    builder
        .replace_node(
            cycle_entry,
            Node::equal_split(Axis::Vertical, [split, second_leaf])
                .expect("valid cyclic node shape"),
        )
        .expect("cycle entry exists");
    let orphan = builder.insert_node(Node::tabs_with_selection([], None));
    let root = RootId::new(1);
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));

    let errors = builder
        .validate()
        .expect_err("corrupted graph must fail strict validation")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::Cycle { .. }
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::SharedNode { .. }
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::OrphanNode { node } if *node == orphan
    )));
}

#[test]
fn strict_validation_rejects_weight_and_canonical_shape_corruption() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let middle = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let nested = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![left, middle],
        weights: vec![
            SplitWeight::new(0.25).expect("positive weight"),
            SplitWeight::new(0.25).expect("positive weight"),
        ],
    });
    let right = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [nested, right]).expect("valid outer split"),
    );
    let root = RootId::new(1);
    builder.set_root(root, RootRecord::new(root_node));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));

    let errors = builder
        .validate()
        .expect_err("non-canonical graph must fail strict validation")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::SplitWeightsNotNormalized { split, .. } if *split == nested
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::SameAxisNesting { child, .. } if *child == nested
    )));
    assert!(SplitWeight::new(0.0).is_err());
    assert!(SplitWeight::new(f32::NAN).is_err());
    assert!(
        SplitWeight::normalize([f32::MIN_POSITIVE, f32::MAX]).is_err(),
        "normalization must not underflow a positive runtime weight to zero"
    );
}

#[test]
fn strict_validation_rejects_duplicate_items_and_invalid_selection() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(1)],
        Some(ItemId::new(99)),
    ));
    let right = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("valid split shape"),
    );
    let root = RootId::new(1);
    builder.set_root(root, RootRecord::new(root_node));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));

    let errors = builder
        .validate()
        .expect_err("item and selection corruption must fail")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::InvalidSelection { tabs, .. } if *tabs == left
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::DuplicateItem { item, .. } if *item == ItemId::new(1)
    )));
}

#[test]
fn strict_validation_enforces_root_presentation_and_backlinks() {
    let mut builder = Workspace::builder();
    let main_node = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let floating_node = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let main_root = RootId::new(1);
    let floating_root = RootId::new(2);
    let first_surface = SurfaceId::new(1);
    let second_surface = SurfaceId::new(2);
    let floating = FloatingPresentationId::new(1);
    builder.set_root(main_root, RootRecord::new(main_node));
    builder.set_root(floating_root, RootRecord::new(floating_node));
    builder.set_surface(first_surface, SurfacePresentation::with_main(main_root));
    builder.set_surface(second_surface, SurfacePresentation::with_main(main_root));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(floating_root, logical_rect()),
    );

    let errors = builder
        .validate()
        .expect_err("presentation corruption must fail")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::RootPresentedMoreThanOnce { root, .. } if *root == main_root
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::ContainedNotPresented { floating: id } if *id == floating
    )));
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::RootNotPresented { root } if *root == floating_root
    )));
}

#[test]
fn strict_validation_preserves_the_first_owner_across_three_presentations() {
    let mut builder = Workspace::builder();
    let main_node = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let floating_node = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let main_root = RootId::new(1);
    let floating_root = RootId::new(2);
    let floating = FloatingPresentationId::new(10);
    builder.set_root(main_root, RootRecord::new(main_node));
    builder.set_root(floating_root, RootRecord::new(floating_node));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(floating_root, logical_rect()),
    );

    for surface in [SurfaceId::new(1), SurfaceId::new(2), SurfaceId::new(3)] {
        builder.set_surface(
            surface,
            SurfacePresentation {
                main_root: Some(main_root),
                contained: vec![floating],
            },
        );
    }

    let errors = builder
        .validate()
        .expect_err("three presentation owners must fail")
        .into_errors();
    let duplicate_main_owners: Vec<_> = errors
        .iter()
        .filter_map(|error| match error {
            WorkspaceValidationError::RootPresentedMoreThanOnce {
                root,
                first,
                duplicate,
            } if *root == main_root => Some((*first, *duplicate)),
            _ => None,
        })
        .collect();
    assert_eq!(
        duplicate_main_owners,
        [
            (
                PresentationLocation::Main(SurfaceId::new(1)),
                PresentationLocation::Main(SurfaceId::new(2)),
            ),
            (
                PresentationLocation::Main(SurfaceId::new(1)),
                PresentationLocation::Main(SurfaceId::new(3)),
            ),
        ]
    );

    let duplicate_floating_owners: Vec<_> = errors
        .iter()
        .filter_map(|error| match error {
            WorkspaceValidationError::ContainedPresentedMoreThanOnce {
                floating: id,
                first_surface,
                duplicate_surface,
            } if *id == floating => Some((*first_surface, *duplicate_surface)),
            _ => None,
        })
        .collect();
    assert_eq!(
        duplicate_floating_owners,
        [
            (SurfaceId::new(1), SurfaceId::new(2)),
            (SurfaceId::new(1), SurfaceId::new(3)),
        ]
    );
}

#[test]
fn central_region_must_be_a_reachable_tabs_leaf() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("valid split"));
    let root = RootId::new(1);
    builder.set_root(root, RootRecord::new(split).with_central(split));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));

    let errors = builder
        .validate()
        .expect_err("a split cannot be central")
        .into_errors();
    assert!(contains_error(&errors, |error| matches!(
        error,
        WorkspaceValidationError::CentralNotTabs { central, .. } if *central == split
    )));
}
