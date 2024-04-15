# Release new versions of loro-wasm and loro-crdt

- Run `pnpm changeset` in the root of the repository. The generated markdown files in the .changeset directory should be committed to the repository.
- Run `git cliff -u | pbcopy` to generate the changelog and copy it. Then edit the new changelog file.
- Create PR and merge into main. The GitHub Action will create another PR that once be merged new versions of specified npm packages will be published.

