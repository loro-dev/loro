---
"loro-crdt": patch
---

Fix published sourcemap `sources` pointing outside the package. The rollup TypeScript plugin's emitted maps were resolved by rollup against the `.ts` source directory, leaving `../../index.ts` in the published `sources`. Vite/Vitest warned about source files escaping the package. The sourcemap now resolves inside the package and `sourcesContent` is included so debuggers don't need to fetch the TypeScript source.
