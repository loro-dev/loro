use std::{collections::HashSet, fmt::Debug};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use rle::HasLength;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    array_mut_ref,
    container::{registry::ContainerWrapper, ContainerID},
    debug_log,
    event::Diff,
    id::ClientID,
    ContainerType, InternalString, List, LoroCore, LoroValue, Map, Text,
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
    loro: LoroCore,
    map_tracker: FxHashMap<String, LoroValue>,
    list_tracker: Vec<LoroValue>,
    text_tracker: String,
    map_containers: Vec<Map>,
    list_containers: Vec<List>,
    text_containers: Vec<Text>,
}

impl Actor {
    fn new(id: ClientID) -> Self {
        let mut actor = Actor {
            loro: LoroCore::new(Default::default(), Some(id)),
            map_tracker: Default::default(),
            list_tracker: Default::default(),
            text_tracker: Default::default(),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
        };

        // SAFETY: test only code
        let text: &mut String = unsafe { std::mem::transmute(&mut actor.text_tracker) };
        actor.loro.log_store.write().unwrap().hierarchy.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Box::new(|event| {
                for diff in event.diff.iter() {
                    match diff {
                        Diff::Text(delta) => {
                            let mut index = 0;
                            for item in delta.iter() {
                                match item {
                                    crate::delta::DeltaItem::Retain { len, meta: _ } => {
                                        index += len;
                                    }
                                    crate::delta::DeltaItem::Insert { value, meta: _ } => {
                                        // text.insert_str(index, value);
                                        index += value.len();
                                    }
                                    crate::delta::DeltaItem::Delete(len) => {
                                        // text.drain(index..index + *len);
                                    }
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }),
            false,
        );

        let map: &mut FxHashMap<InternalString, LoroValue> =
            // SAFETY: test only code
            unsafe { std::mem::transmute(&mut actor.map_tracker) };
        actor.loro.log_store.write().unwrap().hierarchy.subscribe(
            &ContainerID::new_root("map", ContainerType::Map),
            Box::new(|event| {
                for diff in event.diff.iter() {
                    match diff {
                        Diff::Map(map_diff) => {
                            for (key, value) in map_diff.added.iter() {
                                // map.insert(key.clone(), value.clone());
                            }
                            for key in map_diff.deleted.iter() {
                                // map.remove(key);
                            }
                            for (key, value) in map_diff.updated.iter() {
                                // map.insert(key.clone(), value.new.clone());
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }),
            false,
        );

        // SAFETY: test only code
        let list: &mut Vec<LoroValue> = unsafe { std::mem::transmute(&mut actor.list_tracker) };
        actor.loro.log_store.write().unwrap().hierarchy.subscribe(
            &ContainerID::new_root("list", ContainerType::List),
            Box::new(|event| {
                for diff in event.diff.iter() {
                    match diff {
                        Diff::List(delta) => {
                            let mut index = 0;
                            for item in delta.iter() {
                                match item {
                                    crate::delta::DeltaItem::Retain { len, meta: _ } => {
                                        index += len;
                                    }
                                    crate::delta::DeltaItem::Insert { value, meta: _ } => {
                                        for v in value {
                                            // list.insert(index, v.clone());
                                            index += 1;
                                        }
                                    }
                                    crate::delta::DeltaItem::Delete(len) => {
                                        // list.drain(index..index + *len);
                                    }
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }),
            false,
        );

        actor
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
    fn add_new_container(&mut self, new: ContainerID) {
        match new.container_type() {
            ContainerType::Text => self.text_containers.push(self.loro.get_text(new)),
            ContainerType::Map => self.map_containers.push(self.loro.get_map(new)),
            ContainerType::List => self.list_containers.push(self.loro.get_list(new)),
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

                a.loro.import(b.loro.export(a.loro.vv()));
                b.loro.import(a.loro.export(b.loro.vv()));

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
                    a.loro.import(b.loro.export(a.loro.vv()));
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
                    b.loro.import(a.loro.export(b.loro.vv()));
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
                        container.delete(&actor.loro, &key.to_string()).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container.insert(&actor.loro, &key.to_string(), *i).unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let new = container
                            .insert(&actor.loro, &key.to_string(), *c)
                            .unwrap()
                            .unwrap();
                        actor.add_new_container(new);
                    }
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

                match value {
                    FuzzValue::Null => {
                        container.delete(&actor.loro, *key as usize, 1).unwrap();
                    }
                    FuzzValue::I32(i) => {
                        container.insert(&actor.loro, *key as usize, *i).unwrap();
                    }
                    FuzzValue::Container(c) => {
                        let new = container
                            .insert(&actor.loro, *key as usize, *c)
                            .unwrap()
                            .unwrap();
                        actor.add_new_container(new)
                    }
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
                if *is_del {
                    container
                        .delete(&actor.loro, *pos as usize, *value as usize)
                        .unwrap();
                } else {
                    container
                        .insert(&actor.loro, *pos as usize, &(format!("[{}]", value)))
                        .unwrap();
                }
            }
        }
    }
}

fn check_eq(a_actor: &mut Actor, b_actor: &mut Actor) {
    let a_doc = &mut a_actor.loro;
    let b_doc = &mut b_actor.loro;
    let a = a_doc.get_text("text");
    let b = b_doc.get_text("text");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
    // assert_eq!(&**value_a.as_string().unwrap(), &a_actor.text_tracker);

    let a = a_doc.get_map("map");
    let b = b_doc.get_map("map");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
    // assert_eq!(&**value_a.as_map().unwrap(), &a_actor.map_tracker);

    let a = a_doc.get_list("list");
    let b = b_doc.get_list("list");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
    // assert_eq!(&**value_a.as_list().unwrap(), &a_actor.list_tracker);
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log!("-------------------------------");
            debug_log!("checking {} with {}", i, j);
            debug_log!("-------------------------------");

            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;
            a_doc.import(b_doc.export(a_doc.vv()));
            b_doc.import(a_doc.export(b_doc.vv()));
            check_eq(a, b)
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
        let sites_ptr: *mut Vec<_> = &mut sites as *mut _;
        #[allow(clippy::blocks_in_if_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: Test
            let sites = unsafe { &mut *sites_ptr };
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
        debug_log!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    debug_log!("=================================");
    // println!("{}", actions.table());
    check_synced(&mut sites);
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
    fn test() {
        arbtest::builder()
            .budget_ms((10000 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10) as u64)
            .run(|u| prop(u, 2))
    }

    #[test]
    fn test_3sites() {
        arbtest::builder()
            .budget_ms((10000 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10) as u64)
            .run(|u| prop(u, 3))
    }

    #[test]
    fn case_3() {
        test_multi_sites(
            3,
            &mut [
                List {
                    site: 49,
                    container_idx: 71,
                    key: 179,
                    value: Container(C::Map),
                },
                SyncAll,
                Map {
                    site: 130,
                    container_idx: 8,
                    key: 203,
                    value: Container(C::Map),
                },
                List {
                    site: 85,
                    container_idx: 129,
                    key: 244,
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn case_0() {
        test_multi_sites(
            3,
            &mut [
                Map {
                    site: 2,
                    container_idx: 0,
                    key: 255,
                    value: Container(C::List),
                },
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
                    value: Null,
                },
            ],
        )
    }

    #[test]
    fn case_2() {
        test_multi_sites(
            8,
            &mut [
                List {
                    site: 1,
                    container_idx: 0,
                    key: 0,
                    value: Container(C::List),
                },
                List {
                    site: 4,
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
                    value: Container(C::List),
                },
                Sync { from: 1, to: 0 },
                List {
                    site: 4,
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
                Sync { from: 4, to: 0 },
                Sync { from: 1, to: 0 },
                List {
                    site: 4,
                    container_idx: 1,
                    key: 0,
                    value: Null,
                },
                List {
                    site: 1,
                    container_idx: 1,
                    key: 0,
                    value: Container(C::List),
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
