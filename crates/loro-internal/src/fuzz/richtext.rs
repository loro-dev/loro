use std::{fmt::Debug, sync::Arc};

use arbitrary::Arbitrary;
use debug_log::debug_dbg;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::ID;
use tabled::{TableIteratorExt, Tabled};

#[allow(unused_imports)]
use crate::{
    array_mut_ref, container::ContainerID, delta::DeltaItem, id::PeerID, ContainerType, LoroValue,
};
use crate::{
    container::richtext::{ExpandType, StyleKey, TextStyleInfoFlag},
    event::Diff,
    handler::TextDelta,
    loro::LoroDoc,
    value::ToJson,
    version::Frontiers,
    TextHandler,
};

// TODO: how to test style with id?
const STYLES: [TextStyleInfoFlag; 4] = [
    TextStyleInfoFlag::BOLD,
    // TextStyleInfoFlag::COMMENT,
    TextStyleInfoFlag::LINK,
    // TextStyleInfoFlag::from_byte(0),
    TextStyleInfoFlag::LINK.to_delete(),
    TextStyleInfoFlag::BOLD.to_delete(),
    // TextStyleInfoFlag::COMMENT.to_delete(),
    // TextStyleInfoFlag::from_byte(0).to_delete(),
];

const STYLES_NAME: [&str; 4] = [
    "BOLD", // "COMMENT",
    "LINK", // "0",
    "DEL_LINK", "DEL_BOLD", // "DEL_COMMENT",
                // "DEL_0",
];

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    RichText {
        site: u8,
        pos: usize,
        len: usize,
        action: RichTextAction,
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq)]
pub enum RichTextAction {
    Insert,
    Delete,
    Mark(usize),
}

impl Debug for RichTextAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RichTextAction::Insert => write!(f, "RichTextAction::Insert"),
            RichTextAction::Delete => write!(f, "RichTextAction::Delete"),
            RichTextAction::Mark(i) => write!(f, "RichTextAction::Mark({})", STYLES_NAME[*i]),
        }
    }
}

struct Actor {
    peer: PeerID,
    loro: LoroDoc,
    text_tracker: Arc<LoroDoc>,
    text_container: TextHandler,
    history: FxHashMap<Vec<ID>, LoroValue>,
}

impl Actor {
    fn new(id: PeerID) -> Self {
        let app = LoroDoc::new();
        app.set_peer_id(id).unwrap();
        let tracker = LoroDoc::new();
        tracker.set_peer_id(id).unwrap();
        let text = app.get_text("text");
        let actor = Actor {
            peer: id,
            loro: app,
            text_tracker: Arc::new(tracker),
            text_container: text,
            history: Default::default(),
        };

        let text_value = Arc::clone(&actor.text_tracker);

        actor.loro.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Arc::new(move |event| {
                let text_doc = &text_value;
                if let Diff::Text(text_diff) = &event.container.diff {
                    let mut txn = text_doc.txn().unwrap();
                    let text_h = text_doc.get_text("text");
                    println!("diff {:?}", text_diff);
                    if false {
                        let text_deltas = text_diff
                            .iter()
                            .map(|x| match x {
                                DeltaItem::Insert { insert, attributes } => TextDelta::Insert {
                                    insert: insert.to_string(),
                                    attributes: Some(
                                        attributes
                                            .iter()
                                            .map(|(k, v)| match k {
                                                StyleKey::Key(k) => (k.to_string(), v.data),
                                                StyleKey::KeyWithId { key, id } => {
                                                    let mut data = FxHashMap::default();
                                                    data.insert(
                                                        "key".to_string(),
                                                        LoroValue::String(Arc::new(
                                                            key.to_string(),
                                                        )),
                                                    );
                                                    data.insert("data".to_string(), v.data);
                                                    (
                                                        format!("id:{}", id),
                                                        LoroValue::Map(Arc::new(data)),
                                                    )
                                                }
                                            })
                                            .collect(),
                                    ),
                                },
                                DeltaItem::Delete {
                                    delete,
                                    attributes: _,
                                } => TextDelta::Delete { delete: *delete },
                                DeltaItem::Retain { retain, attributes } => TextDelta::Retain {
                                    retain: *retain,
                                    attributes: Some(
                                        attributes
                                            .iter()
                                            .map(|(k, v)| match k {
                                                StyleKey::Key(k) => (k.to_string(), v.data),
                                                StyleKey::KeyWithId { key, id } => {
                                                    let mut data = FxHashMap::default();
                                                    data.insert(
                                                        "key".to_string(),
                                                        LoroValue::String(Arc::new(
                                                            key.to_string(),
                                                        )),
                                                    );
                                                    data.insert("data".to_string(), v.data);
                                                    (
                                                        format!("id:{}", id),
                                                        LoroValue::Map(Arc::new(data)),
                                                    )
                                                }
                                            })
                                            .collect(),
                                    ),
                                },
                            })
                            .collect::<Vec<_>>();
                        println!(
                            "\n{} before {:?}",
                            text_doc.peer_id(),
                            text_h.get_richtext_value()
                        );
                        println!("delta {:?}", text_deltas);
                        text_h.apply_delta_with_txn(&mut txn, &text_deltas).unwrap();

                        println!("after {:?}\n", text_h.get_richtext_value());
                    } else {
                        // println!(
                        //     "\n{} before {:?}",
                        //     text_doc.peer_id(),
                        //     text_h.get_richtext_value()
                        // );
                        let mut index = 0;
                        for item in text_diff.iter() {
                            match item {
                                DeltaItem::Insert { insert, attributes } => {
                                    text_h
                                        .insert_with_txn(&mut txn, index, insert.as_str())
                                        .unwrap();
                                    // println!("at {} insert {}", index, insert.as_str());

                                    for (k, v) in attributes.iter() {
                                        let flag: usize = k.key().parse().unwrap();
                                        text_h
                                            .mark_with_txn(
                                                &mut txn,
                                                index,
                                                index + insert.len_unicode(),
                                                k.key(),
                                                v.data,
                                                TextStyleInfoFlag::new(
                                                    STYLES[flag].mergeable(),
                                                    ExpandType::None,
                                                    STYLES[flag].is_delete(),
                                                    STYLES[flag].is_container(),
                                                ),
                                            )
                                            .unwrap();
                                        // println!(
                                        //     "insert mark {}~{} {:?}",
                                        //     index,
                                        //     index + insert.len_unicode(),
                                        //     flag
                                        // );
                                    }
                                    index += insert.len_unicode();
                                }
                                DeltaItem::Delete {
                                    delete,
                                    attributes: _,
                                } => {
                                    text_h.delete_with_txn(&mut txn, index, *delete).unwrap();
                                    // println!("delete {}~{} ", index, index + *delete);
                                }
                                DeltaItem::Retain { retain, attributes } => {
                                    // println!("retain {}", retain);
                                    for (k, v) in attributes.iter() {
                                        let flag: usize = k.key().parse().unwrap();
                                        text_h
                                            .mark_with_txn(
                                                &mut txn,
                                                index,
                                                index + *retain,
                                                k.key(),
                                                v.data,
                                                TextStyleInfoFlag::new(
                                                    STYLES[flag].mergeable(),
                                                    ExpandType::None,
                                                    STYLES[flag].is_delete(),
                                                    STYLES[flag].is_container(),
                                                ),
                                            )
                                            .unwrap();
                                        // println!(
                                        //     "retain mark {}~{} {:?}",
                                        //     index,
                                        //     index + *retain,
                                        //     flag
                                        // );
                                    }
                                    index += *retain;
                                }
                            }
                        }
                        // println!("after {:?}\n", text_h.get_richtext_value());
                    }
                } else {
                    debug_dbg!(&event.container);
                    unreachable!()
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
            ],
            Action::SyncAll => vec!["sync all".into(), "".into(), "".into()],
            Action::RichText {
                site,
                pos,
                len,
                action,
            } => match action {
                RichTextAction::Insert => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        format!("insert {}", pos).into(),
                        format!("[{:?}]", len).into(),
                    ]
                }
                RichTextAction::Delete => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        "delete".to_string().into(),
                        format!("{}~{}", pos, pos + len).into(),
                        "".into(),
                    ]
                }
                RichTextAction::Mark(i) => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        format!("mark {:?}", STYLES_NAME[*i]).into(),
                        format!("{}~{}", pos, pos + len).into(),
                    ]
                }
            },
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec!["type".into(), "site".into(), "prop".into(), "value".into()]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&mut self, action: &mut Action);
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
            Action::RichText {
                site,
                pos,
                len,
                action,
            } => {
                *site %= max_users;
                let text = &self[*site as usize].text_container;
                let length = text.len_unicode();
                if matches!(action, RichTextAction::Delete | RichTextAction::Mark(_)) && length == 0
                {
                    *action = RichTextAction::Insert;
                }
                match action {
                    RichTextAction::Insert => {
                        *pos %= length + 1;
                    }
                    RichTextAction::Delete => {
                        *pos %= length;
                        *len %= length - *pos;
                        *len = 1.max(*len);
                    }
                    RichTextAction::Mark(i) => {
                        *pos %= length;
                        *len %= length - *pos;
                        *len = 1.max(*len);
                        *i %= STYLES.len();
                    }
                }
            }
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Sync { from, to } => {
                let (a, b) = array_mut_ref!(self, [*from as usize, *to as usize]);

                a.loro
                    .import(&b.loro.export_from(&a.loro.oplog_vv()))
                    .unwrap();
                b.loro
                    .import(&a.loro.export_from(&b.loro.oplog_vv()))
                    .unwrap();

                if a.peer == 1 {
                    a.record_history();
                }
            }
            Action::SyncAll => {
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.loro
                        .import(&b.loro.export_from(&a.loro.oplog_vv()))
                        .unwrap();
                }

                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.loro
                        .import(&a.loro.export_from(&b.loro.oplog_vv()))
                        .unwrap();
                }

                self[1].record_history();
            }
            Action::RichText {
                site,
                pos,
                len,
                action,
            } => match action {
                RichTextAction::Insert => {
                    let actor = &mut self[*site as usize];
                    let mut txn = actor.loro.txn().unwrap();
                    let text = &mut self[*site as usize].text_container;
                    text.insert_with_txn(&mut txn, *pos, &format!("[{}]", len))
                        .unwrap();
                }
                RichTextAction::Delete => {
                    let actor = &mut self[*site as usize];
                    let mut txn = actor.loro.txn().unwrap();
                    let text = &mut self[*site as usize].text_container;
                    text.delete_with_txn(&mut txn, *pos, *len).unwrap();
                }
                RichTextAction::Mark(i) => {
                    let actor = &mut self[*site as usize];
                    let mut txn = actor.loro.txn().unwrap();
                    let text = &mut self[*site as usize].text_container;
                    let style = STYLES[*i];
                    text.mark_with_txn(
                        &mut txn,
                        *pos,
                        *pos + *len,
                        &i.to_string(),
                        if style.is_delete() {
                            LoroValue::Null
                        } else {
                            true.into()
                        },
                        style,
                    )
                    .unwrap();
                }
            },
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
                if k.starts_with("id") {
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
                if k.starts_with("id") {
                    continue;
                }
                assert_value_eq(v, a.get(k).unwrap());
            }
        }
        (LoroValue::List(a), LoroValue::List(b)) => {
            for (av, bv) in a.iter().zip(b.iter()) {
                assert_value_eq(av, bv);
            }
        }
        (a, b) => assert_eq!(a, b),
    }
}

fn check_eq(a_actor: &mut Actor, b_actor: &mut Actor) {
    // let a_doc = &mut a_actor.loro;
    // let b_doc = &mut b_actor.loro;
    let a_result = a_actor.text_container.get_richtext_value();
    let b_result = b_actor.text_container.get_richtext_value();
    let a_value = a_actor.text_tracker.get_text("text").get_richtext_value();

    debug_log::debug_log!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_result);
    // println!("actor {}", a_actor.peer);
    // TODO: test value
    // assert_value_eq(&a_result, &a_value);
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
        // println!("\nfrom {:?} checkout {:?}", actor.loro.oplog_vv(), f);
        // println!("before state {:?}", actor.loro.get_deep_value());
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
    use super::test_multi_sites;
    use super::Action::*;
    use super::RichTextAction;
    #[test]
    fn fuzz1() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 255,
                    pos: 72057594037927935,
                    len: 18446744073709508608,
                    action: RichTextAction::Mark(18446744073698541568),
                },
                RichText {
                    site: 55,
                    pos: 3978709506094226231,
                    len: 3978709268954218551,
                    action: RichTextAction::Mark(15335939993951284180),
                },
                RichText {
                    site: 0,
                    pos: 72057594021150720,
                    len: 3978709660713025611,
                    action: RichTextAction::Insert,
                },
            ],
        )
    }
}
