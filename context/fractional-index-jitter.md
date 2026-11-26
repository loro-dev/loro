# Fractional indexes with jitter

Verified against code 2026-11-14.

Tree sibling positions are `FractionalIndex` byte strings compared
lexicographically (`crates/fractional_index/src/lib.rs`). A generated index is
`base ++ TERMINATOR (0x80)`; with `enable_fractional_index(jitter)` it is
`base ++ 0x80 ++ jitter random bytes` (`src/jitter.rs`). So without jitter every
index ends with `0x80`, but a jittered one ends with a random byte, possibly
`0xFF` or `0x00`.

- `new_between` compares the last byte of the shorter index too; without
  jitter that byte is the terminator, so outputs for unjittered indexes are the
  same as before loro-dev/loro#1211. It returns `None` only when
  `left >= right`, or when `right` is `left` followed by zero bytes: nothing of
  the form `... ++ 0x80` sorts before an all-zero suffix.
- `new_after` on an all-`0xFF` suffix returns it unchanged: the appended
  terminator makes the result sort after it.
- The same rules are ported to `packages/fractional-index/src/index.ts`
  (checked against `tests/fixtures/rust-golden.json`, regenerated with
  `pnpm fixtures:generate` from `examples/npm_fixture.rs`) and to
  `loro-js/src/runtime/fractional-index.ts`.
- The setting (`FractionalIndexSetting`: disabled or a jitter) lives in the
  `TreeState`, not in the doc `Configure`. Snapshot import replaces the states,
  so `decode_snapshot_inner` (`encoding/fast_snapshot.rs`) reads the non-default
  settings of decoded trees first and reapplies them afterwards, also when the
  import fails. `fork()` / `fork_at()` copy them with
  `LoroDoc::copy_tree_fractional_index_settings_to`; the fork gets a new RNG,
  not a copy of the source's (loro-dev/loro#1288).
- The jitter RNG is a `StdRng` seeded from the tree state's `peer_id`
  (`TreeState::new` and `enable_generate_fractional_index`), so different peers
  draw different bytes and a fixed peer id gives reproducible positions. That
  `peer_id` is copied when the state is created; a later `set_peer_id` does not
  reseed it (loro-dev/loro#1289).
- `NodeChildren::generate_fi_at{,_jitter}` (`crates/loro-internal/src/state/tree_state.rs`)
  still unwrap `new_between`, so the `left ++ 00…` case would still panic.

## Tests

`crates/fractional_index/src/lib.rs` (unit tests, random jittered pairs),
`crates/loro/tests/tree_fractional_index_jitter.rs`,
`crates/loro/tests/tree_fractional_index_setting.rs`,
`loro-js/tests/fractional-index.test.ts`, the npm package golden test.
