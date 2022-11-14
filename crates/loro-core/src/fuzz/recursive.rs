use std::{collections::HashSet, time::Instant};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    array_mut_ref,
    container::{registry::ContainerWrapper, ContainerID},
    debug_log,
    id::ClientID,
    ContainerType, List, LoroCore, LoroValue, Map, Text,
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
    site: ClientID,
    loro: LoroCore,
    map_containers: Vec<Map>,
    list_containers: Vec<List>,
    text_containers: Vec<Text>,
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
                    *key %= (list.values_len() as u8).max(1);
                    if *value == FuzzValue::Null && list.values_len() == 0 {
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
                    *pos %= (text.text_len() as u8).max(1);
                    if *is_del {
                        *value &= 0x1f;
                        *value = (*value).min(text.text_len() as u16 - (*pos) as u16);
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
                let container = if container.is_none() {
                    let map = actor.loro.get_map("map");
                    actor.map_containers.push(map);
                    &mut actor.map_containers[0]
                } else {
                    container.unwrap()
                };

                match value {
                    FuzzValue::Null => {
                        container.delete(&actor.loro, &key.to_string());
                    }
                    FuzzValue::I32(i) => {
                        container.insert(&actor.loro, &key.to_string(), *i);
                    }
                    FuzzValue::Container(c) => {
                        let new = container.insert_obj(&actor.loro, &key.to_string(), *c);
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
                    container.unwrap()
                };

                match value {
                    FuzzValue::Null => {
                        container.delete(&actor.loro, *key as usize, 1);
                    }
                    FuzzValue::I32(i) => {
                        container.insert(&actor.loro, *key as usize, *i);
                    }
                    FuzzValue::Container(c) => {
                        let new = container.insert_obj(&actor.loro, *key as usize, *c);
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
                let container = if container.is_none() {
                    let text = actor.loro.get_text("text");
                    actor.text_containers.push(text);
                    &mut actor.text_containers[0]
                } else {
                    container.unwrap()
                };
                if *is_del {
                    container.delete(&actor.loro, *pos as usize, *value as usize);
                } else {
                    container.insert(&actor.loro, *pos as usize, &(format!("[{}]", value)));
                }
            }
        }
    }
}

fn check_eq(site_a: &mut LoroCore, site_b: &mut LoroCore) {
    let a = site_a.get_text("text");
    let b = site_b.get_text("text");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
    let a = site_a.get_map("map");
    let b = site_b.get_map("map");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
    let a = site_a.get_list("list");
    let b = site_b.get_list("list");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a, value_b);
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log!("-------------------------------");
            debug_log!("checking {} with {}", i, j);
            debug_log!("-------------------------------");

            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a = &mut a.loro;
            let b = &mut b.loro;
            a.import(b.export(a.vv()));
            b.import(a.export(b.vv()));
            check_eq(a, b)
        }
    }
}

pub fn normalize(site_num: u8, actions: &mut [Action]) -> Vec<Action> {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(Actor {
            site: i as u64,
            loro: LoroCore::new(Default::default(), Some(i as u64)),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
        });
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        let sites_ptr: *mut Vec<_> = &mut sites as *mut _;
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
        sites.push(Actor {
            site: i as u64,
            loro: LoroCore::new(Default::default(), Some(i as u64)),
            map_containers: Default::default(),
            list_containers: Default::default(),
            text_containers: Default::default(),
        });
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

pub fn minify_error(site_num: u8, actions: Vec<Action>) {
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
    }));

    if std::panic::catch_unwind(|| test_multi_sites(site_num, &mut actions.clone())).is_ok() {
        println!("No Error Found");
        return;
    }

    let mut minified = actions.clone();
    let mut candidates = Vec::new();
    for i in 0..actions.len() {
        let mut new = actions.clone();
        new.remove(i);
        candidates.push(new);
    }

    println!("Minifying...");
    let start = Instant::now();
    while let Some(candidate) = candidates.pop() {
        if std::panic::catch_unwind(|| test_multi_sites(site_num, &mut candidate.clone())).is_err()
        {
            for i in 0..candidate.len() {
                let mut new = candidate.clone();
                new.remove(i);
                candidates.push(new);
            }
            if candidate.len() < minified.len() {
                minified = candidate;
                println!("New min len={}", minified.len());
            }
            if candidates.len() > 40 {
                candidates.drain(0..30);
            }
        }
        if start.elapsed().as_secs() > 10 {
            if minified.len() <= 4 {
                break;
            }
        }
        if start.elapsed().as_secs() > 60 {
            break;
        }
    }

    let minified = normalize(site_num, &mut minified);
    println!(
        "Old Length {}, New Length {}",
        actions.len(),
        minified.len()
    );
    dbg!(&minified);
    if actions.len() > minified.len() {
        minify_error(site_num, minified);
    }
}

#[cfg(test)]
mod failed_tests {
    use crate::tests::PROPTEST_FACTOR_10;
    use crate::ContainerType;

    use super::minify_error;
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
    fn case_0() {
        test_multi_sites(
            8,
            &mut [Map {
                site: 73,
                container_idx: 73,
                key: 73,
                value: Container(ContainerType::Text),
            }],
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
    fn case_1() {
        minify_error(
            5,
            vec![
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

    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
