# Preserve pending viewport inspection paint

The native coordinator failed physical X11/XTest smoke in
[run 42285373560](https://github.com/Latias94/egui_docking/actions/runs/42285373560)
after the guide-control drag-back source press. The inspection response read
reached its existing approximately three-second driver deadline. The old error
did not retain its Request identity, so the historical log cannot distinguish
the ROOT checkpoint from the subsequent child query or establish unique cause.

The pinned inspection plugin had a concrete scheduling gap: it requested the
selected child only when starting a tree request, while pending work kept
requesting ROOT. Native output admission can acknowledge a child repaint and
defer its UI pass. A later ROOT pass therefore left the child request pending
without re-arming its paint obligation.

Published egui commit
[`f94ee58b3914de7ad71dab36919b329ef36b4035`](https://github.com/Latias94/egui/commit/f94ee58b3914de7ad71dab36919b329ef36b4035)
adds selected-child `RequestPaintWhileHidden` scheduling for existing
`AwaitViewportTree` requests. It preserves ROOT inventory progress, Settle,
fresh target output, Removed/Unknown responses and exactly-once completion.
Completed requests stop scheduling. Unrelated children are not requested.
The actual native dependency, lockfile and CI checkout all select this commit.
CI also runs the seven plugin regression tests against that checkout.

The causal test on the unchanged `60c40304` runtime failed specifically when
a later ROOT pass should re-arm an acknowledged child wake without running
that child. Initial startup repaint coalescing was acknowledged before request
submission. The final runtime passes both that test and a real Context removal
test which checks actual output disappearance, no later resurrection, Unknown
for the retired identity and single replies. Fresh AccessKit Label text is
checked through the pinned implementation's `value()` property. Full plugin
and all-feature/all-target tests each pass 7/7; formatting passes. Independent
source review binds source SHA-256
`902fae4ba28ace39c3cc30dad3afa8cd7124ab733b8a9b6fc3a8fe97ab74a447`
and exact diff SHA-256
`34fcbaa04ec8446c00ba50d134e2c4ff05515e66c641c5fd1c727e4dc1e3461c`.

The physical driver's response errors now retain Request identity. A non-root
source's existing ROOT checkpoint logs its start, successful semantic step or
failure. No deadline, retry policy, physical input or acceptance assertion is
changed. Current-head Linux physical smoke remains pending; plugin tests do
not establish native X11 scheduling success or retrospectively prove the old
timeout's cause.

The actual pinned native smoke workspace passes `cargo check --all-targets
--locked`, native formatting, and all 29 physical-driver unit tests on macOS.
Linux-only transport/scenario execution is left to CI. The
[evidence archive](2026-11-07-viewport-inspection/evidence.tar.xz) retains the
historical log, source reviews, corrected causal red, both earlier setup/property
failures, exact qualified patch, published pin and local qualification receipts.
All archived members were read back and compared with their original bytes.
