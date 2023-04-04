use std::{
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, Mutex},
};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    array_mut_ref,
    container::{registry::ContainerIdx, ContainerID},
    delta::DeltaItem,
    event::{Diff, Observer},
    id::ClientID,
    transaction::TransactionWrap,
    ContainerType, EncodeMode, List, LoroCore, LoroValue, Map, Text, Transact,
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
    txn: TransactionWrap,
    loro: LoroCore,
    value_tracker: Arc<Mutex<LoroValue>>,
    map_tracker: Arc<Mutex<FxHashMap<String, LoroValue>>>,
    list_tracker: Arc<Mutex<Vec<LoroValue>>>,
    text_tracker: Arc<Mutex<String>>,
    map_containers: Vec<Map>,
    list_containers: Vec<List>,
    text_containers: Vec<Text>,
}

impl Actor {
    fn new(id: ClientID) -> Self {
        let loro = LoroCore::new(Default::default(), Some(id));
        let txn = loro.transact();
        let mut actor = Actor {
            loro,
            txn,
            value_tracker: Arc::new(Mutex::new(LoroValue::Map(Default::default()))),
            map_tracker: Default::default(),
            list_tracker: Default::default(),
            text_tracker: Default::default(),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
        };

        let root_value = Arc::clone(&actor.value_tracker);
        actor.loro.subscribe_deep(Box::new(move |event| {
            let mut root_value = root_value.lock().unwrap();
            root_value.apply(&event.relative_path, &[event.diff.clone()]);
        }));

        let log_store = actor.loro.log_store.write().unwrap();
        let mut hierarchy = actor.loro.hierarchy.try_lock().unwrap();
        let text = Arc::clone(&actor.text_tracker);
        hierarchy.subscribe(Observer::new_container(
            Box::new(move |event| {
                let mut text = text.lock().unwrap();
                match &event.diff {
                    Diff::Text(delta) => {
                        let mut index = 0;
                        for item in delta.iter() {
                            match item {
                                DeltaItem::Retain { len, meta: _ } => {
                                    index += len;
                                }
                                DeltaItem::Insert { value, meta: _ } => {
                                    text.insert_str(index, value);
                                    index += value.len();
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
            ContainerID::new_root("text", ContainerType::Text),
        ));

        let map = Arc::clone(&actor.map_tracker);
        hierarchy.subscribe(Observer::new_container(
            Box::new(move |event| {
                let mut map = map.lock().unwrap();
                match &event.diff {
                    Diff::Map(map_diff) => {
                        for (key, value) in map_diff.added.iter() {
                            map.insert(key.to_string(), value.clone());
                        }
                        for (key, _v) in map_diff.deleted.iter() {
                            // map.remove(&key.to_string());
                            map.insert(key.to_string(), LoroValue::Null);
                        }
                        for (key, value) in map_diff.updated.iter() {
                            map.insert(key.to_string(), value.new.clone());
                        }
                    }
                    _ => unreachable!(),
                }
            }),
            ContainerID::new_root("map", ContainerType::Map),
        ));

        let list = Arc::clone(&actor.list_tracker);
        hierarchy.subscribe(Observer::new_container(
            Box::new(move |event| {
                let mut list = list.lock().unwrap();
                match &event.diff {
                    Diff::List(delta) => {
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
                    }
                    _ => unreachable!(),
                }
            }),
            ContainerID::new_root("list", ContainerType::List),
        ));

        drop(hierarchy);
        drop(log_store);
        actor.text_containers.push(actor.loro.get_text("text"));
        actor.map_containers.push(actor.loro.get_map("map"));
        actor.list_containers.push(actor.loro.get_list("list"));
        actor
    }

    fn commit(&mut self) {
        let new_txn = self.loro.transact();
        let txn = std::mem::replace(&mut self.txn, new_txn);
        drop(txn);
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
                format!("{} to {}", from, to).into(),
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
        let store = self.loro.log_store.try_read().unwrap();
        let client_id = store.this_client_id;
        let instance = store.get_container_by_idx(&idx).unwrap();
        match type_ {
            ContainerType::Text => self
                .text_containers
                .push(Text::from_instance(instance, client_id)),
            ContainerType::Map => self
                .map_containers
                .push(Map::from_instance(instance, client_id)),
            ContainerType::List => self
                .list_containers
                .push(List::from_instance(instance, client_id)),
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
                    *pos %= (text.len() as u8).max(1);
                    if *is_del {
                        *value &= 0x1f;
                        *value = (*value).min(text.len() as u16 - (*pos) as u16);
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
                // commit txn
                a.commit();
                b.commit();
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

                a.loro.import(b.loro.export(a.loro.vv_cloned()));
                b.loro.import(a.loro.export(b.loro.vv_cloned()));

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
                a.commit();
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
                    a.commit();
                    b.commit();
                    a.loro.import(b.loro.export(a.loro.vv_cloned()));
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
                    a.commit();
                    b.commit();
                    b.loro.import(a.loro.export(b.loro.vv_cloned()));
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
            }
            Action::Map {
                site,
                container_idx,
                key,
                value,
            } => {
                let actor = &mut self[*site as usize];
                let txn = &actor.txn;
                let container = actor.map_containers.get_mut(*container_idx as usize);
                let container = if let Some(container) = container {
                    container
                } else {
                    let map = actor.loro.get_map("map");
                    actor.map_containers.push(map);
                    &mut actor.map_containers[0]
                };
                match value {
                    FuzzValue::Null => {
                        container.delete(txn, &key.to_string()).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container.insert(txn, &key.to_string(), *i).unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let idx = container.insert(txn, &key.to_string(), *c).unwrap();
                        if let Some(container) = idx {
                            actor.add_new_container(container, *c);
                        }
                    }
                };
            }
            Action::List {
                site,
                container_idx,
                key,
                value,
            } => {
                let actor = &mut self[*site as usize];
                let txn = &actor.txn;
                let container = actor.list_containers.get_mut(*container_idx as usize);
                let container = if container.is_none() {
                    let list = actor.loro.get_list("list");
                    actor.list_containers.push(list);
                    &mut actor.list_containers[0]
                } else {
                    #[allow(clippy::unnecessary_unwrap)]
                    container.unwrap()
                };
                match value {
                    FuzzValue::Null => {
                        container.delete(txn, *key as usize, 1).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container.insert(txn, *key as usize, *i).unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let container = container.insert(txn, *key as usize, *c).unwrap();
                        if let Some(container) = container {
                            actor.add_new_container(container, *c);
                        }
                    }
                };
            }
            Action::Text {
                site,
                container_idx,
                pos,
                value,
                is_del,
            } => {
                let actor = &mut self[*site as usize];
                let txn = &actor.txn;
                let container = actor.text_containers.get_mut(*container_idx as usize);
                let container = if let Some(container) = container {
                    container
                } else {
                    let text = actor.loro.get_text("text");
                    actor.text_containers.push(text);
                    &mut actor.text_containers[0]
                };
                if *is_del {
                    container
                        .delete(txn, *pos as usize, *value as usize)
                        .unwrap();
                } else {
                    container
                        .insert(txn, *pos as usize, &(format!("[{}]", value)))
                        .unwrap();
                }
            }
        }
    }
}

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
    let a_result = a_doc.to_json();
    debug_log::debug_log!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_doc.to_json());
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

    use itertools::Itertools;
    for key in a_doc
        .log_store
        .try_read()
        .unwrap()
        .changes()
        .keys()
        .sorted()
    {
        let as_ = a_doc.log_store.try_read().unwrap();
        let ca = as_.changes().get(key).unwrap();
        let bs = b_doc.log_store.try_read().unwrap();
        let cb = bs.changes().get(key).unwrap();
        for (la, lb) in ca.iter().zip(cb.iter()) {
            assert_eq!(la.lamport, lb.lamport);
            assert_eq!(la.id, lb.id);
        }
    }
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log::group!("checking {} with {}", i, j);
            let (a, b) = array_mut_ref!(sites, [i, j]);
            a.commit();
            b.commit();
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;
            if i % 2 == 0 {
                a_doc
                    .decode(&b_doc.encode_with_cfg(EncodeMode::RleUpdates(a_doc.vv_cloned())))
                    .unwrap();
                b_doc
                    .decode(&a_doc.encode_with_cfg(EncodeMode::Updates(b_doc.vv_cloned())))
                    .unwrap();
            } else {
                a_doc
                    .decode(&b_doc.encode_with_cfg(EncodeMode::RleUpdates(a_doc.vv_cloned())))
                    .unwrap();
                b_doc.decode(&a_doc.encode_all()).unwrap();
            }
            check_eq(a, b);
            debug_log::group_end!();
        }
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

    println!("table\n{}", applied.clone().table());
    dbg!(&applied);
    applied
}

pub fn test_multi_sites(site_num: u8, actions: &mut [Action]) {
    // println!("{:?}", actions);
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

    // println!("{}", actions.table());
    debug_log::group!("check synced");
    check_synced(&mut sites);
    debug_log::group_end!();
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
                SyncAll,
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 5,
                    value: 39064,
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
            3,
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
            2,
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
    fn delta_compose() {
        test_multi_sites(
            2,
            &mut [
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 0,
                    value: 31354,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 3,
                    value: 31354,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 10,
                    value: 31354,
                    is_del: false,
                },
                SyncAll,
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 3,
                    value: 31354,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 2,
                    value: 31354,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 29,
                    value: 31354,
                    is_del: false,
                },
                Text {
                    site: 2,
                    container_idx: 0,
                    pos: 27,
                    value: 3,
                    is_del: true,
                },
            ],
        )
    }

    #[test]
    fn delete_container() {
        test_multi_sites(
            2,
            &mut [
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
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
            ],
        )
    }

    #[test]
    fn ordered_event() {
        test_multi_sites(
            2,
            &mut [
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(2054847098),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(1600085855),
                },
                List {
                    site: 0,
                    container_idx: 0,
                    key: 1,
                    value: Container(C::List),
                },
                SyncAll,
                List {
                    site: 2,
                    container_idx: 1,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn unknown_1() {
        test_multi_sites(
            5,
            &mut [
                List {
                    site: 3,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 3,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::List),
                },
                Sync { from: 2, to: 3 },
                List {
                    site: 2,
                    container_idx: 2,
                    key: 0,
                    value: I32(-1650614883),
                },
            ],
        )
    }

    #[test]
    fn utf16() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 0,
                    pos: 0,
                    value: 0,
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
                    site: 10,
                    container_idx: 146,
                    key: 146,
                    value: I32(-1835887982),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 63378,
                    is_del: true,
                },
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                SyncAll,
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 0,
                    value: 0,
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
                    value: I32(-1835887982),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 0,
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
                    site: 146,
                    container_idx: 146,
                    key: 146,
                    value: I32(-1835887982),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
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
            ],
        )
    }

    #[test]
    fn utf16_1() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 6,
                    value: I32(27038354),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37530,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 146,
                    is_del: false,
                },
                Text {
                    site: 0,
                    container_idx: 0,
                    pos: 0,
                    value: 0,
                    is_del: true,
                },
                SyncAll,
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
                    value: Container(C::List),
                },
                SyncAll,
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 0,
                    value: 0,
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
                    container_idx: 101,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 767,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 65426,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 79,
                    value: 20303,
                    is_del: true,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 255,
                    key: 2,
                    value: I32(-1835887982),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 20370,
                    is_del: true,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                List {
                    site: 79,
                    container_idx: 79,
                    key: 79,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 2,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 146,
                    container_idx: 146,
                    key: 146,
                    value: I32(0),
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
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
            ],
        );
    }

    #[test]
    fn utf16_2() {
        test_multi_sites(
            5,
            &mut [
                Text {
                    site: 114,
                    container_idx: 114,
                    pos: 114,
                    value: 29298,
                    is_del: false,
                },
                Text {
                    site: 114,
                    container_idx: 114,
                    pos: 114,
                    value: 29298,
                    is_del: false,
                },
                Text {
                    site: 114,
                    container_idx: 114,
                    pos: 114,
                    value: 29298,
                    is_del: false,
                },
                Text {
                    site: 114,
                    container_idx: 114,
                    pos: 114,
                    value: 29298,
                    is_del: false,
                },
                Text {
                    site: 114,
                    container_idx: 114,
                    pos: 114,
                    value: 114,
                    is_del: true,
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
                List {
                    site: 73,
                    container_idx: 73,
                    key: 73,
                    value: Container(C::List),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 73,
                    container_idx: 73,
                    key: 73,
                    value: Container(C::List),
                },
                List {
                    site: 255,
                    container_idx: 255,
                    key: 255,
                    value: Container(C::List),
                },
                SyncAll,
                SyncAll,
                SyncAll,
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
                    value: I32(-1835887982),
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 4754,
                    is_del: false,
                },
                Text {
                    site: 146,
                    container_idx: 146,
                    pos: 146,
                    value: 37522,
                    is_del: false,
                },
                Map {
                    site: 0,
                    container_idx: 45,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 73,
                    key: 73,
                    value: I32(-1650614883),
                },
                Sync { from: 157, to: 157 },
                Sync { from: 157, to: 157 },
                Sync { from: 157, to: 99 },
                Sync { from: 157, to: 157 },
                Sync { from: 157, to: 157 },
                Sync { from: 157, to: 157 },
                Sync { from: 157, to: 157 },
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
                    value: I32(-1650614883),
                },
                Sync { from: 157, to: 157 },
                Sync { from: 0, to: 0 },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(-1835887982),
                },
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: Null,
                },
                Map {
                    site: 0,
                    container_idx: 146,
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
            ],
        )
    }

    use super::ContainerType as C;
    #[test]
    fn to_minify() {
        minify_error(5, vec![], test_multi_sites, normalize)
    }

    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
