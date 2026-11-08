# Native inspection and pane workflow

## Outcome

Native applications inspect the last committed workspace without handling an
impossible missing-view state. The editor can reopen content after host rebinding
and float one pane out of a contained multi-pane group. Operation feedback names
the completed user operation.

## Evidence and decisions

- `NativeRuntimeState::with_view` unconditionally wraps the callback result in
  `Some`. Native shutdown preserves the committed session. Return the callback's
  result directly and remove the redundant private forwarding method.
- `basic/layout.rs::reopen_pane` looks up the original `SURFACE` after the host
  can be rebound. Use the currently committed `root_surface()`.
- The Move menu disables floating for every contained pane on the host, including
  panes that can be split out of a multi-pane group. Enable that extraction while
  keeping an already-contained complete singleton inert.
- Reopening and docking into the main area repeat placement selection. Share a
  small example-owned placement query; keep application destination choices out
  of the core and native command executor.
- Newly exposed dock, float, raise and recovery operations currently fall through
  to raw debug output in user feedback. Describe their actual result and no-op
  state in ordinary editor language.

## Work

1. Change the public native inspection callback to return `R`, remove the private
   optional wrapper, and migrate every repository consumer without compatibility
   aliases. Preserve locked callbacks, shutdown diagnostics and prepared-action
   ownership. Document the breaking interface.
2. Correct placement and float eligibility in the existing editor inventory.
   Exercise the actual controls and reopening helper with a rebound host, a
   non-central main group, a rootless host and contained multi-pane content.
3. Improve operation feedback and run the existing finite native-window command
   journey. Independently review the final diff, record evidence and make a local
   Conventional Commit. Preserve existing `docs/reviews/` work.

## Verification

Use one Cargo build job, nextest with one worker, and existing target directories.
Run the native all-feature/all-target workspace suite, default native suite,
doctest, correctness/suspicious Clippy and formatting checks. Existing public
consumer tests prove the direct inspection result; existing shutdown tests must
still inspect the retained committed state. Add focused editor behavior tests,
observe the two identified workflow failures before correction, and rerun the
nine-command actual-window smoke in Xvfb/Openbox software GL. Source changes are
native-only; shared core and portable adapter workspaces need no full rerun.

This work adds no command framework, queue, platform claim or docking algorithm.
Publication and remote CI are outside the local completion criteria.
