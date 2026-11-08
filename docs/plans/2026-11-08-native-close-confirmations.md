# Native close confirmations and focus verification

## Outcome

The native editor presents close decisions using pane names and clear choices.
Each accessible action identifies its pane; deferred decisions remain usable
through the same controls. Cross-window selection and ordinary tab-keyboard
navigation are checked in the actual editor.

## Design

- Replace request IDs, target debug output and continuation terminology in close
  confirmations with pane/group headings, Close, Keep open and Decide later.
- Explain that group closes are atomic and that keeping any member open cancels
  the complete request. Preserve application-owned data and explicit decisions.
- Render pending and deferred decisions through one set of controls. Keep the
  exact core-issued initial/deferred tokens and the existing bounded command
  slot; no batch executor, implicit approval or new public interface is needed.
- Give each accessible button a pane-qualified name. Disable further decisions
  while the accepted command or its result owns the slot.
- Use actual-window evidence to check that selecting a pane in an existing
  native child activates its window and editable content. Check End/Home tab
  navigation without changing core focus rules absent a reproduced defect.

## Reproduced focus correction

Ordinary X11 input reproduced an inventory selection that reported an already
selected Inspector while leaving the host window active and the Inspector's
existing native child behind it. Two runs failed the active-window assertion.
Product selection currently installs the same local pane intent used by pointer
tab gestures, which intentionally cannot request native window activation.

For explicit product selection only, reuse the existing explicit viewport
activation path when the pane has a current native focus binding. This keeps
binding eligibility, platform capabilities, causal ordering and observation
acknowledgement under the existing focus coordinator. Embedded selection keeps
its current local pane-focus path. Pointer gestures, movement and close recovery
retain their current focus rules; no second activation queue is introduced.

Add a core regression before correction: selecting an already-selected pane in
another native window must emit that exact binding's RequestFocus, wait for the
native observation, and then request pane focus without claiming it was observed.
Recheck the shared core/portable consumers as well as the native integration.

## Reproduced keyboard correction

The actual-window keyboard check exposed another baseline defect. Home moves
from a focused Inspector tab to Preview and retains tab focus. End selects
Inspector, but the queued action then requests its editable content's focus.
Navigation was prepared as an ordinary tab activation, losing its focus purpose
before deferred submission. A pane without an editable focus target hid this
distinction in the existing renderer regression.

Preserve navigation and content activation as distinct internal surface actions
through immediate and deferred submission. Share the existing frozen-witness
validation, selection and contained raising logic. Navigation preserves framework
tab focus and advances the existing focus coordinator's causal claim, without
creating a pane-focus request or claiming observed focus. This also prevents an
older unfinished pane intent from overriding a newer navigation. Content
activation continues to request pane focus normally.

Use real editable focus targets in the regression. Assert continuous Home/End
navigation and tab focus after queued selection commits and subsequent empty
frames, then repeat the sequence in the actual X11 editor.

## Verification and completion

Add focused controls tests from real core close plans, including deferred
continuation and the pending-slot state. Observe the ambiguous-action test fail
before changing the UI. Run native all-feature/default nextest serially, the
existing public doctest, Clippy and formatting. Build/check Linux consumers and
exercise the actual close-confirmation controls with ordinary X11 mouse/key
input. Verify complete group close and retained-data reopen; preserve existing
programmatic command and close semantics. Run an independent review and create
a local Conventional Commit. Existing `docs/reviews/` stays excluded.

Core ownership, focus authority, docking algorithms and platform support claims
are unchanged unless an actual defect requires a scoped correction. Remote
publication is outside local completion.
