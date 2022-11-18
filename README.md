<p align="center">
  <a href="https://loro.dev">
    <picture>
    </picture>
    <h1 align="center">Loro 🦜</h1>
  </a>
</p>

Loro is a fast [CRDT](https://crdt.tech/) framework with built-in end-to-end encryption ability. 

It provides a set of data structures that can automatically sync without any conflict. With end-to-end encryption addon, all data can be encrypted without losing the ability to collaborate with the others. It aims to be the engine for building [local-first software](https://www.inkandswitch.com/local-first/).


# Why Loro

- 🚀 It is pretty fast
- 🔒 [WIP] Security built-in
- 💻 Syncing data made easy
- 📜 Preserve all history with low overhead
- 🪐 [WIP] Time travel the history in milliseconds

Loro supports a variety of data structures and CRDT algorithms. 

- It supports the most used `List`, `Map` and `Text`. 
- [TODO] [Peritext](https://www.inkandswitch.com/peritext/) for fine-grind rich text operations
- [TODO] [Moveable Tree]() for directory-like moving operations 
- [WIP] Super fast version checkout and undo/redo 


# Credits

- Automerge for its columnar encoding algorithm
- Yjs for the efficient algorithm of merging blocks
- Diamond-types for its idea of low-overhead merging algorithm
- Ink & Switch for Local-first Software and Peritext

