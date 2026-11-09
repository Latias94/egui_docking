# Native promotion of offscreen contained groups

This follow-up starts from `b7309ef`. In the ordinary editor, shrinking the
main window to 680 × 620 puts the Inspector group at x=850 entirely outside
the host's visible area. Open Inspector Window then rejects the request with
`surface 1 has no current presentation geometry`. Expanding the host enough
to reveal part of the group makes the same action succeed.

## Cause and correction

The surface compiler measures every contained group before visibility clipping
and retains its minimum size in `contained_minimums`. A group outside the surface
does not receive a visible `ContainedRecord`. Explicit native promotion used
that visible record to obtain the minimum size required by its recovery
reservation. It therefore confused absent visible chrome with absent measured
geometry, even while the source surface retained valid presentation authority.

Both the availability query and the action reducer now read the existing
contained minimum measurement by floating identity. The current workspace
still identifies the root's owning surface and floating presentation. Scene
validation guarantees complete, unique measurement coverage for each surface;
floating identities are not rebound to a different root. Retained projections
need not represent every latest workspace change, so those identity guarantees
remain relevant when reading their measurements.

Policy, workspace version, complete-root payload, idle interaction, current
source projection, native capability, recovery anchor and staged lifecycle
checks remain in place. Missing measurements still reject the command. No
default measurement, synthetic presentation or new storage is introduced.
Default placement derivation still uses visible geometry to choose a position;
this correction concerns commands supplied with an explicit native placement.

## Regression

The managed native test uses the ordinary measure → paint → presented flow
with a 640 × 480 source surface. It compares contained x=40 with x=850. Both
cases have an interaction projection and retained minimum measurement; only
the visible case has a chrome record. The original reducer produces no native
effect at x=850 and reports `PresentationUnavailable`.

The final test checks command availability, one exact `CreateWindow`, the
requested placement, the complete successful action result and the measured
minimum frozen into the recovery reservation. The source version and both pane
owners must remain unchanged until native creation is admitted. The test reuses
the existing fixture with only its contained x-coordinate parameterized.

The [original failure](2026-11-09-offscreen-native-promotion/offscreen-red.log.gz)
records the rejected offscreen reducer action. The
[focused passing run](2026-11-09-offscreen-native-promotion/offscreen-green.log.gz)
covers the new test and four existing native promotion/recovery constraints.
That focused run precedes the added availability assertion; final complete
suites include it.

## Actual-window baseline

The [baseline journey](2026-11-09-offscreen-native-promotion/baseline_journey.py)
edits Inspector at the original host size, shrinks the host to 680 × 620 and
clicks Open Inspector Window. It uses ordinary XTest input under Linux
Xvfb/Openbox and software GL. This smaller journey uses no tab-list menu.
The exact child window never appears, causing a bounded lookup to fail.
The [screenshot](2026-11-09-offscreen-native-promotion/offscreen-before.png)
shows the rejection, and the
[receipt](2026-11-09-offscreen-native-promotion/baseline-journey.json),
[binary identity](2026-11-09-offscreen-native-promotion/baseline-identity.json)
and [failure log](2026-11-09-offscreen-native-promotion/baseline-journey.log.gz)
identify the run. This is the same binary used for the preceding menu fix's
final GUI check, before the offscreen correction.

## Actual-window correction

The [final journey](2026-11-09-offscreen-native-promotion/final_journey.py)
uses the corrected ordinary editor in the same X11 software-GL environment.
It adds a screenshot checkpoint before shrinking, types at 50 ms intervals,
and extends the creation check through native close and recovery. It never
expands the 680 × 620 source host before requesting creation.

| Check | Observed result |
| --- | --- |
| Edit Inspector before shrinking the host | `~OFFSCREEN_RETAINED~` is visible in the original editor. [Input](2026-11-09-offscreen-native-promotion/input-before-shrink.png) |
| Shrink the host and request Open Inspector Window | The child appears while the host remains narrow; no geometry rejection. |
| Type immediately in the new child without clicking its content | The retained marker and `~OFFSCREEN_PROMOTED~` are both visible with the editor focus ring. [Child](2026-11-09-offscreen-native-promotion/child-input-after.png) |
| Close the child with Alt+F4 | The child retires and Inspector/Preview recover to a contained group. Both markers remain visible after expanding the host for inspection. [Recovered](2026-11-09-offscreen-native-promotion/recovered-content.png) |

The script asserts the exact child appearance, active window, host identity,
live process and visible child's retirement. Text and pane state above were
inspected separately in original screenshots. The
[execution receipt and source hashes](2026-11-09-offscreen-native-promotion/final-journey.json)
and [final binary identity](2026-11-09-offscreen-native-promotion/final-identity.json)
bind this check to the corrected sources. Source hashes still match after the
run; final application stderr is empty.

## Verification and review

All [eleven serial gates](2026-11-09-offscreen-native-promotion/verification.json)
passed with one Cargo build job and nextest worker, reusing existing targets:

| Suite | Result | Log |
| --- | --- | --- |
| Main workspace, all features and targets | 2259 passed, 3 existing skips | [Log](2026-11-09-offscreen-native-promotion/workspace-all.log.gz) |
| Native workspace, all features and targets | 616 passed, 1 existing skip | [Log](2026-11-09-offscreen-native-promotion/native-all.log.gz) |
| Native package, default features and all targets | 565 passed, 1 existing skip | [Log](2026-11-09-offscreen-native-promotion/native-default.log.gz) |
| Product integration harness, all features and targets | 119 passed | [Log](2026-11-09-offscreen-native-promotion/product-all.log.gz) |
| Host integration harness, all features and targets | 335 passed | [Log](2026-11-09-offscreen-native-promotion/host-all.log.gz) |

Main and native workspace Clippy checks pass with correctness and suspicious
lints denied. Both format checks, the diff check and the Linux all-features
example build pass. These complete suites include the final availability
assertion and documentation.

Independent correctness and standards reviewers found no remaining substantive
issues. They checked the retained floating/root identity guarantees, unchanged
presentation and lifecycle gates, both minimum lookups, the exact-effect test
and the explicit-placement documentation. Their reviews were static; they ran
no Cargo or GUI and did not access the unrelated `docs/reviews/` directory.

This correction adds no public API or retained state. The actual-window evidence
qualifies the observed X11 behavior; it does not cover mixed DPI, Wayland,
macOS or Windows native promotion.
