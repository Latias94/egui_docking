# Custom UI host

Read **Engine host integration** in `crates/dockspace/docs/guide.md`,
`docs/ADAPTER_TESTING.md`, and
`integration/dockspace-host-harness/examples/engine_host.rs`.

## Commit, render and present

Own one `DockspaceSession`. Submit ordered input and semantic commands through
its host frame, then supply logical measurements and traverse the complete
ordered surface program. A retained UI copies the required geometry, pane
references and stable `DockspaceVisualId` values into staged owned records.
Preserve program clipping, ordering and `SourceGap` semantics.

Confirm the execution receipt and commit before publishing staged UI changes.
Failed or abandoned candidates leave the installed artifact intact. Input-only
and presentation-settlement frames can commit without painting.

Keep each committed output handle paired with the artifact and its hit records.
Consume it with `report_surface_presentation(Presented | Dropped)` after the
renderer knows the result. Commit a subsequent host frame, then use its exact
`surface_presentation_commits` and `bind_presented_receiver` to promote receiver
authority. Sink acceptance, frame commit and renderer submission each establish
less than actual presentation. Rendering completion may arrive out of order;
an unsettled earlier output still bounds admission in its stream.

For batched application commands, retain each `DockspaceSubmittedAction` and
resolve it against that frame's `HostFrameReport::submitted_action_outcome`.
This preserves the connection between a command and its committed result.

## Route real input facts

Provide one total event order across logical surfaces and use each event's own
coordinates and available timing metadata. Helper samples stay untimed unless
the host supplies genuine observation metadata. A real callback observation time
is valid: use a consistent monotonic clock, retain its source, and preserve the
captured value through queuing and retries. Distinguish it from an original OS
timestamp. During capture, manipulation stays in the source surface while hover
may resolve to a different surface. Keep window DPI,
surface placement, UI zoom and framebuffer density distinct and apply each
coordinate conversion once.

Bind input to the actually presented artifact. Unknown overlays or receiver
facts remain unknown or blocked. At a continuation deadline, sample current
pointer, capture and receiver facts and drive the continuation; a timer does not
prove motion or presentation.

Use `dockspace_winit` helpers when the host uses winit. Invalidate stale mappings
through resize/DPI gaps until a replacement output is actually presented, while
continuing to deliver releases and cancellation. Follow
`crates/dockspace_winit/examples/software.rs` and its `software/` modules for a
complete single-window renderer-neutral host. Native windows and richer input
need their own platform integration, as described in the crate's README.

Keep rendering asynchronous through owned records and terminal reports when the
framework needs it. Host frames and pane callbacks remain synchronous; an async
runtime is an application choice.

## Verify the boundary

Reuse applicable `dockspace_host_conformance` scenario descriptions and provide
a thin executor through the host's real controls. Its existing core executors
are private tests; the catalog is not an external adapter runner. Start with
`integration/dockspace-host-harness/tests/shared_local_scenario.rs` for that seam.
For a deferred renderer, verify commit failure leaves installed UI intact,
dropped/out-of-order outputs settle correctly, and new hit records gain authority
only after their presentation report commits. Test capture between two surfaces
with identical local bounds and a DPI/zoom change. Add a real renderer/input
smoke for the framework; conformance fixtures establish the core contract.

For OS windows, continue with [multiview](multiview.md).
