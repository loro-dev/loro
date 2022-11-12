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
    // TODO: use set and merge
    map_containers: Vec<Map>,
    // TODO: use set and merge
    list_containers: Vec<List>,
    // TODO: use set and merge
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
                    *key %= list.values_len().max(1) as u8;
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
                    *pos %= text.text_len().max(1) as u8;
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
                let a = &mut a.loro;
                let b = &mut b.loro;
                a.import(b.export(a.vv()));
                b.import(a.export(b.vv()));
            }
            Action::SyncAll => {
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.loro.import(b.loro.export(a.loro.vv()));
                }

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.loro.import(a.loro.export(b.loro.vv()));
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
                        let new = container.insert_obj(&actor.loro, &key.to_string(), c.clone());
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
                        let new = container.insert_obj(&actor.loro, *key as usize, c.clone());
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
                    container.insert(&actor.loro, *pos as usize, &(value.to_string() + " "));
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

pub fn test_multi_sites(site_num: u8, mut actions: Vec<Action>) {
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

#[cfg(test)]
mod failed_tests {
    use crate::ContainerType;

    use super::test_multi_sites;
    use super::Action;
    use super::Action::*;
    use super::FuzzValue::*;
    use arbtest::arbitrary::{self, Unstructured};

    fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
        let xs = u.arbitrary::<Vec<Action>>()?;
        if let Err(e) = std::panic::catch_unwind(|| {
            test_multi_sites(site_num, xs.clone());
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
        arbtest::builder().budget_ms(50_000).run(|u| prop(u, 2))
    }

    #[test]
    fn test_3sites() {
        arbtest::builder().budget_ms(50_000).run(|u| prop(u, 3))
    }

    #[test]
    fn case_0() {
        test_multi_sites(
            8,
            vec![Map {
                site: 73,
                container_idx: 73,
                key: 73,
                value: Container(ContainerType::Text),
            }],
        )
    }

    use super::ContainerType as C;
    #[test]
    fn case_1() {
        test_multi_sites(
            8,
            vec![
                List {
                    site: 49,
                    container_idx: 209,
                    key: 0,
                    value: I32(1),
                },
                List {
                    site: 64,
                    container_idx: 45,
                    key: 0,
                    value: I32(2),
                },
                SyncAll,
                Map {
                    site: 0,
                    container_idx: 0,
                    key: 0,
                    value: I32(1229062019),
                },
                List {
                    site: 73,
                    container_idx: 0,
                    key: 0,
                    value: Null,
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

    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
