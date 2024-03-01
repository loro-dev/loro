use std::{
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::ID;
use tabled::{TableIteratorExt, Tabled};

#[allow(unused_imports)]
use crate::{
    array_mut_ref, container::ContainerID, delta::DeltaItem, id::PeerID, ContainerType, LoroValue,
};
use crate::{
    container::{
        idx::ContainerIdx,
        richtext::richtext_state::{unicode_to_utf8_index, utf16_to_utf8_index},
    },
    event::Diff,
    handler::{TextHandler, ValueOrContainer},
    loro::LoroDoc,
    value::ToJson,
    version::Frontiers,
    ApplyDiff, ListHandler, MapHandler,
};

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Map {
        site: u8,
        container_idx: u8,
        key: u8,
        value: FuzzValue,
    },
    List {
        site: u8,
        container_idx: u8,
        key: u8,
        value: FuzzValue,
    },
    Text {
        site: u8,
        container_idx: u8,
        pos: u8,
        value: u16,
        is_del: bool,
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

struct Actor {
    peer: PeerID,
    loro: LoroDoc,
    value_tracker: Arc<Mutex<LoroValue>>,
    map_tracker: Arc<Mutex<FxHashMap<String, LoroValue>>>,
    list_tracker: Arc<Mutex<Vec<LoroValue>>>,
    text_tracker: Arc<Mutex<String>>,
    map_containers: Vec<MapHandler>,
    list_containers: Vec<ListHandler>,
    text_containers: Vec<TextHandler>,
    history: FxHashMap<Vec<ID>, LoroValue>,
}

impl Actor {
    fn new(id: PeerID) -> Self {
        let app = LoroDoc::new();
        app.set_peer_id(id).unwrap();
        let mut actor = Actor {
            peer: id,
            loro: app,
            value_tracker: Arc::new(Mutex::new(LoroValue::Map(Default::default()))),
            map_tracker: Default::default(),
            list_tracker: Default::default(),
            text_tracker: Default::default(),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
            history: Default::default(),
        };
        actor
            .text_containers
            .push(actor.loro.txn().unwrap().get_text("text"));
        actor
            .map_containers
            .push(actor.loro.txn().unwrap().get_map("map"));
        actor
            .list_containers
            .push(actor.loro.txn().unwrap().get_list("list"));

        let root_value = Arc::clone(&actor.value_tracker);
        actor.loro.subscribe_root(Arc::new(move |event| {
            let mut root_value = root_value.lock().unwrap();
            for container_diff in event.events {
                root_value.apply(
                    &container_diff.path.iter().map(|x| x.1.clone()).collect(),
                    &[container_diff.diff.clone()],
                );
            }
        }));

        let text = Arc::clone(&actor.text_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Arc::new(move |event| {
                let mut text = text.lock().unwrap();
                for container_diff in event.events {
                    match &container_diff.diff {
                        Diff::Text(delta) => {
                            let mut index = 0;
                            for item in delta.iter() {
                                match item {
                                    DeltaItem::Retain {
                                        retain: len,
                                        attributes: _,
                                    } => {
                                        index += len;
                                    }
                                    DeltaItem::Insert {
                                        insert: value,
                                        attributes: _,
                                    } => {
                                        let utf8_index = if cfg!(feature = "wasm") {
                                            let ans = utf16_to_utf8_index(&text, index).unwrap();
                                            index += value.len_utf16();
                                            ans
                                        } else {
                                            let ans = unicode_to_utf8_index(&text, index).unwrap();
                                            index += value.len_unicode();
                                            ans
                                        };
                                        text.insert_str(utf8_index, value.as_str());
                                    }
                                    DeltaItem::Delete { delete: len, .. } => {
                                        let utf8_index = if cfg!(feature = "wasm") {
                                            utf16_to_utf8_index(&text, index).unwrap()
                                        } else {
                                            unicode_to_utf8_index(&text, index).unwrap()
                                        };

                                        let utf8_end = if cfg!(feature = "wasm") {
                                            utf16_to_utf8_index(&text, index + *len).unwrap()
                                        } else {
                                            unicode_to_utf8_index(&text, index + *len).unwrap()
                                        };
                                        text.drain(utf8_index..utf8_end);
                                    }
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }),
        );
        let arena = actor.loro.oplog().lock().unwrap().arena.clone();
        let map = Arc::clone(&actor.map_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("map", ContainerType::Map),
            Arc::new(move |event| {
                let mut map = map.lock().unwrap();
                for container_diff in event.events {
                    if container_diff.id != ContainerID::new_root("map", ContainerType::Map) {
                        continue;
                    }
                    if let Diff::Map(map_diff) = &container_diff.diff {
                        for (key, value) in map_diff.updated.iter() {
                            match &value.value {
                                Some(value) => {
                                    let value = match value {
                                        ValueOrContainer::Container(c) => {
                                            let id = arena.idx_to_id(c.container_idx()).unwrap();
                                            LoroValue::Container(id)
                                        }
                                        ValueOrContainer::Value(v) => v.clone(),
                                    };
                                    map.insert(key.to_string(), value);
                                }
                                None => {
                                    map.remove(&key.to_string());
                                }
                            }
                        }
                    } else {
                        unreachable!()
                    }
                }
            }),
        );
        let arena = actor.loro.oplog().lock().unwrap().arena.clone();
        let list = Arc::clone(&actor.list_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("list", ContainerType::List),
            Arc::new(move |event| {
                let mut list = list.lock().unwrap();
                for container_diff in event.events {
                    if container_diff.id != ContainerID::new_root("list", ContainerType::List) {
                        continue;
                    }

                    if let Diff::List(delta) = &container_diff.diff {
                        let mut index = 0;
                        for item in delta.iter() {
                            match item {
                                DeltaItem::Retain {
                                    retain: len,
                                    attributes: _,
                                } => {
                                    index += len;
                                }
                                DeltaItem::Insert {
                                    insert: value,
                                    attributes: _,
                                } => {
                                    for v in value {
                                        let value = match v {
                                            ValueOrContainer::Container(c) => {
                                                let id =
                                                    arena.idx_to_id(c.container_idx()).unwrap();
                                                LoroValue::Container(id)
                                            }
                                            ValueOrContainer::Value(v) => v.clone(),
                                        };
                                        list.insert(index, value);
                                        index += 1;
                                    }
                                }
                                DeltaItem::Delete { delete: len, .. } => {
                                    list.drain(index..index + *len);
                                }
                            }
                        }
                    } else {
                        unreachable!()
                    }
                }
            }),
        );

        actor
    }

    fn record_history(&mut self) {
        let f = self.loro.oplog_frontiers();
        let value = self.loro.get_deep_value();
        let mut ids: Vec<ID> = f.iter().cloned().collect();
        ids.sort_by_key(|x| x.peer);
        self.history.insert(ids, value);
    }
}

#[derive(Arbitrary, Clone, Debug, PartialEq, Eq)]
pub enum FuzzValue {
    Null,
    I32(i32),
    Container(ContainerType),
}

impl From<FuzzValue> for LoroValue {
    fn from(v: FuzzValue) -> Self {
        match v {
            FuzzValue::Null => LoroValue::Null,
            FuzzValue::I32(i) => LoroValue::I64(i as i64),
            FuzzValue::Container(_) => unreachable!(),
        }
    }
}

impl Tabled for Action {
    const LENGTH: usize = 5;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Sync { from, to } => vec![
                "sync".into(),
                format!("{} with {}", from, to).into(),
                "".into(),
                "".into(),
                "".into(),
            ],
            Action::SyncAll => vec!["sync all".into(), "".into(), "".into(), "".into()],
            Action::Map {
                site,
                container_idx,
                key,
                value,
            } => vec![
                "map".into(),
                format!("{}", site).into(),
                format!("{}", container_idx).into(),
                format!("{}", key).into(),
                format!("{:?}", value).into(),
            ],
            Action::List {
                site,
                container_idx,
                key,
                value,
            } => vec![
                "list".into(),
                format!("{}", site).into(),
                format!("{}", container_idx).into(),
                format!("{}", key).into(),
                format!("{:?}", value).into(),
            ],
            Action::Text {
                site,
                container_idx,
                pos,
                value,
                is_del,
            } => vec![
                "text".into(),
                format!("{}", site).into(),
                format!("{}", container_idx).into(),
                format!("{}", pos).into(),
                format!("{}{}", if *is_del { "Delete " } else { "" }, value).into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "type".into(),
            "site".into(),
            "container".into(),
            "prop".into(),
            "value".into(),
        ]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&mut self, action: &mut Action);
}

impl Actor {
    fn add_new_container(&mut self, idx: ContainerIdx, type_: ContainerType) {
        let txn = self.loro.get_global_txn();
        match type_ {
            ContainerType::Text => self.text_containers.push(TextHandler::new(
                txn,
                idx,
                Arc::downgrade(self.loro.app_state()),
            )),
            ContainerType::Map => self.map_containers.push(MapHandler::new(
                txn,
                idx,
                Arc::downgrade(self.loro.app_state()),
            )),
            ContainerType::List => self.list_containers.push(ListHandler::new(
                txn,
                idx,
                Arc::downgrade(self.loro.app_state()),
            )),
            ContainerType::Tree => {
                // TODO Tree
            }
        }
    }
}

impl Actionable for Vec<Actor> {
    fn preprocess(&mut self, action: &mut Action) {
        let max_users = self.len() as u8;
        match action {
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
                if to == from {
                    *to = (*to + 1) % max_users;
                }
            }
            Action::SyncAll => {}
            Action::Map {
                site,
                container_idx,
                ..
            } => {
                *site %= max_users;
                *container_idx %= self[*site as usize].map_containers.len().max(1) as u8;
            }
            Action::List {
                site,
                container_idx,
                key,
                value,
            } => {
                *site %= max_users;
                *container_idx %= self[*site as usize].list_containers.len().max(1) as u8;
                if let Some(list) = self[*site as usize]
                    .list_containers
                    .get(*container_idx as usize)
                {
                    *key %= (list.len() as u8).max(1);
                    if *value == FuzzValue::Null && list.is_empty() {
                        // no value, cannot delete
                        *value = FuzzValue::I32(1);
                    }
                } else {
                    if *value == FuzzValue::Null {
                        *value = FuzzValue::I32(1);
                    }
                    *key = 0;
                }
            }
            Action::Text {
                site,
                container_idx,
                pos,
                value,
                is_del,
            } => {
                *site %= max_users;
                *container_idx %= self[*site as usize].text_containers.len().max(1) as u8;
                if let Some(text) = self[*site as usize]
                    .text_containers
                    .get(*container_idx as usize)
                {
                    *pos %= (text.len_unicode() as u8).max(1);
                    if *is_del {
                        *value &= 0x1f;
                        *value = (*value).min(text.len_unicode() as u16 - (*pos) as u16);
                    }
                } else {
                    *is_del = false;
                    *pos = 0;
                }
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Sync { from, to } => {
                let (a, b) = array_mut_ref!(self, [*from as usize, *to as usize]);
                let mut visited = HashSet::new();
                a.map_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.list_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.text_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });

                a.loro
                    .import(&b.loro.export_from(&a.loro.oplog_vv()))
                    .unwrap();
                b.loro
                    .import(&a.loro.export_from(&b.loro.oplog_vv()))
                    .unwrap();

                if a.peer == 1 {
                    a.record_history();
                }

                b.map_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.map_containers.push(a.loro.txn().unwrap().get_map(id))
                    }
                });
                b.list_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.list_containers.push(a.loro.txn().unwrap().get_list(id))
                    }
                });
                b.text_containers.iter().for_each(|x| {
                    let id = x.id();
                    if !visited.contains(&id) {
                        visited.insert(id.clone());
                        a.text_containers.push(a.loro.txn().unwrap().get_text(id))
                    }
                });

                b.map_containers = a
                    .map_containers
                    .iter()
                    .map(|x| b.loro.get_map(x.id()))
                    .collect();
                b.list_containers = a
                    .list_containers
                    .iter()
                    .map(|x| b.loro.get_list(x.id()))
                    .collect();
                b.text_containers = a
                    .text_containers
                    .iter()
                    .map(|x| b.loro.get_text(x.id()))
                    .collect();
            }
            Action::SyncAll => {
                let mut visited = HashSet::new();
                let a = &mut self[0];
                a.map_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.list_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });
                a.text_containers.iter().for_each(|x| {
                    visited.insert(x.id());
                });

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.loro
                        .import(&b.loro.export_from(&a.loro.oplog_vv()))
                        .unwrap();
                    b.map_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.map_containers.push(a.loro.get_map(id))
                        }
                    });
                    b.list_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.list_containers.push(a.loro.get_list(id))
                        }
                    });
                    b.text_containers.iter().for_each(|x| {
                        let id = x.id();
                        if !visited.contains(&id) {
                            visited.insert(id.clone());
                            a.text_containers.push(a.loro.get_text(id))
                        }
                    });
                }

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.loro
                        .import(&a.loro.export_from(&b.loro.oplog_vv()))
                        .unwrap();
                    b.map_containers = a
                        .map_containers
                        .iter()
                        .map(|x| b.loro.get_map(x.id()))
                        .collect();
                    b.list_containers = a
                        .list_containers
                        .iter()
                        .map(|x| b.loro.get_list(x.id()))
                        .collect();
                    b.text_containers = a
                        .text_containers
                        .iter()
                        .map(|x| b.loro.get_text(x.id()))
                        .collect();
                }

                self[1].record_history();
            }
            Action::Map {
                site,
                container_idx,
                key,
                value,
            } => {
                let actor = &mut self[*site as usize];
                let container = actor.map_containers.get_mut(*container_idx as usize);
                let container = if let Some(container) = container {
                    container
                } else {
                    let map = actor.loro.get_map("map");
                    actor.map_containers.push(map);
                    &mut actor.map_containers[0]
                };
                let mut txn = actor.loro.txn().unwrap();
                match value {
                    FuzzValue::Null => {
                        container
                            .delete_with_txn(&mut txn, &key.to_string())
                            .unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container
                            .insert_with_txn(&mut txn, &key.to_string(), LoroValue::from(*i))
                            .unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let idx = container
                            .insert_container_with_txn(&mut txn, &key.to_string(), *c)
                            .unwrap()
                            .container_idx();
                        actor.add_new_container(idx, *c);
                    }
                };

                txn.commit().unwrap();
                if actor.peer == 1 {
                    actor.record_history();
                }
            }
            Action::List {
                site,
                container_idx,
                key,
                value,
            } => {
                let actor = &mut self[*site as usize];
                let container = actor.list_containers.get_mut(*container_idx as usize);
                let container = if container.is_none() {
                    let list = actor.loro.get_list("list");
                    actor.list_containers.push(list);
                    &mut actor.list_containers[0]
                } else {
                    #[allow(clippy::unnecessary_unwrap)]
                    container.unwrap()
                };
                let mut txn = actor.loro.txn().unwrap();
                match value {
                    FuzzValue::Null => {
                        container
                            .delete_with_txn(&mut txn, *key as usize, 1)
                            .unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container
                            .insert_with_txn(&mut txn, *key as usize, LoroValue::from(*i))
                            .unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let idx = container
                            .insert_container_with_txn(&mut txn, *key as usize, *c)
                            .unwrap()
                            .container_idx();
                        actor.add_new_container(idx, *c);
                    }
                };
                txn.commit().unwrap();
                if actor.peer == 1 {
                    actor.record_history();
                }
            }
            Action::Text {
                site,
                container_idx,
                pos,
                value,
                is_del,
            } => {
                let actor = &mut self[*site as usize];
                let container = actor.text_containers.get_mut(*container_idx as usize);
                let container = if let Some(container) = container {
                    container
                } else {
                    let text = actor.loro.get_text("text");
                    actor.text_containers.push(text);
                    &mut actor.text_containers[0]
                };
                let mut txn = actor.loro.txn().unwrap();
                if *is_del {
                    container
                        .delete_with_txn(&mut txn, *pos as usize, *value as usize)
                        .unwrap();
                } else {
                    container
                        .insert_with_txn(&mut txn, *pos as usize, &(format!("[{}]", value)))
                        .unwrap();
                }
                drop(txn);
                if actor.peer == 1 {
                    actor.record_history();
                }
            }
        }
    }
}

#[allow(unused)]
fn assert_value_eq(a: &LoroValue, b: &LoroValue) {
    match (a, b) {
        (LoroValue::Map(a), LoroValue::Map(b)) => {
            for (k, v) in a.iter() {
                let is_empty = match v {
                    LoroValue::String(s) => s.is_empty(),
                    LoroValue::List(l) => l.is_empty(),
                    LoroValue::Map(m) => m.is_empty(),
                    _ => false,
                };
                if is_empty {
                    continue;
                }
                assert_value_eq(v, b.get(k).unwrap());
            }

            for (k, v) in b.iter() {
                let is_empty = match v {
                    LoroValue::String(s) => s.is_empty(),
                    LoroValue::List(l) => l.is_empty(),
                    LoroValue::Map(m) => m.is_empty(),
                    _ => false,
                };
                if is_empty {
                    continue;
                }

                assert_value_eq(v, a.get(k).unwrap());
            }
        }
        (a, b) => assert_eq!(a, b),
    }
}

fn check_eq(a_actor: &mut Actor, b_actor: &mut Actor) {
    let a_doc = &mut a_actor.loro;
    let b_doc = &mut b_actor.loro;
    let a_result = a_doc.get_state_deep_value();
    tracing::info!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_doc.get_state_deep_value());
    assert_value_eq(&a_result, &a_actor.value_tracker.lock().unwrap());
    assert_value_eq(&a_result, &b_actor.value_tracker.lock().unwrap());

    let a = a_doc.get_text("text");
    let value_a = a.get_value();
    assert_eq!(
        &**value_a.as_string().unwrap(),
        &*a_actor.text_tracker.lock().unwrap(),
    );

    let a = a_doc.get_map("map");
    let value_a = a.get_value();
    assert_eq!(
        &**value_a.as_map().unwrap(),
        &*a_actor.map_tracker.lock().unwrap()
    );

    let a = a_doc.get_list("list");
    let value_a = a.get_value();
    assert_eq!(
        &**value_a.as_list().unwrap(),
        &*a_actor.list_tracker.lock().unwrap(),
    );
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            let s = tracing::span!(tracing::Level::INFO, "checking", i = i, j = j);
            let _e = s.enter();
            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;

            if (i + j) % 2 == 0 {
                let s = tracing::span!(tracing::Level::INFO, "Updates {} to {}", j, i);
                let _e = s.enter();
                a_doc.import(&b_doc.export_from(&a_doc.oplog_vv())).unwrap();

                let s = tracing::span!(tracing::Level::INFO, "Updates {} to {}", i, j);
                let _e = s.enter();
                b_doc.import(&a_doc.export_from(&b_doc.oplog_vv())).unwrap();
            } else {
                let s = tracing::span!(tracing::Level::INFO, "Snapshot {} to {}", j, i);
                let _e = s.enter();
                a_doc.import(&b_doc.export_snapshot()).unwrap();

                let s = tracing::span!(tracing::Level::INFO, "Snapshot {} to {}", i, j);
                let _e = s.enter();
                b_doc.import(&a_doc.export_snapshot()).unwrap();
            }

            check_eq(a, b);

            if i == 1 {
                a.record_history();
            }
            if j == 1 {
                b.record_history();
            }
        }
    }
}

fn check_history(actor: &mut Actor) {
    assert!(!actor.history.is_empty());
    for (f, v) in actor.history.iter() {
        let f = Frontiers::from(f);
        let s = tracing::span!(
            tracing::Level::INFO,
            "Checkout from ",
            from = ?actor.loro.state_frontiers(),
            to = ?f
        );
        let _e = s.enter();
        actor.loro.checkout(&f).unwrap();
        let actual = actor.loro.get_deep_value();
        assert_value_eq(v, &actual);
        assert_value_eq(v, &actor.value_tracker.lock().unwrap());
    }
}

pub fn normalize(site_num: u8, actions: &mut [Action]) -> Vec<Action> {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(Actor::new(i as u64));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        let sites_ptr: usize = &mut sites as *mut _ as usize;
        #[allow(clippy::blocks_in_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: Test
            let sites = unsafe { &mut *(sites_ptr as *mut Vec<_>) };
            sites.apply_action(&action.clone());
        })
        .is_err()
        {
            break;
        }
    }

    println!("{}", applied.clone().table());
    applied
}

pub fn test_multi_sites(site_num: u8, actions: &mut [Action]) {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(Actor::new(i as u64));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        tracing::info!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    let s = tracing::span!(tracing::Level::INFO, "check synced");
    let _e = s.enter();
    check_synced(&mut sites);

    check_history(&mut sites[1]);
}

#[cfg(test)]
mod failed_tests {
    use crate::fuzz::minify_error;
    use crate::tests::PROPTEST_FACTOR_10;

    use super::normalize;
    use super::test_multi_sites;
    use super::Action;
    use super::Action::*;
    use super::ContainerType as C;
    use super::FuzzValue::*;
    use arbtest::arbitrary::{self, Unstructured};

    fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
        let xs = u.arbitrary::<Vec<Action>>()?;
        if let Err(e) = std::panic::catch_unwind(|| {
            test_multi_sites(site_num, &mut xs.clone());
        }) {
            dbg!(xs);
            println!("{:?}", e);
            panic!()
        } else {
            Ok(())
        }
    }

    #[test]
    fn empty() {
        test_multi_sites(2, &mut [])
    }

    #[test]
    fn insert_container() {
        test_multi_sites(
            2,
            &mut [List {
                site: 171,
                container_idx: 171,
                key: 171,
                value: Container(C::Text),
            }],
        )
    }

    #[test]
    fn insert_container_1() {
        test_multi_sites(
            3,
            &mut [Map {
                site: 2,
                container_idx: 1,
                key: 2,
                value: Container(C::List),
            }],
        )
    }

    #[test]
    fn list_insert_del() {
        test_multi_sites(
            3,
            &mut [
                List {
                    site: 1,
                    container_idx: 78,
                    key: 0,
                    value: I32(16),
                },
                SyncAll,
                List {
                    site: 1,
                    container_idx: 78,
                    key: 0,
                    value: Null,
                },
            ],
        );
    }

    #[test]
    fn fuzz_0() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 63,
                    container_idx: 61,
                    key: 55,
                    value: Null,
                },
                List {
                    site: 55,
                    container_idx: 55,
                    key: 55,
                    value: Null,
                },
                List {
                    site: 98,
                    container_idx: 45,
                    key: 98,
                    value: I32(1650614882),
                },
                List {
                    site: 98,
                    container_idx: 98,
                    key: 98,
                    value: I32(761422434),
                },
                List {
                    site: 98,
                    container_idx: 98,
                    key: 98,
                    value: I32(1650614882),
                },
                List {
                    site: 98,
                    container_idx: 98,
                    key: 98,
                    value: I32(1650614882),
                },
                List {
                    site: 98,
                    container_idx: 96,
                    key: 98,
                    value: I32(1650614882),
                },
                List {
                    site: 98,
                    container_idx: 98,
                    key: 98,
                    value: I32(761422434),
                },
                List {
                    site: 98,
                    container_idx: 98,
                    key: 98,
                    value: I32(1657699061),
                },
                List {
                    site: 98,
                    container_idx: 245,
                    key: 65,
                    value: Container(C::List),
                },
            ],
        )
    }

    #[test]
    fn fuzz_1() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 3,
                    container_idx: 30,
                    key: 0,
                    value: Null,
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 14,
                    value: Null,
                },
                Map {
                    site: 3,
                    container_idx: 248,
                    key: 255,
                    value: Null,
                },
            ],
        );
    }

    #[test]
    fn fuzz_2() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(1616928864),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: I32(1616928864),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: I32(1616928864),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: Container(C::Text),
                },
                List {
                    site: 55,
                    container_idx: 55,
                    key: 55,
                    value: Null,
                },
                SyncAll,
                List {
                    site: 55,
                    container_idx: 64,
                    key: 53,
                    value: Null,
                },
                List {
                    site: 56,
                    container_idx: 56,
                    key: 56,
                    value: Container(C::Text),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 64,
                    container_idx: 64,
                    key: 64,
                    value: I32(1616928864),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: I32(1616928864),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 255,
                    value: I32(7),
                },
                Text {
                    site: 97,
                    container_idx: 225,
                    pos: 97,
                    value: 24929,
                    is_del: false,
                },
            ],
        );
    }

    #[test]
    fn fuzz_3() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(0),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(2),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(3),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(4),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(5),
                },
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: I32(6),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(7),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(8),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(9),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(10),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(11),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(12),
                },
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: I32(13),
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(14),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(15),
                },
            ],
        )
    }

    #[test]
    fn encoding_sub_container() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: Container(C::Tree),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: Container(C::List),
                },
                List {
                    site: 90,
                    container_idx: 96,
                    key: 96,
                    value: I32(1516265568),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 7,
                    value: Container(C::Map),
                },
                SyncAll,
                Map {
                    site: 4,
                    container_idx: 21,
                    key: 64,
                    value: I32(-13828256),
                },
                List {
                    site: 45,
                    container_idx: 89,
                    key: 235,
                    value: I32(2122219134),
                },
            ],
        )
    }

    #[test]
    fn notify_causal_order_check() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 38912,
                    is_del: false,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 138,
                    value: Container(C::List),
                },
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                SyncAll,
            ],
        )
    }

    #[test]
    fn test() {
        arbtest::builder()
            .budget_ms((100 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10) as u64)
            .run(|u| prop(u, 2))
    }

    #[test]
    fn test_3sites() {
        arbtest::builder()
            .budget_ms((100 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10) as u64)
            .run(|u| prop(u, 3))
    }

    #[test]
    fn obs() {
        test_multi_sites(
            2,
            &mut [List {
                site: 1,
                container_idx: 255,
                key: 255,
                value: Container(C::List),
            }],
        );
    }

    #[test]
    fn obs_text() {
        test_multi_sites(
            5,
            &mut [Text {
                site: 0,
                container_idx: 0,
                pos: 0,
                value: 13756,
                is_del: false,
            }],
        )
    }

    #[test]
    fn obs_map() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 225,
                    container_idx: 233,
                    key: 234,
                    value: Container(C::Map),
                },
                Map {
                    site: 0,
                    container_idx: 233,
                    key: 233,
                    value: I32(16777215),
                },
            ],
        )
    }

    #[test]
    fn deleted_container() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                SyncAll,
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: I32(-1734829928),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn should_notify() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                SyncAll,
                Text {
                    site: 4,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
            ],
        );
    }

    #[test]
    fn hierarchy() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 255,
                    value: Container(C::Text),
                },
                Map {
                    site: 3,
                    container_idx: 0,
                    key: 255,
                    value: Container(C::Text),
                },
                SyncAll,
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 4,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn apply_directly() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                SyncAll,
                Text {
                    site: 4,
                    container_idx: 0,
                    pos: 0,
                    value: 39061,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39062,
                    is_del: false,
                },
                SyncAll,
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 5,
                    value: 39063,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn find_path_for_deleted_container() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                Map {
                    site: 1,
                    container_idx: 1,
                    key: 255,
                    value: Container(C::List),
                },
                Map {
                    site: 4,
                    container_idx: 1,
                    key: 9,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn list_unknown() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 139,
                    container_idx: 133,
                    key: 32,
                    value: Container(C::Text),
                },
                List {
                    site: 166,
                    container_idx: 127,
                    key: 207,
                    value: Null,
                },
                Text {
                    site: 203,
                    container_idx: 105,
                    pos: 87,
                    value: 52649,
                    is_del: false,
                },
                List {
                    site: 122,
                    container_idx: 137,
                    key: 41,
                    value: Container(C::List),
                },
            ],
        )
    }

    #[test]
    fn path_issue() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
            ],
        )
    }

    #[test]
    fn unknown_1() {
        test_multi_sites(
            5,
            &mut [
                SyncAll,
                Map {
                    site: 32,
                    container_idx: 0,
                    key: 110,
                    value: Null,
                },
                SyncAll,
                List {
                    site: 90,
                    container_idx: 90,
                    key: 90,
                    value: I32(5921392),
                },
                Text {
                    site: 92,
                    container_idx: 140,
                    pos: 0,
                    value: 0,
                    is_del: false,
                },
                SyncAll,
            ],
        );
    }

    #[test]
    fn cannot_skip_ops_from_deleted_container_due_to_this_case() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 2,
                    value: Container(C::List),
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 255,
                    value: Container(C::List),
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 255,
                    value: Container(C::List),
                },
                List {
                    site: 3,
                    container_idx: 3,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 3,
                    key: 0,
                    value: Container(C::List),
                },
                SyncAll,
                List {
                    site: 0,
                    container_idx: 3,
                    key: 0,
                    value: Container(C::Map),
                },
                List {
                    site: 1,
                    container_idx: 3,
                    key: 1,
                    value: Container(C::Map),
                },
            ],
        )
    }

    #[test]
    fn map_apply() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 255,
                    value: Container(C::Map),
                },
            ],
        )
    }

    #[test]
    fn maybe_because_of_hierarchy() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                Sync { from: 1, to: 2 },
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Sync { from: 1, to: 2 },
                Text {
                    site: 1,
                    container_idx: 2,
                    pos: 0,
                    value: 45232,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn checkout_error() {
        test_multi_sites(
            2,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
            ],
        )
    }

    #[test]
    fn unknown() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 5,
                    value: 152,
                    is_del: false,
                },
                Sync { from: 2, to: 3 },
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 10,
                    value: 2,
                    is_del: true,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                Sync { from: 2, to: 3 },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 16,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 8,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 28,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 41,
                    value: 45232,
                    is_del: false,
                },
                Sync { from: 1, to: 2 },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 48,
                    value: 39064,
                    is_del: false,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(-1734829928),
                },
            ],
        )
    }

    #[test]
    fn list_slice_err() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                SyncAll,
                Map {
                    site: 1,
                    container_idx: 1,
                    key: 37,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn utf16_err() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 1,
                    value: 2,
                    is_del: true,
                },
            ],
        )
    }

    #[test]
    fn fuzz_4() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                SyncAll,
                List {
                    site: 1,
                    container_idx: 0,
                    key: 1,
                    value: Container(C::List),
                },
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: I32(1634495596),
                },
                SyncAll,
                List {
                    site: 1,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn merge_err() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 39064,
                    is_del: false,
                },
                SyncAll,
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 5,
                    value: 2,
                    is_del: true,
                },
            ],
        )
    }

    #[test]
    fn unknown_fuzz_err() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 167,
                    container_idx: 163,
                    key: 255,
                    value: Container(C::List),
                },
                List {
                    site: 144,
                    container_idx: 7,
                    key: 0,
                    value: Container(C::Text),
                },
                SyncAll,
                Text {
                    site: 126,
                    container_idx: 13,
                    pos: 122,
                    value: 0,
                    is_del: false,
                },
                Text {
                    site: 6,
                    container_idx: 191,
                    pos: 249,
                    value: 255,
                    is_del: true,
                },
                Text {
                    site: 126,
                    container_idx: 126,
                    pos: 126,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 126,
                    container_idx: 126,
                    pos: 246,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 126,
                    container_idx: 92,
                    pos: 126,
                    value: 65406,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_5() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 3,
                    value: 4,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 6,
                    value: 32502,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 7,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 255,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 10,
                    value: 12414,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 22,
                    value: 1,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 21,
                    value: 14,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 37265,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 63102,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 22,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 21,
                    value: 14,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 2,
                    value: 5,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 22,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 21,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 65503,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 32304,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 31,
                    value: 10113,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 14,
                    value: 17,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 10,
                    value: 32401,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 9,
                    value: 32502,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 34,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 20,
                    value: 31,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 3,
                    value: 7,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 7,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 16,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 10,
                    value: 19,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 6,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 7,
                    value: 32407,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 6,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 2,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 1,
                    value: 63015,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 31,
                    value: 7,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 29,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 12,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 30,
                    value: 65535,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 23,
                    value: 16,
                    is_del: true,
                },
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: I32(8949861),
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 34695,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 18,
                    value: 32502,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 40,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 26,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 12,
                    value: 12543,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 48,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 19,
                    value: 63015,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 3,
                    value: 17,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 29,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 30,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 11,
                    value: 31,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 6,
                    value: 24,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 2,
                    value: 5,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 32382,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_6_reset_children() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 3,
                    container_idx: 1,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 136,
                    key: 0,
                    value: Container(C::Text),
                },
                Text {
                    site: 1,
                    container_idx: 5,
                    pos: 0,
                    value: 0,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_7_checkout() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                Sync { from: 1, to: 4 },
                List {
                    site: 4,
                    container_idx: 1,
                    key: 0,
                    value: I32(1566399837),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn fuzz_8_checkout() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 16639,
                    is_del: false,
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Text),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 245,
                    value: Container(C::List),
                },
                SyncAll,
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                List {
                    site: 1,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::Tree),
                },
                Text {
                    site: 1,
                    container_idx: 1,
                    pos: 0,
                    value: 47288,
                    is_del: false,
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn fuzz_9_checkout() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 223,
                    value: Container(C::Map),
                },
                List {
                    site: 213,
                    container_idx: 85,
                    key: 85,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 14,
                    value: Container(C::Text),
                },
                Map {
                    site: 223,
                    container_idx: 255,
                    key: 126,
                    value: Container(C::Tree),
                },
                Text {
                    site: 255,
                    container_idx: 255,
                    pos: 120,
                    value: 9215,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_10_checkout() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Tree),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(-167),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                Sync { from: 1, to: 0 },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Tree),
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 255,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 17,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 255,
                    value: Null,
                },
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 4,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 191,
                    value: Container(C::List),
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 17,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 255,
                    value: Null,
                },
                List {
                    site: 2,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 4,
                    container_idx: 2,
                    key: 0,
                    value: Container(C::Map),
                },
                SyncAll,
                Map {
                    site: 2,
                    container_idx: 3,
                    key: 239,
                    value: I32(-1073741988),
                },
                Map {
                    site: 0,
                    container_idx: 3,
                    key: 191,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::Text),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 17,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 3,
                    key: 255,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 0,
                    container_idx: 3,
                    key: 0,
                    value: Container(C::Map),
                },
                Map {
                    site: 0,
                    container_idx: 1,
                    key: 191,
                    value: Container(C::Text),
                },
            ],
        )
    }

    #[test]
    fn fuzz_11_checkout() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 4,
                    container_idx: 0,
                    key: 8,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 8,
                    value: Container(C::Text),
                },
                SyncAll,
                Text {
                    site: 2,
                    container_idx: 1,
                    pos: 0,
                    value: 1918,
                    is_del: false,
                },
                SyncAll,
                Text {
                    site: 1,
                    container_idx: 1,
                    pos: 5,
                    value: 1,
                    is_del: true,
                },
                Sync { from: 1, to: 2 },
                Text {
                    site: 2,
                    container_idx: 1,
                    pos: 1,
                    value: 5,
                    is_del: true,
                },
            ],
        )
    }

    #[test]
    fn fuzz_map_bring_back() {
        test_multi_sites(
            5,
            &mut [
                Map {
                    site: 6,
                    container_idx: 63,
                    key: 255,
                    value: Container(C::Tree),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: Container(C::Map),
                },
                List {
                    site: 96,
                    container_idx: 96,
                    key: 96,
                    value: Null,
                },
                SyncAll,
                Map {
                    site: 223,
                    container_idx: 255,
                    key: 96,
                    value: I32(1616928864),
                },
            ],
        )
    }

    #[test]
    fn fuzz_sub_sub_bring_back() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 65,
                    container_idx: 65,
                    key: 65,
                    value: Container(C::Map),
                },
                Map {
                    site: 50,
                    container_idx: 7,
                    key: 7,
                    value: Container(C::Text),
                },
                Sync { from: 0, to: 112 },
                Text {
                    site: 0,
                    container_idx: 13,
                    pos: 0,
                    value: 47712,
                    is_del: false,
                },
                Sync { from: 50, to: 50 },
                SyncAll,
                Map {
                    site: 59,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::Map),
                },
                List {
                    site: 65,
                    container_idx: 65,
                    key: 112,
                    value: Null,
                },
                List {
                    site: 67,
                    container_idx: 65,
                    key: 65,
                    value: Null,
                },
                Map {
                    site: 7,
                    container_idx: 50,
                    key: 7,
                    value: Container(C::Text),
                },
            ],
        )
    }

    #[test]
    fn fuzz_bring_back_sub_is_other_bring_back() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 255,
                    container_idx: 255,
                    key: 90,
                    value: Container(C::List),
                },
                List {
                    site: 255,
                    container_idx: 255,
                    key: 199,
                    value: Container(C::Text),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                SyncAll,
                Text {
                    site: 147,
                    container_idx: 33,
                    pos: 251,
                    value: 37779,
                    is_del: false,
                },
                Map {
                    site: 0,
                    container_idx: 68,
                    key: 68,
                    value: Null,
                },
                List {
                    site: 75,
                    container_idx: 0,
                    key: 75,
                    value: Null,
                },
                Text {
                    site: 0,
                    container_idx: 177,
                    pos: 0,
                    value: 53883,
                    is_del: true,
                },
            ],
        )
    }

    #[test]
    fn fuzz_sub_sub_is_has_diff() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 251,
                    container_idx: 251,
                    key: 123,
                    value: Container(C::List),
                },
                List {
                    site: 255,
                    container_idx: 251,
                    key: 123,
                    value: Container(C::List),
                },
                List {
                    site: 91,
                    container_idx: 33,
                    key: 126,
                    value: Container(C::List),
                },
                List {
                    site: 255,
                    container_idx: 63,
                    key: 251,
                    value: Container(C::Tree),
                },
                Map {
                    site: 255,
                    container_idx: 148,
                    key: 255,
                    value: Container(C::Text),
                },
                SyncAll,
                Sync { from: 14, to: 140 },
                Map {
                    site: 126,
                    container_idx: 0,
                    key: 58,
                    value: Null,
                },
                List {
                    site: 251,
                    container_idx: 63,
                    key: 255,
                    value: Container(C::Tree),
                },
                List {
                    site: 56,
                    container_idx: 40,
                    key: 255,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn fuzz_12() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 69,
                    container_idx: 69,
                    key: 69,
                    value: Container(C::List),
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 69,
                    value: Container(C::List),
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 4,
                    value: Container(C::Text),
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 69,
                    value: Null,
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 4,
                    container_idx: 255,
                    key: 47,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 69,
                    value: Null,
                },
                Map {
                    site: 47,
                    container_idx: 38,
                    key: 250,
                    value: Null,
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 69,
                    value: Container(C::Tree),
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 255,
                    value: 17919,
                    is_del: true,
                },
                List {
                    site: 69,
                    container_idx: 69,
                    key: 4,
                    value: I32(553672192),
                },
                List {
                    site: 69,
                    container_idx: 14,
                    key: 255,
                    value: Container(C::Tree),
                },
            ],
        )
    }

    #[test]
    fn fuzz_14() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 4,
                    container_idx: 0,
                    pos: 0,
                    value: 34816,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 3,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 10,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 10,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 24,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 0,
                    value: 34816,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 11,
                    value: 0,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 22,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34872,
                    is_del: false,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 136,
                    value: Null,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 32,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 18,
                    value: 36232,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 61,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 63,
                    value: 34878,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 56,
                    value: 34952,
                    is_del: false,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 136,
                    value: I32(-2004318072),
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 13,
                    value: 0,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 50,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 39,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 32,
                    value: 34858,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 25,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 18,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 11,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 4,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34816,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 12936,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 68,
                    value: 34957,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 8,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34877,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 2,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 4,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 13,
                    value: 0,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 18,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 19,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 44,
                    value: 34858,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 30,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 16,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 2,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 62,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 55,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 48,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 41,
                    value: 11,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 62,
                    value: 30,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 67,
                    value: 3976,
                    is_del: false,
                },
                Sync { from: 1, to: 3 },
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 27,
                    value: 34952,
                    is_del: false,
                },
                SyncAll,
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 8,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 27,
                    value: 34952,
                    is_del: false,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: I32(-2004318072),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(-2004318072),
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 20,
                    value: 8,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 28,
                    value: 34877,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 21,
                    value: 16008,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 14,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 7,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 141,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 114,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 138,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34816,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 8,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 16,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 25,
                    value: 34952,
                    is_del: false,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 136,
                    value: I32(-1903260024),
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 4,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 34,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 20,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 6,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 0,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 61,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 54,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 47,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 40,
                    value: 12936,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 33,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 26,
                    value: 34957,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 19,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 12,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 5,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34963,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 138,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 36488,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 8,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 35464,
                    is_del: false,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 136,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34824,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 20,
                    value: 0,
                    is_del: false,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Map {
                    site: 1,
                    container_idx: 0,
                    key: 11,
                    value: Null,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 136,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 4,
                    value: 34952,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 20,
                    value: 8,
                    is_del: true,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 2681,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 15,
                    value: 30976,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 0,
                    value: 136,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 19,
                    value: 34824,
                    is_del: false,
                },
                Text {
                    site: 1,
                    container_idx: 0,
                    pos: 1,
                    value: 34952,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_13() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 150,
                    container_idx: 22,
                    pos: 150,
                    value: 38550,
                    is_del: false,
                },
                Text {
                    site: 43,
                    container_idx: 0,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 120,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 150,
                    container_idx: 22,
                    pos: 150,
                    value: 38550,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 0,
                    pos: 0,
                    value: 16,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Map {
                    site: 0,
                    container_idx: 238,
                    key: 0,
                    value: I32(1950809972),
                },
                Text {
                    site: 20,
                    container_idx: 20,
                    pos: 20,
                    value: 5140,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 29812,
                    is_del: false,
                },
                Map {
                    site: 150,
                    container_idx: 150,
                    key: 150,
                    value: I32(1953789044),
                },
                Text {
                    site: 116,
                    container_idx: 116,
                    pos: 116,
                    value: 116,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_15() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 90,
                    container_idx: 90,
                    key: 90,
                    value: I32(1515870810),
                },
                List {
                    site: 90,
                    container_idx: 175,
                    key: 165,
                    value: I32(1515890085),
                },
                List {
                    site: 90,
                    container_idx: 90,
                    key: 131,
                    value: I32(1520805286),
                },
                Sync { from: 122, to: 90 },
                Sync { from: 165, to: 165 },
                Sync { from: 90, to: 90 },
                List {
                    site: 26,
                    container_idx: 90,
                    key: 131,
                    value: I32(1515879083),
                },
                Sync { from: 165, to: 165 },
                List {
                    site: 90,
                    container_idx: 90,
                    key: 90,
                    value: I32(1509972611),
                },
                List {
                    site: 165,
                    container_idx: 165,
                    key: 165,
                    value: I32(1515870810),
                },
            ],
        )
    }

    #[test]
    fn fuzz_16() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 8,
                    container_idx: 0,
                    key: 92,
                    value: Null,
                },
                Sync { from: 113, to: 7 },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(-1077952577),
                },
                Map {
                    site: 191,
                    container_idx: 191,
                    key: 191,
                    value: Container(C::Text),
                },
                Sync { from: 61, to: 58 },
                List {
                    site: 58,
                    container_idx: 58,
                    key: 58,
                    value: I32(1617542919),
                },
                List {
                    site: 191,
                    container_idx: 191,
                    key: 191,
                    value: Container(C::Text),
                },
                Sync { from: 202, to: 202 },
                List {
                    site: 0,
                    container_idx: 58,
                    key: 58,
                    value: Null,
                },
                List {
                    site: 58,
                    container_idx: 186,
                    key: 58,
                    value: Null,
                },
                Sync { from: 8, to: 92 },
                Sync { from: 191, to: 28 },
                List {
                    site: 0,
                    container_idx: 100,
                    key: 191,
                    value: I32(1618020287),
                },
                List {
                    site: 191,
                    container_idx: 191,
                    key: 191,
                    value: Container(C::Text),
                },
                Sync { from: 202, to: 191 },
                Sync { from: 191, to: 113 },
                List {
                    site: 191,
                    container_idx: 191,
                    key: 191,
                    value: Container(C::Text),
                },
                Map {
                    site: 58,
                    container_idx: 58,
                    key: 245,
                    value: I32(-1077976064),
                },
                List {
                    site: 58,
                    container_idx: 0,
                    key: 100,
                    value: Container(C::Map),
                },
                Map {
                    site: 100,
                    container_idx: 191,
                    key: 191,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn fuzz_17() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 0,
                    value: 27756,
                    is_del: false,
                },
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 2,
                    value: 47288,
                    is_del: false,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                Text {
                    site: 3,
                    container_idx: 0,
                    pos: 10,
                    value: 4,
                    is_del: true,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 27756,
                    is_del: false,
                },
                Sync { from: 3, to: 4 },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 6,
                    value: 27756,
                    is_del: false,
                },
                Sync { from: 4, to: 0 },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 13,
                    value: 15476,
                    is_del: false,
                },
            ],
        )
    }

    #[test]
    fn fuzz_18() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn fuzz_19() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(2),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 1,
                    value: Null,
                },
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn to_minify() {
        minify_error(5, vec![], test_multi_sites, normalize)
    }
}
