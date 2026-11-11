# Keep logical surfaces separate from native windows

A surface identifies docking content; a native window identifies an OS resource
with its own lifetime. An externally owned Root window may compose several
surface placements, while a runtime-owned Child has one whole-client placement.
Treating those identities as interchangeable would prevent shared-window hosts
and correlate close, capture and retirement with the wrong resource.

Placements publish as a complete atomic roster. Close and destruction act on the
physical window's full roster; hit, output and pane-focus authority remain exact
to each placement. Child creation retains durable source ownership until all
required fresh first-live outputs are presented. Preparation or visibility alone
cannot commit that transfer. Root continuation preserves the external resource
while replacing logical bindings, with fresh presentation required afterward.

This adds explicit mapping and lifetime obligations to the host, but supports
multiple UI frameworks without embedding a window manager in the core. Missing
platform facts remain Unknown instead of being inferred from cached geometry.
See [platform support](../PLATFORM_SUPPORT.md).
