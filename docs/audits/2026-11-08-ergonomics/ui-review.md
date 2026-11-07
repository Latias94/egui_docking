# Final UI and construction source review

Verdict: **no blocking correctness or regression finding** in the reviewed UI,
construction and owned-run composition changes. No implementation change is
required by this review.

The review compares the current worktree with
`2697b04ca9733af173863419d657bdac5d67de5d`. It excludes re-review of this
lane's implementor migrations. No Cargo, tests, GUI, network or timing was run;
the parent's reported 2,252 passes and three skips are not independent execution
by this reviewer. `docs/reviews` and repository sources were not modified.

## Tooltip interaction and output ownership

The new calls are ordinary `Response::on_hover_text`: tab title and tab close
(`product_render/tabs.rs:352,553`), contained title and group close
(`contained.rs:145,196`), and overflow controls (`tab_chrome.rs:54–61`).
They contain text, not buttons or selectable content.

Both inspected dependencies make text tooltips click-through. The official
registry `egui 0.37.0` implementation:

- `response.rs:785–794` creates a normal tooltip with a Label.
- `containers/tooltip.rs:156–178` makes the Area interactable only when the
  previous tooltip had interactive widgets or widget inspection is active;
  selectable labels are explicitly disabled.
- `containers/popup.rs:145–148` places this on **Order::Tooltip**, distinct from
  the Order::Foreground used by menus.
- `memory/mod.rs:1285–1306` skips non-interactable Areas in `layer_id_at`.
- `context.rs:1678–1707` omits disabled and non-interactive widgets from the
  interactive rectangle inventory.

Consequently, an ordinary long-hover text tooltip cannot itself make the
official adapter's `snapshot.layer_at(position) != scene.layer` check fail, nor
add an unknown interactive rectangle to `product_pointer.rs:533–571`.
The pinned native fork `f94ee58` has the same tooltip interactivity rule and its
completed-pass hit test also filters non-interactable Areas
(`hit_test.rs:141–154`). There is no new persistent blocker or bypass in these
changes. Inspection mode and genuinely interactive popups keep their existing
blocking semantics; they must not receive a blanket tooltip exemption.

Output authority is also retained. The owned-run surface capture uses
`SurfaceOutputCapture::begin_multilayer` and finishes after chrome painting
(`product_owned_run.rs:447–482`). That capture accepts newly registered layer
suffixes while preserving prior prefixes and transforms
(`output_proof.rs:347–383`); it does not omit tooltip paint or invent renderer
presentation. Pane callbacks remain separately audited. This conclusion is
source-based: the existing foreign-popup/modal blocker tests were inspected,
but this lane did not execute a long-dwell GUI journey.

## UI behavior

- **Contained title:** `contained.rs:69–103` reserves the existing grip gutter,
  uses the actual title-drag horizontal bounds, ignores unrelated vertical
  window margins, truncates within available width, and vertically centers a
  Button-style galley in the title strip. Empty horizontal space skips text
  paint. None of the receiver geometry, source-gap policy or actions changed.
  Full title text remains available through the tooltip and accessibility
  label. `floating_title_glyphs_fit_without_using_vertical_window_margins`
  checks actual emitted glyph bounds under two window margins, rather than
  mirroring the inset helper. Arbitrarily enlarged fonts or deliberately tiny
  configured title heights are not newly guaranteed by this fix.
- **Overflow arrows:** `tab_chrome.rs:100–121` now places the chevron tip at
  `direction * half`. Backward therefore points left and forward right.
  Control identities, command dispatch, disabled blockers and accessibility
  labels remain unchanged. The added test examines actual line segments in
  the corresponding accessible button bounds.
- **Close clarity:** the contained close tooltip explicitly says it closes all
  panes. This is additional description; the application-owned close decision
  workflow remains authoritative. Tooltip creation does not activate the
  control.

## Construction and composition

`builder.rs:16–79` removes a single-variant source enum while retaining style
validation before the persistent/non-persistent branch and forwarding exactly
the same layout, policy, style, identity and document bootstrap.

`product_dockspace.rs:63–148` centralizes three identical adapter initializers.
Every field has the preceding default value, including pending focus, pointer,
settlement flags and UI action queue. Session creation and its typed failure
paths still precede the private `from_session` helper. It introduces no public
unchecked constructor and does not bypass layout, persistence or presentation
configuration validation.

`run_single_surface` delegates to the same owned executor with an empty
composition closure (`product_owned_run.rs:232–263`). The new callback executes
once per actual pass before the tagged docking child; the original input
snapshot, candidate, output hooks, proof validation, publication and renderer
receipt paths remain. Its documentation correctly says external callback side
effects do not roll back, and instructs callers to apply command intent only
after success.

Existing tests now exercise the actual composition seam: remaining panel and
pane bounds (`product_owned_run_tests.rs:233–299`), repeated passes (`:438–478`)
and late output damage with candidate rollback (`:785–824`). A composition
callback panic is subject to the same existing run and candidate RAII owners;
this change adds no alternate publication path.

## Source bindings

SHA-256 values captured before any later parent formatting or integration:

```text
c4ec18d9e577288b12943e373248ad44d45d28a95729355373aec01cdf03252e  crates/egui_dockspace/src/builder.rs
3fea64e46d5b68bd78c722df5ac3880b739824d7d57f6ce01c5fbbee903cab32  crates/egui_dockspace/src/product_dockspace.rs
39f207e30100173d1438d2bb0d417f811b7c36ae462778316ca0502ade0e950b  crates/egui_dockspace/src/product_owned_run.rs
6620532336fc6a64cedbed91d4ea539a8f589bb4b3c74237b2382849363ecb76  crates/egui_dockspace/src/product_owned_run_tests.rs
2c4bdcc7f85cf257b74a5f2cb17a8a8a05eb4d3cf6f278a8bd43a0166367e539  crates/egui_dockspace/src/product_render/contained.rs
e292cc7b62ab8e79e7d4dd07400ff68b61abf450a66d9f064350c0a447722bb4  crates/egui_dockspace/src/product_render/tab_chrome.rs
797a1f4329c11e9ae005fa9a223223ddee6f47ced24b0bbb49ad5a312894009b  crates/egui_dockspace/src/product_render/tabs.rs
7f12c96f749898b672a29ce5780a2458fecfd3f5aacfbee88ddbff20922a33ac  crates/egui_dockspace/src/product_render/mod.rs
01a5c88d113739d5e2e242c687654901e1f35cb6cb922e3958aa492217bf5e65  crates/egui_dockspace/src/product_pointer.rs
5018b79a74515960bc652ec9e20a734c4fb3504f4db9514d035aefb3097075e5  crates/egui_dockspace/src/output_proof.rs
aa6a7991b2289c01e887bf1da60fa959658423f9f2e129f62c0a39fc026d019d  integration/egui-official-harness/tests/hover_blockers.rs
```
