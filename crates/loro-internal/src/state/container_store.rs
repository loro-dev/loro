use super::{ContainerCreationContext, State};
use crate::arena::LoadAllFlag;
use crate::sync::{AtomicU64, Mutex};
use crate::{
    arena::SharedArena, configure::Configure, container::idx::ContainerIdx,
    utils::kv_wrapper::KvWrapper, version::Frontiers,
};
use bytes::Bytes;
use inner_store::InnerStore;
use loro_common::{ContainerID, InternalString, LoroResult, LoroValue};
use std::sync::Arc;

pub(crate) use container_wrapper::ContainerWrapper;

mod container_wrapper;
mod inner_store;

/// Encoding Schema for Container Store
///
/// KV-Store:
/// - Key: Encoded Container ID
/// - Value: Encoded Container State
///
/// ─ ─ ─ ─ ─ ─ ─ For Each Container Type ─ ─ ─ ─ ─ ─ ─ ─
///
/// ┌────────────────┬──────────────────────────────────┐
/// │   u16 Depth    │             ParentID             │
/// └────────────────┴──────────────────────────────────┘
/// ┌───────────────────────────────────────────────────┐
/// │ ┌───────────────────────────────────────────────┐ │
/// │ │                Into<LoroValue>                │ │
/// │ └───────────────────────────────────────────────┘ │
/// │                                                   │
/// │             Container Specific Encode             │
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// └───────────────────────────────────────────────────┘
pub(crate) struct ContainerStore {
    arena: SharedArena,
    store: InnerStore,
    shallow_root_store: Option<Arc<GcStore>>,
    conf: Configure,
    peer: Arc<AtomicU64>,
}

pub(crate) const FRONTIERS_KEY: &[u8] = b"fr";
impl std::fmt::Debug for ContainerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerStore")
            .field("store", &self.store)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct GcStore {
    pub shallow_root_frontiers: Frontiers,
    pub encoded_state_bytes: Bytes,
    pub store: Mutex<InnerStore>,
}

macro_rules! ctx {
    ($self:expr) => {
        ContainerCreationContext {
            configure: &$self.conf,
            peer: $self.peer.load(std::sync::atomic::Ordering::Relaxed),
        }
    };
}

impl ContainerStore {
    pub fn new(arena: SharedArena, conf: Configure, peer: Arc<AtomicU64>) -> Self {
        ContainerStore {
            store: InnerStore::new(arena.clone(), conf.clone()),
            arena,
            conf,
            shallow_root_store: None,
            peer,
        }
    }

    pub fn can_import_snapshot(&self) -> bool {
        if self.shallow_root_store.is_some() {
            return false;
        }

        self.store.can_import_snapshot()
    }

    pub fn get_container_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.store
            .get_mut(idx)
            .map(|x| x.get_state_mut(idx, ctx!(self)))
    }

    #[allow(unused)]
    pub fn get_container(&mut self, idx: ContainerIdx) -> Option<&State> {
        self.store
            .get_mut(idx)
            .map(|x| x.get_state(idx, ctx!(self)))
    }

    pub fn shallow_root_store(&self) -> Option<&Arc<GcStore>> {
        self.shallow_root_store.as_ref()
    }

    pub fn get_value(&mut self, idx: ContainerIdx) -> Option<LoroValue> {
        self.store
            .with_container_for_read(idx, |c| c.get_value(idx, ctx!(self)))
    }

    pub(crate) fn get_value_ephemeral(&mut self, idx: ContainerIdx) -> Option<LoroValue> {
        self.try_get_value_ephemeral(idx)
            .expect("snapshot-backed container should decode")
    }

    pub(crate) fn try_get_value_ephemeral(
        &mut self,
        idx: ContainerIdx,
    ) -> LoroResult<Option<LoroValue>> {
        self.store.try_get_value_ephemeral(idx, ctx!(self))
    }

    pub(crate) fn try_get_parent_and_value_ephemeral(
        &mut self,
        idx: ContainerIdx,
    ) -> LoroResult<Option<(Option<ContainerID>, LoroValue)>> {
        self.store
            .try_get_parent_and_value_ephemeral(idx, ctx!(self))
    }

    pub(crate) fn get_parent_ephemeral(
        &mut self,
        idx: ContainerIdx,
    ) -> LoroResult<Option<Option<ContainerID>>> {
        self.store.get_parent_ephemeral(idx)
    }

    pub fn map_get(&mut self, idx: ContainerIdx, key: &str) -> Option<LoroValue> {
        self.store
            .with_container_for_read(idx, |c| c.map_get(idx, ctx!(self), key))?
    }

    pub fn map_len(&mut self, idx: ContainerIdx) -> usize {
        self.store
            .with_container_for_read(idx, |c| c.map_len(idx, ctx!(self)))
            .unwrap_or(0)
    }

    pub fn map_keys(&mut self, idx: ContainerIdx) -> Vec<InternalString> {
        self.store
            .with_container_for_read(idx, |c| c.map_keys(idx, ctx!(self)))
            .unwrap_or_default()
    }

    pub fn map_entries(&mut self, idx: ContainerIdx) -> Vec<(InternalString, LoroValue)> {
        self.store
            .with_container_for_read(idx, |c| c.map_entries(idx, ctx!(self)))
            .unwrap_or_default()
    }

    pub fn list_get(&mut self, idx: ContainerIdx, index: usize) -> Option<LoroValue> {
        self.store
            .with_container_for_read(idx, |c| c.list_get(idx, ctx!(self), index))?
    }

    pub fn list_len(&mut self, idx: ContainerIdx) -> usize {
        self.store
            .with_container_for_read(idx, |c| c.list_len(idx, ctx!(self)))
            .unwrap_or(0)
    }

    pub fn list_values(&mut self, idx: ContainerIdx) -> Vec<LoroValue> {
        self.store
            .with_container_for_read(idx, |c| c.list_values(idx, ctx!(self)))
            .unwrap_or_default()
    }

    pub fn text_unicode_len(&mut self, idx: ContainerIdx) -> Option<usize> {
        self.store
            .with_container_for_read(idx, |c| c.text_unicode_len(idx, ctx!(self)))?
    }

    pub fn text_utf16_len(&mut self, idx: ContainerIdx) -> Option<usize> {
        self.store
            .with_container_for_read(idx, |c| c.text_utf16_len(idx, ctx!(self)))?
    }

    pub fn text_utf8_len(&mut self, idx: ContainerIdx) -> Option<usize> {
        self.store
            .with_container_for_read(idx, |c| c.text_utf8_len(idx, ctx!(self)))?
    }

    pub fn has_decoded_state(&mut self, idx: ContainerIdx) -> bool {
        self.store.has_decoded_state(idx)
    }

    pub fn encode(&mut self) -> Bytes {
        self.store.encode()
    }

    pub(crate) fn flush(&mut self) {
        self.store.flush()
    }

    pub fn shallow_root_frontiers(&self) -> Option<&Frontiers> {
        self.shallow_root_store
            .as_ref()
            .map(|x| &x.shallow_root_frontiers)
    }

    pub(crate) fn encode_shallow_root_state(&self) -> Option<Bytes> {
        let shallow_root = self.shallow_root_store.as_ref()?;
        Some(shallow_root.encoded_state_bytes.clone())
    }

    pub(crate) fn shallow_root_state_for_export(&self) -> Option<(Bytes, KvWrapper)> {
        let shallow_root = self.shallow_root_store.as_ref()?;
        let shallow_root_kv = shallow_root.store.lock().get_kv_clone();
        Some((shallow_root.encoded_state_bytes.clone(), shallow_root_kv))
    }

    pub(crate) fn decode(&mut self, bytes: Bytes) -> LoroResult<Option<Frontiers>> {
        self.store.decode(bytes)
    }

    pub(crate) fn decode_gc(
        &mut self,
        shallow_bytes: Bytes,
        start_frontiers: Frontiers,
        config: Configure,
    ) -> LoroResult<Option<Frontiers>> {
        assert!(self.shallow_root_store.is_none());
        let mut inner = InnerStore::new(self.arena.clone(), config);
        let encoded_state_bytes = shallow_bytes.clone();
        let f = inner.decode(shallow_bytes)?;
        let encoded_state_bytes = if f.as_ref() == Some(&start_frontiers) {
            encoded_state_bytes
        } else {
            let kv = inner.get_kv_clone();
            kv.insert(FRONTIERS_KEY, start_frontiers.encode().into());
            kv.export()
        };
        self.shallow_root_store = Some(Arc::new(GcStore {
            shallow_root_frontiers: start_frontiers,
            encoded_state_bytes,
            store: Mutex::new(inner),
        }));
        Ok(f)
    }

    pub(crate) fn decode_state_by_two_bytes(
        &mut self,
        shallow_bytes: Bytes,
        state_bytes: Bytes,
    ) -> LoroResult<()> {
        self.store
            .decode_twice(shallow_bytes.clone(), state_bytes)?;
        Ok(())
    }

    pub fn iter_and_decode_all(&mut self) -> impl Iterator<Item = &mut State> {
        self.store.iter_all_containers_mut().map(|(idx, v)| {
            v.get_state_mut(
                idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    pub fn get_kv_clone(&self) -> KvWrapper {
        self.store.get_kv_clone()
    }

    pub fn contains_id(&mut self, id: &ContainerID) -> bool {
        self.store.contains_id(id)
    }

    pub fn iter_all_containers(
        &mut self,
    ) -> impl Iterator<Item = (ContainerIdx, &mut ContainerWrapper)> {
        self.store.iter_all_containers_mut()
    }

    pub fn iter_all_container_ids(&mut self) -> impl Iterator<Item = ContainerID> + '_ {
        self.store.iter_all_container_ids()
    }

    pub fn load_root_containers(&mut self) -> LoadAllFlag {
        self.store.load_roots();
        LoadAllFlag
    }

    pub(super) fn get_or_create_mut(&mut self, idx: ContainerIdx) -> &mut State {
        self.store
            .get_or_insert_with(idx, || {
                let state = super::create_state_(
                    idx,
                    &self.conf,
                    self.peer.load(std::sync::atomic::Ordering::Relaxed),
                );
                ContainerWrapper::new(state, &self.arena)
            })
            .get_state_mut(idx, ctx!(self))
    }

    pub(crate) fn ensure_container(&mut self, id: &loro_common::ContainerID) {
        let idx = self.arena.register_container(id);
        self.store.ensure_container(idx, || {
            let state = super::create_state_(
                idx,
                &self.conf,
                self.peer.load(std::sync::atomic::Ordering::Relaxed),
            );
            ContainerWrapper::new(state, &self.arena)
        });
    }

    pub(super) fn get_or_create_imm(&mut self, idx: ContainerIdx) -> &State {
        self.store
            .get_or_insert_with(idx, || {
                let state = super::create_state_(
                    idx,
                    &self.conf,
                    self.peer.load(std::sync::atomic::Ordering::Relaxed),
                );
                ContainerWrapper::new(state, &self.arena)
            })
            .get_state(idx, ctx!(self))
    }

    pub(crate) fn fork(
        &mut self,
        arena: SharedArena,
        peer: Arc<AtomicU64>,
        config: Configure,
    ) -> ContainerStore {
        ContainerStore {
            store: self.store.fork(arena.clone(), &config),
            arena,
            conf: config,
            peer,
            shallow_root_store: None,
        }
    }

    #[allow(unused)]
    fn check_eq_after_parsing(&mut self, other: &mut ContainerStore) {
        for (idx, container) in self.store.iter_all_containers_mut() {
            let id = self.arena.get_container_id(idx).unwrap();
            let other_idx = other.arena.register_container(&id);
            let other_container = other
                .store
                .get_mut(other_idx)
                .expect("container not found on other store");
            let other_id = other.arena.get_container_id(other_idx).unwrap();
            assert_eq!(
                id, other_id,
                "container id mismatch {:?} {:?}",
                id, other_id
            );
            assert_eq!(
                container.get_value(idx, ctx!(self)),
                other_container.get_value(other_idx, ctx!(other)),
                "value mismatch"
            );

            if container.encode() != other_container.encode() {
                panic!(
                    "container mismatch Origin: {:#?}, New: {:#?}",
                    &container, &other_container
                );
            }

            other_container
                .decode_state(other_idx, ctx!(other))
                .unwrap();
            other_container.clear_bytes();
            if container.encode() != other_container.encode() {
                panic!(
                    "container mismatch Origin: {:#?}, New: {:#?}",
                    &container, &other_container
                );
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        cursor::PosType, handler::HandlerTrait, state::TreeParentId, ContainerType, ListHandler,
        LoroDoc, MapHandler, MovableListHandler,
    };

    fn decode_container_store(bytes: Bytes) -> ContainerStore {
        let mut new_store = ContainerStore::new(
            SharedArena::new(),
            Configure::default(),
            Arc::new(AtomicU64::new(233)),
        );

        new_store.decode(bytes).unwrap();
        new_store
    }

    fn init_doc() -> LoroDoc {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let text = doc.get_text("text");
        text.insert(0, "hello", PosType::Unicode).unwrap();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();

        let list = doc.get_list("list");
        list.push("item1").unwrap();

        let tree = doc.get_tree("tree");
        let root = tree.create(TreeParentId::Root).unwrap();
        tree.create_at(TreeParentId::Node(root), 0).unwrap();

        let movable_list = doc.get_movable_list("movable_list");
        movable_list.insert(0, "movable_item").unwrap();

        // Create child containers
        let child_map = map
            .insert_container("child_map", MapHandler::new_detached())
            .unwrap();
        child_map.insert("child_key", "child_value").unwrap();

        let child_list = list
            .insert_container(0, ListHandler::new_detached())
            .unwrap();
        child_list.push("child_item").unwrap();
        let child_movable_list = movable_list
            .insert_container(0, MovableListHandler::new_detached())
            .unwrap();
        child_movable_list.insert(0, "child_movable_item").unwrap();
        doc
    }

    fn export_container_store(doc: &LoroDoc) -> Bytes {
        let mut state = doc.app_state().lock();
        state.ensure_all_alive_containers().unwrap();
        state.store.encode()
    }

    #[test]
    fn test_container_store_exports_imports() {
        let doc = init_doc();
        let bytes = export_container_store(&doc);
        let mut new_store = decode_container_store(bytes);
        let mut s = doc.app_state().lock();
        s.store.check_eq_after_parsing(&mut new_store);
    }

    #[test]
    fn first_lazy_read_caches_value() {
        let doc = init_doc();
        let bytes = export_container_store(&doc);
        let mut store = decode_container_store(bytes);
        let map_id = ContainerID::new_root("map", ContainerType::Map);
        let map_idx = store.arena.register_container(&map_id);

        assert!(!store.store.has_cached_value_for_test(map_idx));
        assert_eq!(store.map_len(map_idx), 2);
        assert!(store.store.has_cached_value_for_test(map_idx));
    }

    #[test]
    fn ephemeral_lazy_read_does_not_cache_value() {
        let doc = init_doc();
        let bytes = export_container_store(&doc);
        let mut store = decode_container_store(bytes);
        let map_id = ContainerID::new_root("map", ContainerType::Map);
        let map_idx = store.arena.register_container(&map_id);

        assert!(!store.store.has_cached_value_for_test(map_idx));
        assert_eq!(
            store
                .get_value_ephemeral(map_idx)
                .and_then(|value| value.as_map().map(|map| map.len())),
            Some(2)
        );
        assert!(!store.store.has_cached_value_for_test(map_idx));
    }

    #[test]
    fn combined_ephemeral_parent_and_value_read_does_not_cache_value() {
        let doc = init_doc();
        let bytes = export_container_store(&doc);
        let mut store = decode_container_store(bytes);
        let map_id = ContainerID::new_root("map", ContainerType::Map);
        let map_idx = store.arena.register_container(&map_id);

        assert!(!store.store.has_cached_value_for_test(map_idx));
        let (parent, value) = store
            .try_get_parent_and_value_ephemeral(map_idx)
            .unwrap()
            .unwrap();
        assert_eq!(parent, None);
        assert_eq!(value.as_map().map(|map| map.len()), Some(2));
        assert!(!store.store.has_cached_value_for_test(map_idx));
    }

    #[test]
    fn snapshot_export_does_not_materialize_map_value_after_key_read() {
        let source = init_doc();
        let snapshot = source
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        let imported = LoroDoc::new();
        imported.import(&snapshot).unwrap();
        let map_id = ContainerID::new_root("map", ContainerType::Map);

        assert_eq!(imported.get_map("map").get("key"), Some("value".into()));
        {
            let mut state = imported.app_state().lock();
            let map_idx = state.store.arena.register_container(&map_id);
            assert!(state.store.store.has_cached_value_for_test(map_idx));
            assert!(!state
                .store
                .store
                .has_materialized_map_value_for_test(map_idx));
        }

        imported
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();

        let mut state = imported.app_state().lock();
        let map_idx = state.store.arena.register_container(&map_id);
        assert!(!state
            .store
            .store
            .has_materialized_map_value_for_test(map_idx));
    }

    #[test]
    fn snapshot_export_keeps_imported_state_lazy() {
        let source = init_doc();
        let snapshot = source
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        let imported = LoroDoc::new();
        imported.import(&snapshot).unwrap();
        let map_id = ContainerID::new_root("map", ContainerType::Map);

        {
            let mut state = imported.app_state().lock();
            let map_idx = state.store.arena.register_container(&map_id);
            assert!(!state.store.store.has_cached_value_for_test(map_idx));
        }

        let exported = imported
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        assert!(!exported.is_empty());
        let exported_again = imported
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        assert_eq!(exported_again, exported);

        let mut state = imported.app_state().lock();
        let map_idx = state.store.arena.register_container(&map_id);
        assert!(!state.store.store.has_cached_value_for_test(map_idx));
    }

    #[test]
    fn snapshot_export_includes_empty_root_without_version_change() {
        let doc = LoroDoc::new_auto_commit();
        doc.get_map("first");
        doc.export(crate::encoding::ExportMode::Snapshot).unwrap();
        let version_before = doc.state_frontiers();

        let second_id = ContainerID::new_root("second", ContainerType::Map);
        doc.get_map("second");
        assert_eq!(doc.state_frontiers(), version_before);

        let snapshot = doc.export(crate::encoding::ExportMode::Snapshot).unwrap();
        let round_tripped = LoroDoc::new();
        round_tripped.import(&snapshot).unwrap();
        let mut state = round_tripped.app_state().lock();
        assert!(state.store.contains_id(&second_id));
    }

    #[test]
    fn snapshot_export_includes_new_empty_child() {
        let doc = LoroDoc::new_auto_commit();
        let root = doc.get_map("root");
        root.insert("value", 1).unwrap();
        doc.export(crate::encoding::ExportMode::Snapshot).unwrap();

        let child = root
            .insert_container("child", MapHandler::new_detached())
            .unwrap();
        let child_id = child.id();
        let snapshot = doc.export(crate::encoding::ExportMode::Snapshot).unwrap();
        let round_tripped = LoroDoc::new();
        round_tripped.import(&snapshot).unwrap();
        let mut state = round_tripped.app_state().lock();
        assert!(state.store.contains_id(&child_id));
    }

    /// An empty child created by a remote peer has no ops of its own, so the importer only
    /// applies the parent's diff. The diff-apply hook must still create a store entry for the
    /// child, or the importer's own full snapshot would silently drop it.
    #[test]
    fn snapshot_export_includes_remote_empty_child() {
        let a = LoroDoc::new_auto_commit();
        let map_child_id = a
            .get_map("map")
            .insert_container("child", MapHandler::new_detached())
            .unwrap()
            .id();
        let list_child_id = a
            .get_list("list")
            .insert_container(0, ListHandler::new_detached())
            .unwrap()
            .id();
        let movable_child_id = a
            .get_movable_list("movable")
            .insert_container(0, MovableListHandler::new_detached())
            .unwrap()
            .id();
        let updates = a
            .export(crate::encoding::ExportMode::all_updates())
            .unwrap();

        let b = LoroDoc::new_auto_commit();
        b.import(&updates).unwrap();
        let snapshot = b.export(crate::encoding::ExportMode::Snapshot).unwrap();

        let c = LoroDoc::new();
        c.import(&snapshot).unwrap();
        assert_eq!(a.get_deep_value(), c.get_deep_value());
        let mut state = c.app_state().lock();
        for id in [&map_child_id, &list_child_id, &movable_child_id] {
            assert!(state.store.contains_id(id), "missing {id:?}");
        }
    }

    /// A remote tree node's associated meta map has no ops until someone writes metadata; the
    /// tree diff's `Create` action must ensure its store entry on the importer.
    #[test]
    fn snapshot_export_includes_remote_tree_meta() {
        let a = LoroDoc::new_auto_commit();
        let node = a.get_tree("tree").create(TreeParentId::Root).unwrap();
        let meta_id = node.associated_meta_container();
        let updates = a
            .export(crate::encoding::ExportMode::all_updates())
            .unwrap();

        let b = LoroDoc::new_auto_commit();
        b.import(&updates).unwrap();
        let snapshot = b.export(crate::encoding::ExportMode::Snapshot).unwrap();

        let c = LoroDoc::new();
        c.import(&snapshot).unwrap();
        let mut state = c.app_state().lock();
        assert!(state.store.contains_id(&meta_id));
    }

    /// An ensured-but-empty mergeable child has no store entry anywhere — the parent map's
    /// marker is its single source of truth. A full snapshot round-trip through an importer
    /// must keep the child resolvable by id without materializing an entry for it.
    #[test]
    fn snapshot_export_keeps_empty_marker_child_resolvable_without_entry() {
        let a = LoroDoc::new_auto_commit();
        let child_id = a
            .get_map("state")
            .ensure_mergeable_text("notes")
            .unwrap()
            .id();
        let updates = a
            .export(crate::encoding::ExportMode::all_updates())
            .unwrap();

        let b = LoroDoc::new_auto_commit();
        b.import(&updates).unwrap();
        let snapshot = b.export(crate::encoding::ExportMode::Snapshot).unwrap();

        let c = LoroDoc::new();
        c.import(&snapshot).unwrap();
        let mut state = c.app_state().lock();
        assert!(!state.store.contains_id(&child_id));
        assert!(
            state.does_container_exist(&child_id),
            "empty mergeable child must resolve from the marker after round-trip"
        );
    }

    #[test]
    fn deep_value_read_keeps_imported_state_lazy() {
        let source = init_doc();
        let expected = source.get_deep_value();
        let child_map_id = source
            .get_map("map")
            .get("child_map")
            .and_then(|value| value.as_container().cloned())
            .unwrap();
        let snapshot = source
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        let imported = LoroDoc::new();
        imported.import(&snapshot).unwrap();
        let map_id = ContainerID::new_root("map", ContainerType::Map);

        assert_eq!(imported.get_deep_value(), expected);
        assert_eq!(imported.get_deep_value(), expected);

        let mut state = imported.app_state().lock();
        let map_idx = state.store.arena.register_container(&map_id);
        let child_map_idx = state.store.arena.register_container(&child_map_id);
        assert!(!state.store.store.has_cached_value_for_test(map_idx));
        assert!(!state.store.store.has_cached_value_for_test(child_map_idx));
    }

    #[test]
    fn snapshot_export_persists_empty_alive_child() {
        let source = LoroDoc::new_auto_commit();
        let child = source
            .get_map("root")
            .insert_container("empty", MapHandler::new_detached())
            .unwrap();
        let child_id = child.id();
        let updates = source
            .export(crate::encoding::ExportMode::all_updates())
            .unwrap();
        let target = LoroDoc::new();
        target.import(&updates).unwrap();
        {
            // The diff-apply hook creates the empty child's store entry at import time; full
            // snapshot export relies on this invariant instead of walking the alive graph.
            let mut state = target.app_state().lock();
            assert!(state.store.contains_id(&child_id));
        }

        let snapshot = target
            .export(crate::encoding::ExportMode::Snapshot)
            .unwrap();
        let round_tripped = LoroDoc::new();
        round_tripped.import(&snapshot).unwrap();

        let mut state = round_tripped.app_state().lock();
        assert!(state.store.contains_id(&child_id));
    }
}
