use super::*;
use crate::presentation::tab_strip::TabStripStateKey;

fn session(generation: u64) -> TabListMenuSessionId {
    TabListMenuSessionId::new_for_test(
        TabStripStateKey::new(
            SurfaceId::new(1),
            TabBarSceneId {
                root: RootId::new(2),
                tabs: NodeId::default(),
            },
        ),
        generation,
    )
}

fn tab(root: RootId, item: u64) -> SurfaceProgramRootOperation {
    SurfaceProgramRootOperation::Tab(TabSceneId {
        root,
        tabs: NodeId::default(),
        item: ItemId::new(item),
    })
}

fn assert_finish_rejected_without_consuming(
    program: &mut SurfaceProgram,
    expected: SurfaceProgramBuildError,
) {
    let before = program.clone();
    assert_eq!(program.finish(), Err(expected));
    assert_eq!(
        *program, before,
        "rejected drafts remain available for repair"
    );
}

#[test]
fn finish_preserves_first_duplicate_category_precedence_and_repairable_drafts() {
    let root = RootId::new(2);
    let first_guide = DropGuideClusterId::outer(SurfaceId::new(1), RootId::new(9));
    let smaller_guide = DropGuideClusterId::outer(SurfaceId::new(1), RootId::new(1));
    let first_session = session(9);
    let smaller_session = session(1);
    let orphan_session = session(10);
    let first_target = DropTargetId::TabGap {
        surface: SurfaceId::new(1),
        root,
        tabs: NodeId::default(),
        index: 9,
    };
    let smaller_target = DropTargetId::TabGap {
        surface: SurfaceId::new(1),
        root,
        tabs: NodeId::default(),
        index: 1,
    };
    let first_operation = tab(root, 9);
    let smaller_operation = tab(root, 1);
    let mut program = SurfaceProgram {
        deferred_guides: vec![first_guide, smaller_guide, first_guide, smaller_guide],
        deferred_popup_backdrops: vec![
            first_session,
            smaller_session,
            first_session,
            smaller_session,
        ],
        deferred_popup_menus: vec![
            first_session,
            smaller_session,
            first_session,
            smaller_session,
            orphan_session,
        ],
        deferred_drop_targets: vec![first_target, smaller_target, first_target, smaller_target],
        deferred_guide_targets: vec![first_target, first_target],
        ..SurfaceProgram::default()
    };
    program
        .begin_root(root, SurfaceProgramRootPresentation::Main)
        .expect("root begins");
    for operation in [
        first_operation,
        smaller_operation,
        first_operation,
        smaller_operation,
    ] {
        program
            .push_root_operation(operation, None)
            .expect("operation is drafted");
    }
    program.end_root(root).expect("root ends");

    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::DuplicateGuide { id: first_guide },
    );
    program.deferred_guides.truncate(2);
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::DuplicatePopupBackdrop {
            session: first_session,
        },
    );
    program.deferred_popup_backdrops.truncate(2);
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::DuplicatePopupMenu {
            session: first_session,
        },
    );
    program.deferred_popup_menus = vec![first_session, smaller_session, orphan_session];
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::PopupMenuWithoutBackdrop {
            session: orphan_session,
        },
    );
    program.deferred_popup_menus.pop();
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::DuplicateDropTarget { id: first_target },
    );
    program.deferred_drop_targets.truncate(2);
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::DuplicateOperation {
            root,
            operation: first_operation,
        },
    );
    program.root_drafts[0].operations.truncate(2);
    program.finish().expect("the repaired draft finishes");
    let SurfaceProgramTape::Draft(blocks) = &program.tape else {
        panic!("finish retains the actual draft until record linking");
    };
    let drop_targets = blocks.iter().filter_map(|block| match block {
        SurfaceProgramBlock::DropTarget(id) => Some(*id),
        _ => None,
    });
    assert_eq!(
        drop_targets.collect::<Vec<_>>(),
        vec![smaller_target],
        "guide membership compares values from separate draft inventories",
    );
    assert_finish_rejected_without_consuming(
        &mut program,
        SurfaceProgramBuildError::AlreadyFinished,
    );
}
