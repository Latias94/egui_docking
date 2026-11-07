# Binary split inputs: retained-frame actual application review

No source/application blocker was found. The actual control's five layout files match the retained binary package's complete base, the observed five live files match its complete tree, and the only core source delta is the three original binary-input hunks in `layout.rs`. This is an exact application/source check, not qualification or a performance result. Root reported qualification session 88731 running when assigning this task; this review does not treat that process as completed or passed.

## Identity and application

- Actual immutable control: `dc689a6004fb2500ad8ee2d89054ff991255c1cb` (`dc689a6`), retained compact frame on e3.
- Application receipt: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/binary-inputs-application.json`, SHA-256 `9efefcbabb457bd17249e1296e253306aa0356742f7d545179934331c3e97f77`.
- Applied package: `/tmp/binary-split-inputs-retained/candidate.patch`, SHA-256 `fc5674ab966b59c7830d9c3c13ec0644d851956cb7941e27c2ff06b438dc93d8`, 2,755 bytes, one runtime file +32/-13, no test changes.
- Package manifest SHA-256: `be12a9ddd0c315ac08a585770626aab0aa9f4b2302f870c3f297a94b69dc5d24`.
- Observed complete `git diff dc689a6 -- crates/dockspace` SHA-256: `c1531ab5b91f97d27cdcf9e487ee5838c5f856dc6c12ab73013d09d49c8d1353`.

| Source relative to `crates/dockspace/src/` | dc689a6 / package base SHA-256 | Observed live / package tree SHA-256 |
| --- | --- | --- |
| `layout.rs` | `908e93dafa9ddb343f645dd77be1011616cd1b330209afd7665e4fe9693cfa2a` | `02a94c4c811b47214f0ca930f69b68bb94d18f25ec9c9d28644102536ad38f21` |
| `layout/projection.rs` | `d82120c1f004511128e65ad7a57f907583c150545ff286acfead2bc4cbfeeed4` | same |
| `layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | same |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | same |
| `layout/topology_tests.rs` | `e7bacde83b6d7c957ee8d749cdaf70fb06ca6f85c5850515169e2bd279b1d729` | same |

Both complete source sets, byte counts, manifest/READY and package artifact bindings match. The base set is `07f1c6efc319077d05910c42c717f6e387e1e2f36a26a52134615c7233f107f7`; tree set is `6aff73c1aa124fa3fe02287016de5fcce41e47ac68d2bfddb64d56f1bb6aca90`. The receipt's per-file actual base/candidate hashes equal these independently checked bytes.

All three old hunk bodies match uniquely and reconstruct the exact tree in memory. The observed Git diff has only `layout.rs`, its complete added/deleted body sequence equals the package patch, and no untracked core source was present. Readback still showed HEAD dc689a6 and all five live source files equal this tree. These statements describe the observed application; later WIP requires a new receipt and cannot inherit this identity automatically.

## Hunk and semantic boundary

The retained patch differs from the previously reviewed three-vector patch (`48f8bf0cafdb29d1771728a2a36b5b8c911c10cb44aa91011a2536e026d7c3f0`) only in hunk header offsets: all original added/deleted lines and context are exact. The entire resulting `prepare_split_projection` function equals that independently reviewed three-vector output. Everything outside that function is byte-identical to the actual frame base. The prior three-vector review (`/tmp/binary-split-inputs-3vec/review.md`, SHA-256 `e331d8a3be7c2dc33c341fe5e238861a8b053b51514cc9d046448f50cc03cf61`) remains frozen; this review additionally binds the actual retained gather parent.

The original slice-based composition, production/delayed child gathers, raw compiler and independent reference remain unchanged e3 inputs outside prepare. Thus this application does not mix in rejected generic childrows or slab. It also adds no unified values, scalar central route, committed-axis bypass, timer, fixture, output, test, or counter change.

The reviewed selection boundary remains concrete: two actual children use two local AxisConstraints in child order; all other arities clear/refill the original constraints Vec. The graph-weight array is used only when both actual child and weight slices have length two; mismatched/other arities clear/refill the complete weight Vec. An actual override is selected before graph weights and stays the full borrowed f64 slice. Neither capacity nor leftover scratch values are interpreted as current inputs. Local arrays are initialized only on their applicable branch and are not stored in a continuation.

The pure constraint mapper and f32-to-f64 conversions preserve fields, bits and order. Minimum/maximum output writes, central selection, splitter geometry, the single fully checked solver and outcomes keep their original order. The solver, arithmetic, validators and compact continuation are outside this function's changes and retain the actual frame base bytes. There is no alternate binary solver, new allocation container, weakened cardinality check or precision narrowing.

## Limits

The applied source preserves current tests and the existing mixed-arity/raw-cardinality, override precision, central, late-error/old-output and complete-oracle gates. Source equality does not turn those tests into executed results. No result from the running qualification session was inferred or copied into this review.

Local arrays, branches and code layout may offset avoided temporary Vec filling; nonbinary scratch, eligibility work, fresh leaf input, checked numerical work and full output remain. No stack/ABI size, allocator saving, speedup or target gain is established. This check covers actual source application after the retained frame; Root still owns ordinary qualification, timing, retention and fresh runtime CI.

Only `/tmp/binary-split-inputs-retained-review.md` was written for this task. No Cargo, compiler, formatting, tests, timing, runner, archived script, network or child agent was used. Repository, fork, cache, candidates and existing receipts were not modified.
