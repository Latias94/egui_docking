# Isolate native forks inside the monorepo

Core, adapters and conformance scenarios evolve together in one repository.
Independent consumer workspaces test their public boundaries without requiring
separate releases for each architectural change.

The public egui adapter uses official dependencies. Experimental native hosting
needs additional output, input and window-lifecycle seams, so its pinned forks
live in a separate Cargo workspace and lockfile. Putting those patches in the
root workspace would hide incompatibilities for ordinary egui consumers. Native
remains unpublished until its dependency and platform support have a separate
release decision. See [maintaining](../MAINTAINING.md).
