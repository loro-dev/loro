# loro-crdt-map

This package publishes the WebAssembly source maps for [`loro-crdt`](https://www.npmjs.com/package/loro-crdt).

The source maps are split by target (`bundler`, `nodejs`, `web`) and are intended to be referenced from the compiled
WebAssembly modules that ship with the main package. They are hosted on unpkg so that they can be loaded on demand
without bloating the main npm distribution.

Most users should not need to depend on this package directly. The published files primarily exist so that browsers and
other tooling can download the source map specified in the `sourceMappingURL` metadata embedded inside
`loro_wasm_bg.wasm`.

