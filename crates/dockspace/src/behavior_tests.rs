//! Crate-internal behavior and protocol regression suite.

#[path = "../tests/support/mod.rs"]
mod support;

#[path = "../tests/canonical_graph.rs"]
mod canonical_graph;
#[path = "../tests/command_model.rs"]
mod command_model;
#[path = "../tests/command_sequences.rs"]
mod command_sequences;
#[path = "../tests/contained_placement.rs"]
mod contained_placement;
#[path = "../tests/contained_presentation_update.rs"]
mod contained_presentation_update;
#[path = "../tests/contained_transform.rs"]
mod contained_transform;
#[path = "../tests/desktop_global_pointer_journal.rs"]
mod desktop_global_pointer_journal;
#[path = "../tests/document.rs"]
mod document;
#[path = "../tests/drop_guide_interaction.rs"]
mod drop_guide_interaction;
#[path = "../tests/drop_guide_resolution.rs"]
mod drop_guide_resolution;
#[path = "../tests/drop_guides.rs"]
mod drop_guides;
#[path = "../tests/engine_atomicity.rs"]
mod engine_atomicity;
#[path = "../tests/external_item_key_map.rs"]
mod external_item_key_map;
#[path = "../tests/geometry_types.rs"]
mod geometry_types;
#[path = "../tests/graph_model.rs"]
mod graph_model;
#[path = "../tests/layout_solver.rs"]
mod layout_solver;
#[path = "../tests/mixed_dpi.rs"]
mod mixed_dpi;
#[path = "../tests/model_properties.rs"]
mod model_properties;
#[path = "../tests/native_surface_close_protocol.rs"]
mod native_surface_close_protocol;
#[path = "../tests/open_gpui_close_contract.rs"]
mod open_gpui_close_contract;
#[path = "../tests/open_gpui_drop_contract.rs"]
mod open_gpui_drop_contract;
#[path = "../tests/open_gpui_presentation_contract.rs"]
mod open_gpui_presentation_contract;
#[path = "../tests/persistence.rs"]
mod persistence;
#[path = "../tests/pointer_authority.rs"]
mod pointer_authority;
#[path = "../tests/pointer_host_frame.rs"]
mod pointer_host_frame;
#[path = "../tests/pointer_journal_properties.rs"]
mod pointer_journal_properties;
#[path = "../tests/pointer_receiver_receipts.rs"]
mod pointer_receiver_receipts;
#[path = "../tests/pointer_scroll_journal.rs"]
mod pointer_scroll_journal;
#[path = "../tests/policy_snapshot.rs"]
mod policy_snapshot;
#[path = "../tests/policy_transaction_authority.rs"]
mod policy_transaction_authority;
#[path = "../tests/presentation_compiler.rs"]
mod presentation_compiler;
#[path = "../tests/presentation_hit_manifest.rs"]
mod presentation_hit_manifest;
#[path = "../tests/presentation_manifest.rs"]
mod presentation_manifest;
#[path = "../tests/product_actions.rs"]
mod product_actions;
#[path = "../tests/product_model.rs"]
mod product_model;
#[path = "../tests/recovery_policy_authority.rs"]
mod recovery_policy_authority;
#[path = "../tests/reducer_ticks.rs"]
mod reducer_ticks;
#[path = "../tests/rootless_structural.rs"]
mod rootless_structural;
#[path = "../tests/splitter_resize_regressions.rs"]
mod splitter_resize_regressions;
#[path = "../tests/surface_scene_exchange.rs"]
mod surface_scene_exchange;
#[path = "../tests/tab_gesture_activation.rs"]
mod tab_gesture_activation;
#[path = "../tests/tab_mru.rs"]
mod tab_mru;
#[path = "../tests/tick_final_surface_vacancy.rs"]
mod tick_final_surface_vacancy;
#[path = "../tests/transaction_atomicity.rs"]
mod transaction_atomicity;
#[path = "../tests/viewport_focus.rs"]
mod viewport_focus;
#[path = "../tests/viewport_lifecycle.rs"]
mod viewport_lifecycle;
#[path = "../tests/viewport_persistence.rs"]
mod viewport_persistence;
#[path = "../tests/viewport_recovery.rs"]
mod viewport_recovery;
#[path = "../tests/viewport_surface_roster.rs"]
mod viewport_surface_roster;
