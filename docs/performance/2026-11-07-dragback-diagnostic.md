# Preserve the first failed physical drag-back boundary

Fresh CI run `42323715317` at immutable `20fe10d` fails the physical X11/XTest
smoke after successful Child press and Root checkpoint. The first established
failure is the Child threshold semantic barrier: its tree response read reaches
the existing three-second deadline. Five other jobs and the preceding native
checks pass. The later `Retired + Edge(North)` observation occurs around error
cleanup and does not establish the initiating event.

The [independent trace analysis](2026-11-07-dragback-diagnostic/failure-analysis.md)
compares the failed head with the green binary-input run. Both use identical
smoke, native, workflow and fork sources, and identical discovered source,
threshold and target coordinates. The historical green run receives the
threshold frame and completes Center merge/retirement. The evidence does not
prove a layout regression, close misclick, moved window or viewport deletion.

The driver currently propagates a tree read error with a bare `?`, discarding
its existing attempts, last step, pointer observation and bounded tree summary.
Its cleanup then physically releases the button before the error is reported.
That release can commit an edge preview at the threshold point. Without the
earlier context, the trace cannot distinguish zero successfully returned
snapshots from earlier mismatching ones.

Add failure-only context to that exact read error and log the held-drag error
before cleanup release. Preserve the original error, input sequence, deadline,
assertions, cleanup and success path. `attempts` counts successfully returned
snapshots; zero does not prove zero UI passes. This is a diagnostic improvement,
not a confirmed product-root-cause repair or permission to accept stale output.

The [independent source review](2026-11-07-dragback-diagnostic/source-review.md)
checks the exact two-hunk +14/-1 delta and finds no source blocker. Formatting
and whitespace checks pass. Linux-only driver compilation and real physical
execution are not available on this macOS host; fresh Linux CI remains the
validation boundary. The unchanged failed head has also been requested for
one failed-job rerun; no subsequent outcome is claimed by this checkpoint.

The [receipt](2026-11-07-dragback-diagnostic/check.json) binds old/new source
hashes and the exact patch. The directory preserves the original failed run,
complete failed-step log, source/trace reviews, rerun request and pre-edit scope.
All semantic success requirements and time budgets remain unchanged.

The unchanged-head rerun now [passes](https://github.com/Latias94/egui_docking/actions/runs/42323715317/attempts/2).
Its native job finishes the actual editor, physical X11/XTest docking and
retirement requirements, and all six jobs have successful conclusions.
The [result receipt](2026-11-07-dragback-diagnostic/unchanged-rerun-result.json)
and compressed native log preserve this observation. No code, timeout, pointer
sequence or assertion was changed between these attempts. This establishes
intermittency at the observed boundary, not the initiating product or host
cause. The later diagnostic change and retained runtime head still require
their own fresh CI; the earlier green head does not validate them.
