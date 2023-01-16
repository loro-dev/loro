# Loro Framework

The layers of loro:

- Loro Framework(this crate): It is agnostic to the op content. Thus intermediary nodes can work on this layer. It can handle apply updates, encode updates, hash and authentication. 
  - The change content is byte stream in this layer (may be encrypted).
  - If it's encrypted, the public key is accessible for the intermediary nodes
- Encoding & Decoding layer(this crate). This layer also handle encryption and decryption
- CRDT Framework(loro-internal crate): It is agnostic to the specific CRDT algorithms. We can register different CRDT algorithm upon it.
- Specific CRDT Algorithm(loro-text, loro-array crate).
