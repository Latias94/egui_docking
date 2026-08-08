use crate::geometry::LogicalRect;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceLayoutError, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout,
};

const MAIN_SURFACE: SurfaceId = SurfaceId::new(10);
const ROOTLESS_SURFACE: SurfaceId = SurfaceId::new(20);
const MAIN_ROOT: RootId = RootId::new(100);
const CONTAINED_ROOT: RootId = RootId::new(200);
const ROOTLESS_CONTAINED_ROOT: RootId = RootId::new(300);
const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(1_000);
const ROOTLESS_CONTAINED: FloatingPresentationId = FloatingPresentationId::new(2_000);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

#[test]
fn product_layout_builds_central_nary_contained_and_rootless_topology() {
    let main = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([ItemId::new(1)]),
            DockspaceNode::central_tabs([ItemId::new(2), ItemId::new(3)]),
            DockspaceNode::tabs([ItemId::new(4)]),
        ],
    )
    .expect("three children form a valid N-ary split");
    let contained = DockspaceContainedLayout::new(
        CONTAINED,
        DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([ItemId::new(5)])),
        rect(30.0, 40.0, 320.0, 180.0),
    );
    let rooted_surface =
        DockspaceSurfaceLayout::new(MAIN_SURFACE, DockspaceRootLayout::new(MAIN_ROOT, main))
            .with_contained(contained);

    let rootless_contained = DockspaceContainedLayout::new(
        ROOTLESS_CONTAINED,
        DockspaceRootLayout::new(ROOTLESS_CONTAINED_ROOT, DockspaceNode::central_tabs([])),
        rect(5.0, 8.0, 120.0, 90.0),
    );
    let rootless_surface =
        DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(rootless_contained);

    let layout = DockspaceLayout::new([rooted_surface, rootless_surface])
        .expect("product declaration must compile into a validated workspace");
    let semantic_debug = format!("{layout:#?}");
    assert!(!semantic_debug.contains("NodeId"));
    assert!(!semantic_debug.contains("Workspace {"));
    let view = layout.view();

    let rooted = view
        .surface(MAIN_SURFACE)
        .expect("rooted surface must be queryable");
    assert!(!rooted.is_rootless());
    assert_eq!(rooted.contained_count(), 1);
    let main_root = rooted.main_root().expect("main root must be queryable");
    assert_eq!(main_root.id(), MAIN_ROOT);
    let split = main_root
        .content()
        .and_then(|node| node.split())
        .expect("main root must retain its N-ary split");
    assert_eq!(split.axis(), DockspaceAxis::Horizontal);
    assert_eq!(split.child_count(), 3);
    let weights = split.weights().collect::<Vec<_>>();
    assert_eq!(weights.len(), 3);
    assert!(
        weights
            .iter()
            .all(|weight| (*weight - (1.0 / 3.0)).abs() < 1.0e-6)
    );
    assert!((weights.iter().sum::<f32>() - 1.0).abs() <= f32::EPSILON * 2.0);

    let children = split.children().collect::<Vec<_>>();
    assert_eq!(children.len(), 3);
    assert!(children[1].is_central());
    let central = main_root
        .central()
        .and_then(|node| node.tabs())
        .expect("central tabs must be queryable without a runtime node ID");
    assert_eq!(central.items(), &[ItemId::new(2), ItemId::new(3)]);
    assert_eq!(central.selected(), Some(ItemId::new(2)));

    let floating = rooted
        .contained()
        .next()
        .expect("contained root must preserve roster order");
    assert_eq!(floating.id(), CONTAINED);
    assert_eq!(floating.rect(), rect(30.0, 40.0, 320.0, 180.0));
    assert_eq!(floating.root().map(|root| root.id()), Some(CONTAINED_ROOT));

    let rootless = view
        .surface(ROOTLESS_SURFACE)
        .expect("rootless surface must be queryable");
    assert!(rootless.is_rootless());
    assert!(rootless.main_root().is_none());
    assert_eq!(rootless.contained_count(), 1);
    let empty_central = rootless
        .contained()
        .next()
        .and_then(|floating| floating.root())
        .and_then(|root| root.central())
        .and_then(|node| node.tabs())
        .expect("an empty central leaf remains a valid product view");
    assert!(empty_central.items().is_empty());
    assert_eq!(empty_central.selected(), None);
}

#[test]
fn product_layout_rejects_invalid_stable_declarations() {
    let duplicate_item = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(
            MAIN_ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Vertical,
                [
                    DockspaceNode::tabs([ItemId::new(7)]),
                    DockspaceNode::tabs([ItemId::new(7)]),
                ],
            )
            .expect("the split shape itself is valid"),
        ),
    )])
    .expect_err("one stable item cannot appear in two leaves");
    assert_eq!(
        duplicate_item,
        DockspaceLayoutError::DuplicateItem {
            item: ItemId::new(7)
        }
    );

    let empty_rootless = DockspaceLayout::new([DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE)])
        .expect_err("a rootless surface needs contained content");
    assert_eq!(
        empty_rootless,
        DockspaceLayoutError::EmptyRootlessSurface {
            surface: ROOTLESS_SURFACE
        }
    );
}
