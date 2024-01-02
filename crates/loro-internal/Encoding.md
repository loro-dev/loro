# Encoding Schema

## Header

The header has 22 bytes.

- (0-4 bytes) Magic Bytes: The encoding starts with `loro` as magic bytes.
- (4-20 bytes) Checksum: MD5 checksum of the encoded data, including the header starting from 20th bytes. The checksum is encoded as a 16-byte array. The `checksum` and `magic bytes` fields are trimmed when calculating the checksum.
- (20-21 bytes) Encoding Method (2 bytes, big endian): Multiple encoding methods are available for a specific encoding version.

## Encode Mode: Updates

In this approach, only ops, specifically their historical record, are encoded, while document states are excluded.

Like Automerge's format, we employ columnar encoding for operations and changes.

Previously, operations were ordered by their Operation ID (OpId) before columnar encoding. However, sorting operations based on their respective containers initially enhance compression potential.

## Encode Mode: Snapshot

This mode simultaneously captures document state and historical data. Upon importing a snapshot into a new document, initialization occurs directly from the snapshot, bypassing the need for CRDT-based recalculations.

Unlike previous snapshot encoding methods, the current binary output in snapshot mode is compatible with the updates mode. This enhances the efficiency of importing snapshots into non-empty documents, where initialization via snapshot is infeasible. 

Additionally, when feasible, we leverage the sequence of operations to construct state snapshots. In CRDTs, deducing the specific ops constituting the current container state is feasible. These ops are tagged in relation to the container, facilitating direct state reconstruction from them. This approach, pioneered by Automerge, significantly improves compression efficiency.
