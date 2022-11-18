# Glossary

| Name                    | Meaning                                                              | Note                                                                                                                                                         |
|-------------------------|----------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------|
| DAG                     | Directed Acyclic Graph                                               | The changes in Loro form a DAG. The relationship is expressed by deps field.                                                                                 |
| [RLE](#rle)             | Run Length Encoding                                                  | We not only use it on the encoding, but also in the vec and tree.                                                                                            |
| Change                  | A merged batch of ops                                                |                                                                                                                                                              |
| Frontiers               | The DAG nodes that no one has dependencies on them                   | They can be represented by a series of id                                                                                                                    |
| Op                      | Operation                                                            |                                                                                                                                                              |
| State (of a Container)  | The                                                                  | In the code, the state most refers to the visible state to users. For example, the text string of a text container, the key value pairs in the map container |
| Effect (of an Op)       | How the op affect the state                                          | We use CRDT to calculate the effect (obviously)                                                                                                              |
| Tracker                 | A special data structure to calculate the effects                    |                                                                                                                                                              |
| [Container](#container) | A unit of CRDT data structure                                        |                                                                                                                                                              |
| Content                 | Content of an Op                                                     |                                                                                                                                                              |
| ID                      | A global unit id for an Op                                           | It has the structure of {clientId, counter}                                                                                                                  |
| Counter                 | The second field inside ID                                           |                                                                                                                                                              |
| Lamport                 | [Lamport timestamp](https://en.wikipedia.org/wiki/Lamport_timestamp) |                                                                                                                                                              |
| Span                    | A series of continuous things                                        |                                                                                                                                                              |
| Causal Order            | The order of happen-before relationship                              | The DAG expresses the causal order of the changes                                                                                                            |
| VV                      | [Version Vector](https://en.wikipedia.org/wiki/Version_vector)       |                                                                                                                                                              |


### RLE

We not only use RLE on the encoding, but also in the vec and tree.

i.e. the elements can be merged, sliced and have lengths. We call the element with length of 1 as atom element (cannot be sliced).

We use a `Rle` trait to express it. So `Op`, `Change`, elements inside `RleTree` all implement `Rle`. 
This gives us a compact way to represent the mergeable elements.

### Container

Each op is associated with one container. Different CRDT algorithms use different types of containers. There are hierarchical relationship between containers, but they cannot affect each other
