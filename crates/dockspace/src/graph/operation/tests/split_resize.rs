use super::*;

#[test]
fn a_resize_frontier_validates_each_root_once_without_lending_its_source_proof() {
    const SPLITS: usize = 64;
    let mut builder = Workspace::builder();
    let mut subtree = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let mut nodes = Vec::new();
    for index in 0..SPLITS {
        let peer = builder.insert_node(Node::tabs([ItemId::new(index as u64 + 2)]));
        let axis = if index % 2 == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        subtree = builder.insert_node(Node::equal_split(axis, [peer, subtree]).unwrap());
        nodes.push(subtree);
    }
    builder.set_root(MAIN, RootRecord::new(subtree));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
    let foreign = builder.insert_node(Node::tabs([ItemId::new(1000)]));
    builder.set_root(SURVIVOR_ROOT, RootRecord::new(foreign));
    builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
    let mut workspace = builder.build().unwrap();
    let original = workspace.capture_node_source(MAIN, subtree).unwrap();
    let splits = nodes
        .iter()
        .map(|&node| {
            SplitResize::new(
                NodeSource {
                    root: MAIN,
                    node,
                    reference: original.reference().clone(),
                },
                SplitWeight::normalize([0.4, 0.6]).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    crate::interaction::targeting::resolver::structural_work::reset();
    let validated = validate_split_resizes_structure(&workspace, &splits).unwrap();
    assert_eq!(validated.len(), SPLITS);
    assert!(validated.iter().all(|(_, changed)| *changed));
    let work = crate::interaction::targeting::resolver::structural_work::snapshot();
    assert_eq!(work.root_fingerprint_builds, 0);
    assert_eq!(work.root_fingerprint_node_visits, 0);

    let foreign_source = NodeSource {
        root: MAIN,
        node: foreign,
        reference: original.reference().clone(),
    };
    let foreign_batch = [
        splits[0].clone(),
        SplitResize::new(foreign_source, SplitWeight::normalize([0.4, 0.6]).unwrap()),
    ];
    assert!(matches!(
        validate_split_resizes_structure(&workspace, &foreign_batch),
        Err(CommandError::NodeOutsideRoot { root: MAIN, node, .. }) if node == foreign
    ));

    workspace.begin_root_edit(MAIN);
    let Some(Node::Split { weights, .. }) = workspace.storage.nodes.get_mut(subtree) else {
        unreachable!();
    };
    *weights = SplitWeight::normalize([0.3, 0.7]).unwrap();
    workspace.complete_root_change(MAIN).unwrap();
    workspace.complete_root_structure(MAIN).unwrap();
    let current = workspace.capture_node_source(MAIN, subtree).unwrap();
    let stale_batch = [
        SplitResize::new(current, SplitWeight::normalize([0.4, 0.6]).unwrap()),
        splits[0].clone(),
    ];
    assert!(matches!(
        validate_split_resizes_structure(&workspace, &stale_batch),
        Err(CommandError::StaleNode { node, .. }) if node == nodes[0]
    ));
}
