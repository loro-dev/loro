# Release new versions of loro-wasm and loro-crdt

- Run `pnpm changeset` in the root of the repository. The generated markdown files in the .changeset directory should be committed to the repository.
- Run `git cliff -u | pbcopy` to generate the changelog and copy it. Then edit the new changelog file.
- Create PR and merge into main. The GitHub Action will create another PR that once be merged new versions of specified npm packages will be published.


# Release Manually

- Run `pnpm changeset` in the root of the repository. The generated markdown files in the .changeset directory should be committed to the repository.
- Run `git cliff -u | pbcopy` to generate the changelog and copy it. Then edit the new changelog file.
- Run `nr release-wasm` to build the WASM crate
- Run `pnpm changeset version` to update the version info
- Commit the changed files
- Run `pnpm changeset publish` to publish the packages to npm
- `git push && git push --tags`
