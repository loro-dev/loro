use std::{
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use arbitrary::Arbitrary;
use debug_log::debug_dbg;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::ID;
use tabled::{TableIteratorExt, Tabled};

#[allow(unused_imports)]
use crate::{
    array_mut_ref, container::ContainerID, delta::DeltaItem, event::Diff, id::PeerID,
    ContainerType, LoroValue,
};
use crate::{
    container::{idx::ContainerIdx, richtext::richtext_state::unicode_to_utf8_index},
    handler::TextHandler,
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
        app.set_peer_id(id);
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

        let root_value = Arc::clone(&actor.value_tracker);
        actor.loro.subscribe_deep(Arc::new(move |event| {
            let mut root_value = root_value.lock().unwrap();
            // debug_log::debug_dbg!(&event);
            root_value.apply(
                &event.container.path.iter().map(|x| x.1.clone()).collect(),
                &[event.container.diff.clone()],
            );
        }));

        let text = Arc::clone(&actor.text_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Arc::new(move |event| {
                if event.from_children {
                    return;
                }

                let mut text = text.lock().unwrap();
                match &event.container.diff {
                    Diff::Text(delta) => {
                        let mut index = 0;
                        for item in delta.iter() {
                            match item {
                                DeltaItem::Retain { len, meta: _ } => {
                                    index += len;
                                }
                                DeltaItem::Insert { value, meta: _ } => {
                                    let utf8_index = unicode_to_utf8_index(&text, index).unwrap();
                                    text.insert_str(utf8_index, value.as_str());
                                    index += value.len_unicode();
                                }
                                DeltaItem::Delete { len, .. } => {
                                    text.drain(index..index + *len);
                                }
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }),
        );

        let map = Arc::clone(&actor.map_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("map", ContainerType::Map),
            Arc::new(move |event| {
                if event.from_children {
                    return;
                }
                let mut map = map.lock().unwrap();
                if let Diff::NewMap(map_diff) = &event.container.diff {
                    for (key, value) in map_diff.updated.iter() {
                        match &value.value {
                            Some(value) => {
                                map.insert(key.to_string(), value.clone());
                            }
                            None => {
                                map.remove(&key.to_string());
                            }
                        }
                    }
                } else {
                    debug_dbg!(&event.container);
                    unreachable!()
                }
            }),
        );

        let list = Arc::clone(&actor.list_tracker);
        actor.loro.subscribe(
            &ContainerID::new_root("list", ContainerType::List),
            Arc::new(move |event| {
                if event.from_children {
                    return;
                }
                let mut list = list.lock().unwrap();
                if let Diff::List(delta) = &event.container.diff {
                    let mut index = 0;
                    for item in delta.iter() {
                        match item {
                            DeltaItem::Retain { len, meta: _ } => {
                                index += len;
                            }
                            DeltaItem::Insert { value, meta: _ } => {
                                for v in value {
                                    list.insert(index, v.clone());
                                    index += 1;
                                }
                            }
                            DeltaItem::Delete { len, .. } => {
                                list.drain(index..index + *len);
                            }
                        }
                    }
                } else {
                    unreachable!()
                }
            }),
        );
        actor
            .text_containers
            .push(actor.loro.txn().unwrap().get_text("text"));
        actor
            .map_containers
            .push(actor.loro.txn().unwrap().get_map("map"));
        actor
            .list_containers
            .push(actor.loro.txn().unwrap().get_list("list"));
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
            FuzzValue::I32(i) => LoroValue::I32(i),
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
        match type_ {
            ContainerType::Text => self
                .text_containers
                .push(TextHandler::new(idx, Arc::downgrade(self.loro.app_state()))),
            ContainerType::Map => self
                .map_containers
                .push(MapHandler::new(idx, Arc::downgrade(self.loro.app_state()))),
            ContainerType::List => self
                .list_containers
                .push(ListHandler::new(idx, Arc::downgrade(self.loro.app_state()))),
            ContainerType::Text => {
                // TODO richtext
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
                        container.delete(&mut txn, &key.to_string()).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container
                            .insert(&mut txn, &key.to_string(), LoroValue::from(*i))
                            .unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let idx = container
                            .insert_container(&mut txn, &key.to_string(), *c)
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
                        container.delete(&mut txn, *key as usize, 1).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container
                            .insert(&mut txn, *key as usize, LoroValue::from(*i))
                            .unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let idx = container
                            .insert_container(&mut txn, *key as usize, *c)
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
                        .delete(&mut txn, *pos as usize, *value as usize)
                        .unwrap();
                } else {
                    container
                        .insert(&mut txn, *pos as usize, &(format!("[{}]", value)))
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
    debug_log::debug_log!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_doc.get_state_deep_value());
    assert_value_eq(&a_result, &a_actor.value_tracker.lock().unwrap());

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
            debug_log::group!("checking {} with {}", i, j);
            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;
            if (i + j) % 2 == 0 {
                debug_log::group!("Updates {} to {}", j, i);
                a_doc.import(&b_doc.export_from(&a_doc.oplog_vv())).unwrap();
                debug_log::group_end!();
                debug_log::group!("Updates {} to {}", i, j);
                b_doc.import(&a_doc.export_from(&b_doc.oplog_vv())).unwrap();
                debug_log::group_end!();
            } else {
                debug_log::group!("Snapshot {} to {}", j, i);
                a_doc.import(&b_doc.export_snapshot()).unwrap();
                debug_log::group_end!();
                debug_log::group!("Snapshot {} to {}", i, j);
                b_doc.import(&a_doc.export_snapshot()).unwrap();
                debug_log::group_end!();
            }

            check_eq(a, b);
            debug_log::group_end!();
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
    for (c, (f, v)) in actor.history.iter().enumerate() {
        let f = Frontiers::from(f);
        actor.loro.checkout(&f).unwrap();
        let actual = actor.loro.get_deep_value();
        assert_eq!(v, &actual, "Version mismatched at {:?}, cnt={}", f, c);
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
        #[allow(clippy::blocks_in_if_conditions)]
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
        debug_log::debug_log!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    debug_log::group!("check synced");
    check_synced(&mut sites);
    debug_log::group_end!();
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


    use super::ContainerType as C;
    #[test]
    fn to_minify() {
        minify_error(5, vec![], test_multi_sites, normalize)
    }
}
