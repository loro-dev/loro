# Container Value Cache (InnerStore)

Verified against code 2026-09-04.

## The funnel

Every per-container read — handler reads like `map_get`, `list_get`,
`map_len`, text reads, `contains_id` — goes through
`InnerStore::with_container_for_read`
(`crates/loro-internal/src/state/container_store/inner_store.rs`). On a KV
miss the wrapper is created from the KV bytes and, if the read decoded the
value (`ContainerWrapper::has_cached_value`), inserted into
`InnerStore.store`. Bulk paths (`toJSON`, deep values, snapshot export) use
`try_get_value_ephemeral` / `try_with_container_for_ephemeral_read`, which
deliberately leave no residue.

## The bounded cache (loro-dev/loro#1092)

Before 2026-09, a decoded value stayed pinned in `store` until the doc was
dropped: ~4 KB per container ever read, superlinear growth on
container-by-container walks, wasm32 trap at 4 GiB near one million
containers.

`track_value_cache` now bounds the number of cached decoded values to
`MAX_CACHED_CONTAINER_VALUES` (2048; 16 under `cfg(test)`) with a
second-chance FIFO (`value_cache_queue` + per-wrapper
`in_value_cache_queue` / `value_cache_referenced` bits on
`ContainerWrapper`). The second-chance bit exists so a hot ancestor (e.g. the
root list re-read once per item during a deep walk) survives eviction passes.

## Eviction safety contract

Only `flushed && Lazy && value.is_some()` wrappers are evictable
(`ContainerWrapper::is_evictable_cached_value`): a flushed lazy wrapper is a
pure cache over the KV bytes, so dropping it loses nothing. Once a container
is mutated, `get_state_mut` converts the wrapper to `State` and clears
`flushed`; `State` wrappers are never evicted, so unflushed edits can never be
lost. Evicted entries are re-created from KV on the next read — eviction only
costs a re-decode.

Because of eviction, `store` is strictly a cache over `kv`: all lookup paths
(`get_or_insert_with`, `ensure_container`, `get_mut`, `with_container_for_read`,
the ephemeral reads, `contains_id`) fall back to `kv` on a `store` miss
regardless of `load_state`. Do not reintroduce `load_state != AllLoaded` gates
on those fallbacks — `load_all`/`decode_twice` (GC snapshot import) put the
store in `AllLoaded` mode, and evicted entries must stay reachable there.

## Remaining pins

- Tree containers materialize a full `State` on first value read
  (`decode_value_from_bytes` returns `decoded_state` for trees), so tree walks
  still pin state; they are not covered by the bound.
- `decode_twice`/`load_all` insert one lightweight lazy shell per container
  into `store` (no decoded value); those shells are small and stay.

## Tests

- `crates/loro-internal/src/state/container_store.rs` (`mod test`):
  `handle_reads_bound_cached_container_values`,
  `evicted_containers_stay_readable_and_editable`,
  `evicted_entries_stay_readable_when_all_loaded`.
- `crates/loro-wasm/tests/walk_mem.test.ts`: ~100k-container handle walk keeps
  `process.memoryUsage().external` within a small multiple of the `toJSON()`
  delta (fails at ~286 MB on the pre-fix build, ~13 MB after).
