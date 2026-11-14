use super::*;
use crate::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use crate::graph::{RootRecord, SurfacePresentation};
use crate::platform::viewport::{
    CloseObservationGeneration, InventoryGeneration, ViewportRole, WindowToken,
};
use crate::policy::CloseCapability;
use crate::runtime::NativeSurfacePlacement;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);

fn placements(include_b: bool) -> Vec<NativeSurfacePlacement> {
    [A, B]
        .into_iter()
        .take(if include_b { 2 } else { 1 })
        .enumerate()
        .map(|(index, surface)| {
            NativeSurfacePlacement::new(
                surface,
                LogicalSize::new(300.0, 240.0).unwrap(),
                PhysicalPoint::new(if index == 0 { 0.0 } else { 300.0 }, 0.0).unwrap(),
                ScaleFactor::new(1.0).unwrap(),
                PhysicalRect::new(if index == 0 { 0.0 } else { 300.0 }, 0.0, 300.0, 240.0).unwrap(),
            )
        })
        .collect()
}

fn fixture(empty_b: bool) -> (DockEngine, NativeCloseEdge, Vec<ViewportBinding>) {
    let mut builder = Workspace::builder();
    for (surface, items) in [
        (A, vec![ItemId::new(1)]),
        (
            B,
            if empty_b {
                Vec::new()
            } else {
                vec![ItemId::new(2)]
            },
        ),
    ] {
        let node = builder.insert_node(Node::tabs(items));
        let root = RootId::new(if surface == A { 1 } else { 2 });
        builder.set_root(root, RootRecord::new(node).with_central(node));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
    }
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let mut engine = DockEngine::new(builder.build().unwrap(), policy).unwrap();
    let binding = engine
        .viewport
        .register_existing(
            engine.version.epoch(),
            A,
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .unwrap();
    engine
        .viewport
        .set_surface_placements(binding.window(), &placements(true))
        .unwrap();
    let bindings = engine
        .viewport
        .registry()
        .bindings_for_window(binding.window())
        .collect::<Vec<_>>();
    let edge = NativeCloseEdge::from_authoritative_requested(
        engine.authority_domain,
        binding,
        CloseObservationGeneration::new(1),
        InventoryGeneration::new(1),
    );
    (engine, edge, bindings)
}

fn open(engine: &mut DockEngine, edge: NativeCloseEdge, bindings: &[ViewportBinding]) -> ClosePlan {
    let capture = engine
        .capture_surface_close(edge, &SurfaceCloseRequest::CloseContent, &engine.policy)
        .unwrap();
    engine
        .close
        .open_window(
            engine.close_authority(),
            edge,
            SurfaceCloseRequest::CloseContent,
            bindings.to_vec(),
            capture.requirements,
            capture.prepared,
        )
        .unwrap()
}

#[test]
fn window_content_close_waits_for_all_siblings_and_prepares_one_draft() {
    let (mut engine, edge, bindings) = fixture(false);
    let before = engine.workspace.clone();
    let plan = open(&mut engine, edge, &bindings);
    assert_eq!(plan.native_bindings(), bindings);
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [ItemId::new(1), ItemId::new(2)]
    );
    let authority = engine.close_authority();
    assert!(matches!(
        engine
            .close
            .resolve(
                plan.request(),
                plan.items()[0].token(),
                authority,
                CloseDecision::Allow
            )
            .unwrap(),
        CloseResolutionOutcome::Recorded { .. }
    ));
    let deferred = engine
        .close
        .resolve(
            plan.request(),
            plan.items()[1].token(),
            authority,
            CloseDecision::Deferred,
        )
        .unwrap();
    let CloseResolutionOutcome::Deferred { continuation, .. } = deferred else {
        panic!("the second sibling must keep the entire close deferred")
    };
    assert_eq!(engine.workspace, before);
    assert_eq!(
        engine.close.plan(plan.request()).unwrap().phase(),
        ClosePlanPhase::Deferred
    );
    assert!(matches!(
        engine.close.continue_deferred(
            plan.request(),
            continuation,
            authority,
            DeferredCloseDecision::Allow
        ),
        CloseResolutionOutcome::Approved { .. }
    ));
    let prepared = engine.close.prepared(plan.request()).unwrap();
    crate::interaction::targeting::resolver::structural_work::reset();
    let commit = engine
        .stage_prepared_surface_close(engine.last_input, prepared, &BTreeMap::new())
        .unwrap()
        .unwrap();
    assert!(commit.candidate.surface(A).is_none());
    assert!(commit.candidate.surface(B).is_none());
    assert_eq!(commit.events.len(), 2);
    assert_eq!(
        engine.workspace, before,
        "preparation leaves the published owner untouched"
    );
    assert_eq!(
        crate::interaction::targeting::resolver::structural_work::snapshot()
            .workspace_deep_clones
            .transaction_candidates
            .calls,
        1
    );
}

#[test]
fn window_content_close_keeps_an_empty_central_sibling_in_the_exact_scope() {
    let (mut engine, edge, bindings) = fixture(true);
    let plan = open(&mut engine, edge, &bindings);
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.native_bindings().len(), 2);
    assert!(matches!(
        engine
            .close
            .resolve(
                plan.request(),
                plan.items()[0].token(),
                engine.close_authority(),
                CloseDecision::Allow,
            )
            .unwrap(),
        CloseResolutionOutcome::Approved { .. }
    ));
    let prepared = engine.close.prepared(plan.request()).unwrap();
    let commit = engine
        .stage_prepared_surface_close(engine.last_input, prepared, &BTreeMap::new())
        .unwrap()
        .unwrap();
    assert!(commit.candidate.surface(A).is_none());
    assert!(commit.candidate.surface(B).is_none());
    assert_eq!(commit.events.len(), 2);
}

#[test]
fn one_sibling_veto_keeps_the_complete_window_close_unapproved() {
    let (mut engine, edge, bindings) = fixture(false);
    let before = engine.workspace.clone();
    let plan = open(&mut engine, edge, &bindings);
    let authority = engine.close_authority();
    assert!(matches!(
        engine
            .close
            .resolve(
                plan.request(),
                plan.items()[0].token(),
                authority,
                CloseDecision::Allow
            )
            .unwrap(),
        CloseResolutionOutcome::Recorded { .. }
    ));
    assert!(matches!(
        engine
            .close
            .resolve(
                plan.request(),
                plan.items()[1].token(),
                authority,
                CloseDecision::Veto
            )
            .unwrap(),
        CloseResolutionOutcome::Vetoed { .. }
    ));
    let current = engine.close.plan(plan.request()).unwrap();
    assert_eq!(current.phase(), ClosePlanPhase::Vetoed);
    assert_eq!(current.native_bindings(), bindings);
    assert_eq!(engine.close.active_plans().count(), 1);
    assert_eq!(engine.workspace, before);
    assert!(engine.close.native_close_effect(plan.request()).is_none());
}

#[test]
fn changing_and_restoring_window_membership_cannot_revive_a_close_scope() {
    let (mut engine, edge, bindings) = fixture(false);
    let plan = open(&mut engine, edge, &bindings);
    engine
        .viewport
        .set_surface_placements(edge.binding().window(), &placements(false))
        .unwrap();
    engine.reconcile_close_scopes();
    assert!(engine.close.prepared(plan.request()).is_none());
    engine
        .viewport
        .set_surface_placements(edge.binding().window(), &placements(true))
        .unwrap();
    engine.reconcile_close_scopes();
    assert!(engine.close.prepared(plan.request()).is_none());
    assert!(engine.workspace.surface(A).is_some());
    assert!(engine.workspace.surface(B).is_some());
    assert!(matches!(
        engine.close.plan(plan.request()).unwrap().phase(),
        ClosePlanPhase::CancelRequested | ClosePlanPhase::Stale
    ));
}
