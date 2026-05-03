# Loro Bundler Smoke Tests

This package checks that the published `loro-crdt` JavaScript/WASM package can be
imported by common browser bundlers.

The tests create one temporary project per bundler under `.tmp/`, install the
requested bundler version there, build a tiny app, and inspect the output for the
WASM packaging shape that matters for Loro.

## Usage

Build `crates/loro-wasm` first so `bundler/`, `browser/`, `base64/`, `nodejs/`,
and `web/` artifacts exist:

```sh
pnpm release-wasm
```

Then run:

```sh
pnpm --dir examples/bundler-smoke-tests run test:fast
```

Run Next.js/Turbopack separately because it is heavier:

```sh
pnpm --dir examples/bundler-smoke-tests run test:next
```

To test an already-published package instead of the local workspace build:

```sh
LORO_SMOKE_PACKAGE=loro-crdt@1.12.1 pnpm --dir examples/bundler-smoke-tests run test:fast
```

## Matrix

- `vite5`, `vite6`, `vite7`, `vite8`: bare `import "loro-crdt"`.
- `rolldown-vite`: bare `import "loro-crdt"` against Rolldown's Vite package.
- `webpack5`: bare `import "loro-crdt"`.
- `rsbuild2`: bare `import "loro-crdt"`.
- `rspack2`: bare `import "loro-crdt"`.
- `parcel2`: bare `import "loro-crdt"`.
- `esbuild-default-copy`: bare `import "loro-crdt"` plus a post-build copy of
  `browser/loro_wasm_bg.wasm` next to the emitted JS bundle.
- `rollup-default-copy`: bare `import "loro-crdt"` plus the same post-build copy.
- `esbuild-base64`: `import "loro-crdt/base64"` with no external WASM asset.
- `rollup-base64`: `import "loro-crdt/base64"` with no external WASM asset.
- `next16-turbopack`: Next 16 default Turbopack production build with bare
  `import "loro-crdt"`.
- `next16-webpack`: Next 16 production build with `--webpack` and
  `import "loro-crdt/base64"`.

## Notes

Vite and Webpack understand `new URL("./asset", import.meta.url)` as an asset
reference and emit the `.wasm` file automatically. Plain esbuild and plain Rollup
do not do that by themselves, so they should either use `loro-crdt/base64` for a
single-file bundle with no top-level await, or copy `browser/loro_wasm_bg.wasm`
to the output directory as a build step.

Next 16 Turbopack handles the default browser entry in this smoke test. Next 16's
Webpack build was observed to resolve Loro's bundler entry instead of the package
`browser` remap, so this matrix tests the documented `base64` entry for that
mode.
