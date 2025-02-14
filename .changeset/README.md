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

# Release Rust Crates

- Run `pnpm release-rust <target-version>` to get the command that can update the version of the rust crates.

```
cargo release version -x --workspace 1.4.1 --exclude loro-rle --exclude loro-delta --exclude loro_fractional_index        
```

- Commit the changes
- Replace the `version` command with `publish`

```
cargo release publish -x --workspace --exclude loro-rle --exclude loro-delta --exclude loro_fractional_index        
```

- Add git tags by replacing `publish` with `tag`

```
cargo release tag -x --workspace --exclude loro-rle --exclude loro-delta --exclude loro_fractional_index        
```

- Push the changes

```
git push && git push --tags
```
