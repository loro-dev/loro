# Prototype test results

Debug build of the prototype on main `ea1d383e`, 2026-11-06.

`cargo test -p loro --tests --no-fail-fast`:

| Configuration | Binaries passing | Tests passing | Failing targets |
| --- | --- | --- | --- |
| `LORO_PROTO_NO_NEAR=1` | 76 | 834 | none |
| `LORO_PROTO_NO_NEAR=1 LORO_PROTO_BLOCK_CAP=8` | 76 | 834 | none |
| default, no shallow guard | 71 | 789 | `movable_list_invalid_ops`, `shallow_checkout_equal_value`, `shallow_root_critical`, `shallow_snapshot_deleted_containers`, `unparsable_history` |
| default, shallow guard | 75 | 828 | `unparsable_history` |

`LORO_PROTO_BLOCK_CAP=8 cargo test -p loro-internal --lib`: 477 passed, 0 failed, 4
ignored. This ran before the shallow guard was added. `loro-kv-store`: 31 passed.

The shallow-guard failure was not diagnosed beyond the target name. The likely cause
is in `context/memory-analysis.md`, and it has not been confirmed by a stack.
