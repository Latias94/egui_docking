# Maintaining Dockspace

## Names and repository structure

The product name is **Dockspace**. The recommended GitHub repository name is
`dockspace`: the primary library is UI-framework-neutral, while egui is an
adapter. `egui_docking` suggests that the whole project is an egui widget.
`dockspace-rs` is a fallback if the shorter name is unavailable.

Keep these names distinct:

| Name | Role |
| --- | --- |
| `dockspace` | Core crate and proposed repository slug |
| `egui_dockspace` | Official egui adapter crate |
| `egui_dockspace_native` | Experimental unpublished native adapter |
| `dockspace_host_conformance` | Shared test-scenario crate |

Keep one repository. Core, adapters and conformance scenarios currently evolve
together, and independent consumer workspaces already test their separation.
Splitting repositories would add release coordination before it has a concrete
benefit. Keep the official and fork-backed Cargo workspaces separate.

Use a concrete GitHub About description, such as "Renderer-neutral docking for
Rust editor UIs, with egui adapters." Suggested topics are `rust`, `docking`,
`egui`, `gui` and `editor`. Point the homepage at a maintained user guide or demo
once one is hosted; avoid linking an internal plan as the product homepage.

## Proposed GitHub rename

The current remote remains `Latias94/egui_docking`. The rename is a pending owner
decision, not an operation performed by the documentation change. Do it before
the first registry release if accepted:

1. Rename the GitHub repository to `dockspace` and confirm the new URL resolves.
2. Update each maintained checkout's `origin`, package `repository` fields,
   current source-install instructions and active CI/documentation references.
3. Confirm the default branch, Actions, branch/ruleset configuration and external
   integrations still point to the intended repository. Check Pages separately
   if it is enabled later.
4. Keep the old repository name unused so GitHub's redirects continue to work.
   Historical audit URLs can remain historical; active product links should use
   the new location.

GitHub redirects ordinary web and Git access after a rename, but Pages and
references to a repository-hosted Action have separate behavior. Follow
[GitHub's rename documentation](https://docs.github.com/en/repositories/creating-and-managing-repositories/renaming-a-repository).
Changing the local directory name is optional. A repository rename does not
rename crates, Rust imports or package versions.

## Documentation ownership

- The root README answers what the project does, how to try it and which host
  to choose. Keep it short and avoid dated implementation history.
- Crate READMEs describe installation, first use and customization. Packaged
  guides retain the complete host contracts.
- `docs/EXAMPLES.md` owns the run-command catalog and explains example limits.
- `CONTRIBUTING.md` owns development commands; CI owns the executable gates.
- `docs/VALIDATION.md` indexes dated evidence. Qualification results do not
  silently expand a platform or performance claim.
- Keep build outputs and caches untracked. Preserve existing qualification
  artifacts unless a separate change deliberately reorganizes them.

## First registry release

Both public crates are currently source-only. Do not advertise a crates.io
installation command or docs.rs version badge until that release exists.

1. Finalize naming and package metadata, include the declared license texts in
   each distributable package, check third-party notices, and write user-facing
   release notes for the selected source revision.
2. Run the CI gates and applicable native qualification. Check both default and
   persistence consumers and keep the documented platform limits accurate.
3. Inspect package contents. `dockspace` must include its command example and
   host guide; `egui_dockspace` must include its application examples and guide.
4. Publish `dockspace` first. Then verify and publish `egui_dockspace` against
   that exact released core version. A local extracted-pair test does not prove
   registry publication.
5. Add installation/version links and release notes after verifying the registry
   packages and generated documentation. Keep the native adapter unpublished
   until it has a separate release decision.

Useful package inspection commands:

```sh
cargo package -p dockspace --all-features --locked --list
cargo package -p egui_dockspace --all-features --locked --list
```

Package verification is a separate step from listing. Before registry release,
the adapter needs the matching unpublished core through the existing extracted
package workflow; otherwise Cargo may resolve an absent or mismatched registry
version. The [validation index](VALIDATION.md) retains the previous package-pair
qualification and its source scope.
