# Release a new version of loro-wasm and loro-crdt

- Run `deno task release-wasm` to build the WASM
- Run `pnpm changeset` in the root of the repository. The generated markdown files in the .changeset directory should be committed to the repository.
- Run `git cliff -u | pbcopy` to generate the changelog and copy it. Then edit the new changelog file.
- Run `pnpm changeset version`. This will bump the versions of the packages previously specified with pnpm changeset (and any dependents of those) and update the changelog files.
- Run `pnpm install`. This will update the lockfile and rebuild packages.
- Commit the changes.
- Run `pnpm changeset publish`. This command will publish all packages that have bumped versions not yet present in the registry.
