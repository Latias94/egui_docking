# Documentation

## Start here

- [Project overview](../README.md): what Dockspace does and which integration to use.
- [Examples](EXAMPLES.md): runnable core, egui and native starting points.
- [Core overview](../crates/dockspace/README.md): stable identities, layout and host responsibilities.
- [egui overview](../crates/egui_dockspace/README.md): host choices and pane rendering.
- [Native overview](../crates/egui_dockspace_native/README.md): experimental window integration.

## Integration guides

- [Engine host guide](../crates/dockspace/docs/guide.md): frame orchestration,
  measurements, ordered input, output settlement, close decisions and persistence.
- [egui guide](../crates/egui_dockspace/docs/guide.md): complete-output and callback
  integration, focus, background work, style and configuration.
- [Adapter testing](ADAPTER_TESTING.md): host responsibilities and reusable
  framework-neutral scenarios.
- [Platform support](PLATFORM_SUPPORT.md): capabilities and qualified native profiles.

## Development and evidence

- [Contributing](../CONTRIBUTING.md): repository layout and local checks.
- [Maintaining](MAINTAINING.md): package/release order and proposed repository naming.
- [Validation](VALIDATION.md): dated results and the limits of their claims.
- [Third-party sources](../THIRD_PARTY.md): prior art and fixture attribution.

`plans/`, `audits/`, `performance/` and `knowledge/` contain design history,
engineering notes and qualification evidence. They are supporting material;
the crate guides and API documentation describe integration. The
[engine-first migration notes](ENGINE_FIRST_MIGRATION.md) and
[completion audit](COMPLETION_AUDIT.md) remain historical references.
