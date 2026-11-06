# Short source trace: OutputsInFlight after retained-facts qualification

Date: 2026-11-06. Read-only follow-up during the frozen final qualification. No source edit, Cargo, test, benchmark, profiler, Git write, or new performance attribution was performed. Timing ranges were supplied by the root agent; this note independently checks only the source path.

## Finding

**Yes: the confirmed-current-candidate eligibility filter prevents selection rebinding in the measured in-flight update and leads to the complete compiler. However, this workload also changes actual measurement inputs between adjacent updates. Removing that filter alone does not justify reusing the newest candidate's whole facts.**

Trace:

1. `frame_cost.rs:538-546` prefills `older_outputs` updates outside the measured windows, then times one more update. Prefill uses `Scenario::OutputsInFlight`; none of these outputs is reported Presented until the later `settle()` at `:469-484`. **This round's qualified headless timing and allocation runs explicitly use `--in-flight 1`: one prefill update followed by one timed update, with two outputs pending before settlement.** The source default is three older outputs; it was not this round's measurement parameter.
2. Every such update selects alternating Item2/Item3 (`:352-370`) **and** alternates bounds width 16,384/16,448 plus uniform tab intrinsic 64/72 (`:408-429`). It executes the complete recording program, confirms painting, commits, and retains the owned sink/output permit in `pending` (`:439-466`). This is selection plus measurement change plus output retention, not a fixed-geometry selection with extra receipts.
3. `advance_selection_revision` can still retain/reissue the structural requirement inventory. Its geometry preparation is narrower: `presentation_authority.rs:255-276` requires `ready.paint_fallback().output_ticket() == ready.output_ticket()`. After an unobserved replacement is emitted, the current candidate is newer than the confirmed fallback, so this check returns no `SelectionRebind`.
4. Manifest reconciliation carries only the confirmed paint fallback across the changed requirement (`scene/lifecycle.rs:1131-1209`, `into_confirmed_paint_fallback` at `:226`). Preparation then has a Stale surface, so the ordinary Ready-plan reuse route misses. With no `selection_rebind`, the specialized route at `surface_contribution.rs:346-399` also misses, and execution reaches `compile_surface_plan(...).finalize()`.
5. Even if the newest unconfirmed candidate were admitted as a fact source, `same_values` at `surface_contribution.rs:363` would reject the next update's alternating bounds/intrinsics. Those changes genuinely alter the inputs of the whole geometry/program/hit representation.

With this round's explicit single prefill, two consecutive alternating updates occur per sample: the prefill changes to the opposite measurement profile, and the timed update returns to the prefill-start/last-settled profile while differing from the immediately preceding unconfirmed candidate. The source default of three prefills would instead produce four alternating updates and the same profile-return parity; both prefill counts are odd. Thus an older retained fact set can have matching inputs even though the newest candidate does not. This is a reuse opportunity in representation/eligibility, not permission to reuse stale output authority. Other `--in-flight` values can change the parity; no general hit rate is inferred here.

## Necessary constraints versus the current bounded choice

Necessary: retained/emitted outputs keep their own source binding and observation state; unpresented or stale outputs cannot authorize input; current ticket/config/policy/coordinates/native lifecycle must be checked; current complete measurements and actual minima/reveal must match any reused whole fact set. Changed measurement inputs cannot be ignored.

Bounded choice: using only a current candidate that is also the confirmed paint fallback as the source of reusable facts. Immutable fact reuse does not itself require that the facts have been Presented. A later extension could choose an exact compatible retained fact owner while keeping all emission/interaction authority separate. The existing conservative fallback survival rules should not be weakened merely to obtain such a cache source.

## Next priority and limits

Keep this freeze intact. For a later small investigation, first distinguish in-flight selection with fixed measurements from this mixed workload using counters/source tracing outside formal qualification. Then evaluate a bounded compatible-fact-source choice over already retained owners, with complete answer checks and fresh source/output bindings. If inputs differ from every retained owner, the next question is genuine changed-geometry construction cost; it is a separate experiment, not a reason to expand the current guard blindly or build a generic history/dependency framework.

The reported 8.30–8.41 ms in-flight update is **not** a proved lower bound, isolated ledger/settlement cost, unavoidable neutrality/Rust cost, or the residual cost of the same task as the 1.79–1.80 ms fixed-measurement selection. No subtraction or phase budget follows from those ranges. This trace establishes branch selection and distinct dependencies only; it predicts no additional speedup and does not partition the measured time.
