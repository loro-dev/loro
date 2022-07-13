# Loro Framework

The layers of loro:

- Loro Framework(this crate): It is agnostic to the op content. Thus intermediary nodes can work on this layer. It can handle apply updates, encode updates, hash and authentication.
- CRDT Framework(loro-core crate): It is agnostic to the specific CRDT algorithms. We can register different CRDT algorithm upon it.
- Specific CRDT Algorithm(loro-text, loro-array crate).
