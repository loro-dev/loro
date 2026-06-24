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

To also launch each production-built app in Chromium and verify `doc.toJSON()`
returns `{ map: { text: "mergeable-smoke" } }` in a real browser:

```sh
pnpm --dir examples/bundler-smoke-tests run test:browser
```

This command installs Playwright's Chromium browser if needed.

To test dev-server runtime behavior in Chromium:

```sh
pnpm --dir examples/bundler-smoke-tests run test:dev
```

To run both production browser runtime and dev-server smoke tests:

```sh
pnpm --dir examples/bundler-smoke-tests run test:smoke
```

To test an already-published package instead of the local workspace build:

```sh
LORO_SMOKE_PACKAGE=loro-crdt@1.12.1 pnpm --dir examples/bundler-smoke-tests run test:fast
```

## Matrix

- `vite5`, `vite6`, `vite7`, `vite8`: Vite production builds with bare
  `import "loro-crdt"`.
- `vitest-node`: Vitest Node runtime with bare `import "loro-crdt"`, covering
  package conditional exports as seen by test runners.
- `vite5-dev`, `vite6-dev`, `vite7-dev`, `vite8-dev`: Vite dev servers with
  bare `import "loro-crdt"` and WASM/top-level-await plugins enabled.
- `vite5-web-mirror-dev`: `import "loro-crdt/web"` plus `loro-mirror`, which
  verifies that peer packages importing bare `loro-crdt` do not reintroduce a
  broken dev-server WASM path.
- `rolldown-vite`, `rolldown-vite-dev`: production build and dev server against
  Rolldown's Vite package.
- `webpack5`, `webpack5-dev`: Webpack production build and dev server.
- `rsbuild2`, `rsbuild2-dev`: Rsbuild production build and dev server.
- `rspack2`, `rspack2-dev`: Rspack production build and dev server.
- `parcel2`, `parcel2-dev`: Parcel production build and dev server.
- `esbuild-default-copy`: bare `import "loro-crdt"` plus a post-build copy of
  `browser/loro_wasm_bg.wasm` next to the emitted JS bundle.
- `rollup-default-copy`: bare `import "loro-crdt"` plus the same post-build copy.
- `esbuild-base64`: `import "loro-crdt/base64"` with no external WASM asset.
- `rollup-base64`: `import "loro-crdt/base64"` with no external WASM asset.
- `next16-turbopack`: Next 16 default Turbopack production build with bare
  `import "loro-crdt"`.
- `next16-webpack`: Next 16 production build with `--webpack` and
  `import "loro-crdt/base64"`.
- `next16-turbopack-dev`: Next 16 default Turbopack dev server with bare
  `import "loro-crdt"`.
- `next16-webpack-dev`: Next 16 dev server with `--webpack` and
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
