# UI adapter integration and testing

`dockspace` supports immediate rendering and retained UI integration through the
same public `model`, `policy`, and `runtime` interfaces. Core dependencies contain
no UI or platform framework. The independent
[public host harness](../integration/dockspace-host-harness/README.md) demonstrates
owned rendering records and input submission outside egui. Egui is the main
reference adapter. The independent [GPUI harness](../integration/gpui-harness/README.md)
adds a retained consumer with actual pane views, input and exact-frame submission
using a neighboring Open GPUI checkout. Its single-window scope does not qualify
GPUI native multiview. Recording tests do not qualify a framework's renderer or
operating-system integration.

## Adapter responsibilities

| Boundary | Core supplies | Adapter supplies and verifies |
| --- | --- | --- |
| Measurement | Typed measurement requests and logical layout | Text, chrome and pane measurements in logical units |
| Rendering | Ordered surface program, stable visual IDs, clipping and content requests | Drawing or retained-node updates with the complete ordering and clipping |
| Hit testing | Receiver descriptors and semantic target resolution | Actual framework delivery, overlay occlusion and current hover facts |
| Input | Ordered journal, gesture reduction and source/hover routes | Event-time coordinates, capture facts and one total order across surfaces |
| Presentation | Exact output handles, settlement and receiver authority | The real result of presenting each exact artifact, including dropped output |
| Time and focus | Continuation deadlines and typed pane-focus requests | A monotonic clock, fresh observations at wake time and framework focus execution |
| Native hosting | Typed effects, bindings and lifecycle obligations | Window creation, client/desktop mapping, physical focus and destruction observations |

Use `measure_surface_with` for measurements and
`SurfacePaintPlan::execute_surface_program` for the complete ordered program.
An adapter can draw during traversal or copy the required data into owned
records for later consumption. `DockspaceVisualId` can key retained nodes.
Preserve `SourceGap` identity and semantic geometry while suppressing its ordinary
visible pixels. Do not retain frame borrows or expose private engine reducers.

Application commands and prepared surface actions work without a pointer producer
or native registration. Add those capabilities when the adapter implements their
facts; a single-window adapter need not implement native hosting first.

For a center merge or tab insertion, `DockspaceDragPreview::tab_bar` and `tabs`
expose the future strip computed during the existing candidate preflight. Paint
its members in order, using their visible and title clips. `visit_tab_resources`
includes future members even when they are clipped out of the current strip.
These records grant no input authority; the currently presented layout remains
the target-selection basis. Split targets do not add future tab shapes.

## Retained UI publication

Keep candidate node changes and hit records separate from the currently installed
artifact until the host-frame commit succeeds:

1. Measure and traverse the complete surface program into staged owned records.
   Keep rendering and hit records associated with the same exact output.
2. Pass the execution receipt to `confirm_surface_painted`, account for unpainted
   surfaces, then commit the host frame. A receipt proves sink acceptance; it
   does not prove presentation. Discard staged changes if the candidate fails.
3. Install or enqueue the committed artifact using the framework's own rendering
   boundary. Keep its output handle until that exact artifact is presented or
   dropped. Do not promote its new hit records to input authority yet.
4. Consume the handle with `report_surface_presentation(Presented | Dropped)`
   after observing the actual result. A failed or abandoned render is dropped.
5. Commit a subsequent host frame. Match `surface_presentation_commits` to the
   exact artifacts, retire obsolete records and promote admitted hit records.
   Only then can `bind_presented_receiver` authorize the matching descriptor.

Deferred output capacity is ordinary backpressure. A host can still commit input
without producing a new output. Later presentation results cannot bypass an
unsettled earlier output in the same stream. Keep this behavior when the framework
coalesces updates or renders asynchronously; the
[engine host example](../integration/dockspace-host-harness/examples/engine_host.rs)
demonstrates owned records leaving the frame borrow before exact settlement.

Keep source manipulation and hover routing independent during capture, including
across surfaces with identical local bounds. Map window DPI and framebuffer
density separately. Unknown widgets or overlays cannot assert a clear hover lane.
At a continuation deadline, observe the current pointer, capture and hit facts;
do not replay an old event or synthesize motion to satisfy a timer.

## Reusable automatic tests

| Layer | Existing asset | What passing proves |
| --- | --- | --- |
| Core host conformance | [Scenario catalog and private executors](../crates/dockspace_host_conformance/README.md) | Core behavior through public APIs under declared capabilities |
| Independent public consumer | [Recording host tests](../integration/dockspace-host-harness/tests/) | External API usability, owned output, routing and settlement contracts |
| Concrete UI entry point | [egui product harness](../integration/egui-product-harness/tests/product.rs) | Actual egui measurement, widgets, accessibility and input mapping |
| Retained UI entry point | [GPUI harness](../integration/gpui-harness/README.md) | GPUI views, event delivery, candidate rollback and exact test-renderer outcomes |
| Native physical integration | [X11 qualification drivers](../integration/egui-native-smoke/README.md) | Only the exercised backend, environment and physical journeys |

`dockspace_host_conformance` is an unpublished test crate. Its shared `scenario`
module describes semantic selectors, actions, capabilities and observations. Its
existing core executors are private tests, not a generic external adapter runner.
The journey catalog names coverage requirements; not every entry is a directly
reusable executable scenario.

The [external L08 consumer](../integration/dockspace-host-harness/tests/shared_local_scenario.rs)
is a small example for a new adapter. It consumes
`LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED`, resolves controls in the current public
paint plan, applies prepared actions and checks selection, close requests,
splitter movement and contained resize before recording observations. It uses
the complete recording sink and exact presentation settlement. It declares its
local capabilities and rejects `NATIVE_CREATE_REDOCK_RETIREMENT` before input.
The egui executor consumes the same semantic scenario through actual accessibility
controls and keyboard input. Neither executor shares runtime IDs or output authority.

For a new framework, keep the semantic scenario and replace the thin execution
layer with that framework's real controls. An early headless consumer can use
prepared actions, but UI qualification must also drive the actual input bridge.
Record observed state or feedback after checking it; do not copy expected
observations into the result without checking the behavior.

Extend coverage in this order:

1. Run L08 through real tab, close, splitter and contained-resize controls.
2. Run `MAGNETIC_DOCKING_BASINS` (L03): five basins, three normalized inset samples
   and four cadences, for 60 cases. Resolve points against current framework
   bounds rather than copying egui coordinates.
3. Exercise rollback, stale or foreign receivers, dropped and late outputs,
   overlay occlusion, independent source/hover routing and deferred consumption.
4. Verify continuation scheduling and pane-focus observations through the actual
   framework. Add N01 lifecycle and physical journeys when native support exists.

Declare unsupported capabilities before scheduling a journey. Passing local
recording scenarios does not imply native or physical authority. A new adapter
needs no new core mutation path, shared GUI driver language or alternate state
machine to reuse these tests.

## Local gates

Run these commands from the repository root, serially. The two independent
workspaces retain their own lockfiles. Existing CI includes all host test targets
in default and `serde` configurations.

```console
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
cargo nextest run -p dockspace_host_conformance --lib --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --test shared_local_scenario --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --test shared_local_scenario --features serde --locked --test-threads=1
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --test product --locked --test-threads=1 -E 'test(shared_local_interaction_scenario) | test(shared_l03_complete_magnetic_docking_matrix)'
python3 scripts/check_core_dependencies.py
```

The dependency check covers core normal/build dependencies across features and
targets. It does not certify an adapter's dependency graph or runtime behavior.
