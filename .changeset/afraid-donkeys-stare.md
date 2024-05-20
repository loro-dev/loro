---
"loro-wasm": minor
"loro-crdt": minor
---

Movable Tree Children & Undo

#### 🐛 Bug Fixes

- Refine error message on corrupted data (#356)
- Add MovableList to CONTAINER_TYPES (#359)
- Better jitter for fractional index (#360)

#### 🧪 Testing

- Add compatibility tests (#357)

#### Feat

- Make the encoding format forward and backward compatible (#329)
- Undo (#361)
- Use fractional index to order the children of the tree (#298)

#### 🐛 Bug Fixes

- Tree fuzz sort value (#351)
- Upgrade wasm-bindgen to fix str free err (#353)
