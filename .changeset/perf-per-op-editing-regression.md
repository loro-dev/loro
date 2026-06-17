---
"loro-crdt": patch
"loro-crdt-map": patch
---

Recover two per-operation editing slowdowns regressed since 1.11.

Both are constant-factor regressions on the per-op (auto-commit) editing path
introduced by the lazy-snapshot work in #985, measured against the 1.11.1
release.

1. Every `MapHandler`/`ListHandler`/`MovableListHandler` insert validated its
   value with `ensure_no_regular_container_value`, which heap-allocated a `Vec`
   on each call even for scalar values (the common case). A scalar fast-path now
   skips the allocation and traversal entirely. `map create 10^4 key`:
   ~19.4ms -> ~10.7ms.

2. The per-op text bounds check (`TextHandler::len`/`len_unicode`/`len_utf16`)
   took two `DocState` locks — one to check whether the container state was
   decoded, then another to query the length. These are now consolidated into a
   single `DocState::get_text_len` that takes one lock and one container-store
   lookup. The lazy-snapshot memory behavior is preserved: a still-lazy
   container reads its cached length metadata without materializing the full
   richtext state. `bench_text B4 apply` (per-op text editing): ~389ms -> ~352ms.
