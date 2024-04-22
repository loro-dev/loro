use std::{fmt::Debug, sync::Arc};

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
    event::Diff, handler::TextDelta, loro::LoroDoc, value::ToJson, version::Frontiers, TextHandler,
};

const STYLES_NAME: [&str; 4] = ["bold", "comment", "link", "highlight"];

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    RichText {
        site: u8,
        pos: usize,
        value: usize,
        action: RichTextAction,
    },
    Checkout {
        site: u8,
        to: u32,
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
            RichTextAction::Mark(i) => write!(f, "RichTextAction::Mark({})", i),
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
        let mut default_history = FxHashMap::default();
        default_history.insert(Vec::new(), app.get_deep_value());
        let actor = Actor {
            peer: id,
            loro: app,
            text_tracker: Arc::new(tracker),
            text_container: text,
            history: default_history,
        };

        let text_value = Arc::clone(&actor.text_tracker);

        actor.loro.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Arc::new(move |event| {
                let text_doc = &text_value;
                for container_diff in event.events {
                    if let Diff::Text(text_diff) = &container_diff.diff {
                        let mut txn = text_doc.txn().unwrap();
                        let text_h = text_doc.get_text("text");
                        // println!("diff {:?}", text_diff);
                        let text_deltas = text_diff.iter().map(TextDelta::from).collect::<Vec<_>>();
                        // println!(
                        //     "\n{} before {:?}",
                        //     text_doc.peer_id(),
                        //     text_h.get_richtext_value()
                        // );
                        tracing::info!("delta {:?}", text_deltas);
                        text_h.apply_delta_with_txn(&mut txn, &text_deltas).unwrap();

                        // tracing::info!("after {:?}\n", text_h.get_richtext_value());
                    } else {
                        unreachable!()
                    }
                }
            }),
        );

        actor
    }

    fn record_history(&mut self) {
        self.loro.attach();
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
            ],
            Action::SyncAll => vec!["sync all".into(), "".into(), "".into()],
            Action::Checkout { site, to } => vec![
                "checkout".into(),
                format!("{}", site).into(),
                format!("to {}", to).into(),
                "".into(),
            ],
            Action::RichText {
                site,
                pos,
                value,
                action,
            } => match action {
                RichTextAction::Insert => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        format!("insert {}", pos).into(),
                        format!("[{:?}]", value).into(),
                    ]
                }
                RichTextAction::Delete => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        "delete".to_string().into(),
                        format!("{}~{}", pos, pos + value).into(),
                        "".into(),
                    ]
                }
                RichTextAction::Mark(i) => {
                    vec![
                        "richtext".into(),
                        format!("{}", site).into(),
                        format!("mark {:?}", STYLES_NAME[*i]).into(),
                        format!("{}~{}", pos, pos + value).into(),
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
            Action::Checkout { site, to } => {
                *site %= max_users;
                *to %= self[*site as usize].history.len() as u32;
            }
            Action::RichText {
                site,
                pos,
                value: len,
                action,
            } => {
                *site %= max_users;
                self[*site as usize].loro.attach();
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
                        *i %= STYLES_NAME.len();
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
            Action::Checkout { site, to } => {
                let actor = &mut self[*site as usize];
                let f = actor.history.keys().nth(*to as usize).unwrap();
                let f = Frontiers::from(f);
                tracing::info!("Checkout to {:?}", &f);
                actor.loro.checkout(&f).unwrap();
            }
            Action::RichText {
                site,
                pos,
                value: len,
                action,
            } => {
                let (mut txn, text) = {
                    let actor = &mut self[*site as usize];
                    let txn = actor.loro.txn().unwrap();
                    let text = &mut self[*site as usize].text_container;
                    (txn, text)
                };
                match action {
                    RichTextAction::Insert => {
                        text.insert_with_txn(&mut txn, *pos, &format!("[{}]", len))
                            .unwrap();
                    }
                    RichTextAction::Delete => {
                        text.delete_with_txn(&mut txn, *pos, *len).unwrap();
                    }
                    RichTextAction::Mark(i) => {
                        text.mark_with_txn(
                            &mut txn,
                            *pos,
                            *pos + *len,
                            STYLES_NAME[*i],
                            (*pos as i32).into(),
                            false,
                        )
                        .unwrap();
                    }
                }
                drop(txn);
                let actor = &mut self[*site as usize];
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
        (LoroValue::List(a), LoroValue::List(b)) => {
            for (av, bv) in a.iter().zip(b.iter()) {
                assert_value_eq(av, bv);
            }
        }
        (a, b) => assert_eq!(a, b),
    }
}

fn check_eq(a_actor: &mut Actor, b_actor: &mut Actor) {
    a_actor.loro.check_state_diff_calc_consistency_slow();
    b_actor.loro.check_state_diff_calc_consistency_slow();
    let a_result = a_actor.text_container.get_richtext_value();
    let b_result = b_actor.text_container.get_richtext_value();
    let a_value = a_actor.text_tracker.get_text("text").get_richtext_value();

    tracing::info!("{}", a_result.to_json_pretty());
    assert_eq!(&a_result, &b_result);
    tracing::info!("{}", a_value.to_json_pretty());
    assert_value_eq(&a_result, &a_value);
}

fn check_synced(sites: &mut [Actor]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            let s = tracing::span!(tracing::Level::INFO, "checking {} with {}", i, j);
            let _e = s.enter();
            let (a, b) = array_mut_ref!(sites, [i, j]);
            let a_doc = &mut a.loro;
            let b_doc = &mut b.loro;
            a_doc.attach();
            b_doc.attach();
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

#[tracing::instrument(skip_all)]
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
        let s = tracing::span!(tracing::Level::INFO, "ApplyingAction", action=?action);
        let _e = s.enter();
        sites.apply_action(action);
    }

    let s = tracing::span!(tracing::Level::INFO, "check synced");
    let _e = s.enter();
    check_synced(&mut sites);

    check_history(&mut sites[1]);
}

#[cfg(test)]
mod failed_tests {
    static mut GUARD: Option<FlushGuard> = None;
    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
        use tracing_chrome::ChromeLayerBuilder;
        use tracing_subscriber::{prelude::*, registry::Registry};
        if option_env!("DEBUG").is_some() {
            let (chrome_layer, _guard) = ChromeLayerBuilder::new()
                .include_args(true)
                .include_locations(true)
                .build();
            // SAFETY: Test
            unsafe { GUARD = Some(_guard) };
            tracing::subscriber::set_global_default(
                Registry::default()
                    .with(
                        tracing_subscriber::fmt::Layer::default()
                            .with_file(true)
                            .with_line_number(true),
                    )
                    .with(chrome_layer),
            )
            .unwrap();
        }
    }

    use tracing_chrome::FlushGuard;

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
                    value: 18446744073709508608,
                    action: RichTextAction::Mark(18446744073698541568),
                },
                RichText {
                    site: 55,
                    pos: 3978709506094226231,
                    value: 3978709268954218551,
                    action: RichTextAction::Mark(15335939993951284180),
                },
                RichText {
                    site: 0,
                    pos: 72057594021150720,
                    value: 3978709660713025611,
                    action: RichTextAction::Insert,
                },
            ],
        )
    }

    #[test]
    fn fuzz_2() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 0,
                    pos: 0,
                    value: 18437736874454765568,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 9,
                    value: 11156776183901913088,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 8,
                    value: 28,
                    action: RichTextAction::Mark(0),
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 24,
                    value: 3558932692,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 10,
                    value: 18374685380159995904,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 60,
                    value: 6,
                    action: RichTextAction::Mark(0),
                },
                RichText {
                    site: 4,
                    pos: 0,
                    value: 3158382343024284628,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 3,
                    pos: 4,
                    value: 21,
                    action: RichTextAction::Mark(0),
                },
                RichText {
                    site: 0,
                    pos: 3,
                    value: 12,
                    action: RichTextAction::Mark(0),
                },
                RichText {
                    site: 0,
                    pos: 78,
                    value: 120259084288,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 32,
                    value: 5,
                    action: RichTextAction::Mark(2),
                },
                RichText {
                    site: 3,
                    pos: 12,
                    value: 181419418583088,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 48,
                    value: 23,
                    action: RichTextAction::Mark(3),
                },
                RichText {
                    site: 0,
                    pos: 40,
                    value: 21,
                    action: RichTextAction::Mark(0),
                },
                RichText {
                    site: 3,
                    pos: 2,
                    value: 11140450636105252867,
                    action: RichTextAction::Insert,
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 116,
                    value: 212,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 66,
                    value: 2421481759735939069,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 13,
                    value: 3917287250882199552,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 176,
                    value: 6917529027792076800,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 4,
                    pos: 83,
                    value: 62,
                    action: RichTextAction::Delete,
                },
            ],
        )
    }

    #[test]
    fn checkout() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 212,
                    pos: 6542548,
                    value: 165,
                    action: RichTextAction::Delete,
                },
                RichText {
                    site: 106,
                    pos: 7668058320836127338,
                    value: 7668058320836127338,
                    action: RichTextAction::Delete,
                },
                Checkout {
                    site: 106,
                    to: 1785358954,
                },
            ],
        )
    }

    #[test]
    fn checkout_delete() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 0,
                    pos: 0,
                    value: 0,
                    action: RichTextAction::Mark(3098706341580712916),
                },
                RichText {
                    site: 0,
                    pos: 196608,
                    value: 16558833364434944,
                    action: RichTextAction::Delete,
                },
                SyncAll,
                Checkout { site: 3, to: 0 },
                RichText {
                    site: 0,
                    pos: 12080808861146021892,
                    value: 12080808863958804391,
                    action: RichTextAction::Delete,
                },
                SyncAll,
            ],
        )
    }

    #[test]
    fn checkout2() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 37,
                    pos: 144115188075855653,
                    value: 88888888888888,
                    action: RichTextAction::Insert,
                },
                SyncAll,
                RichText {
                    site: 37,
                    pos: 18385141895277128741,
                    value: 18422257949758979904,
                    action: RichTextAction::Mark(18446528569430507519),
                },
                RichText {
                    site: 37,
                    pos: 2676586395015587109,
                    value: 7777777777777777777,
                    action: RichTextAction::Insert,
                },
                Checkout {
                    site: 84,
                    to: 1414812756,
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 18446462598732840960,
                    value: 6666666666666666666,
                    action: RichTextAction::Insert,
                },
                Checkout {
                    site: 126,
                    to: 2567001172,
                },
                Checkout {
                    site: 84,
                    to: 1414812756,
                },
                SyncAll,
            ],
        )
    }

    #[test]
    fn checkout_3() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 0,
                    pos: 5908722711110090752,
                    value: 5931894172722287186,
                    action: RichTextAction::Insert,
                },
                Checkout {
                    site: 82,
                    to: 1381126738,
                },
                RichText {
                    site: 212,
                    pos: 15336116641665330132,
                    value: 15335883693215765716,
                    action: RichTextAction::Mark(2966521040065582292),
                },
                SyncAll,
                RichText {
                    site: 253,
                    pos: 15276210850130558976,
                    value: 4174656672104448,
                    action: RichTextAction::Mark(3170534137649176832),
                },
                RichText {
                    site: 0,
                    pos: 15283557786841306112,
                    value: 8366027960336307412,
                    action: RichTextAction::Delete,
                },
                RichText {
                    site: 26,
                    pos: 7696582235254,
                    value: 0,
                    action: RichTextAction::Insert,
                },
                Checkout {
                    site: 212,
                    to: 54387,
                },
                RichText {
                    site: 0,
                    pos: 15335884830126637056,
                    value: 14395694394764215508,
                    action: RichTextAction::Mark(1880844493352075264),
                },
                Checkout {
                    site: 43,
                    to: 23357908,
                },
                RichText {
                    site: 0,
                    pos: 8391361093162777600,
                    value: 1874573671255012468,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 26,
                    pos: 1933217919218752026,
                    value: 2314885532965937270,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 33,
                    pos: 4407665038823784148,
                    value: 15336116638101536777,
                    action: RichTextAction::Mark(8391360705119704276),
                },
                RichText {
                    site: 26,
                    pos: 14106654022170,
                    value: 7,
                    action: RichTextAction::Insert,
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 0,
                    value: 14339677631930434048,
                    action: RichTextAction::Mark(3556784640),
                },
                RichText {
                    site: 26,
                    pos: 100321451692029044,
                    value: 16888498602639360,
                    action: RichTextAction::Delete,
                },
                Checkout {
                    site: 116,
                    to: 436458194,
                },
                RichText {
                    site: 0,
                    pos: 8391360705107466752,
                    value: 11538257936951552884,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 32,
                    pos: 100663310,
                    value: 302447620784142,
                    action: RichTextAction::Insert,
                },
            ],
        )
    }

    #[test]
    fn checkout_4() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 5,
                    pos: 1010580480,
                    value: 0,
                    action: RichTextAction::Insert,
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 2377900603268071424,
                    value: 2387225703656530209,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 33,
                    pos: 6701392671700754720,
                    value: 2387189277486809121,
                    action: RichTextAction::Delete,
                },
                Sync { from: 137, to: 137 },
                RichText {
                    site: 33,
                    pos: 2377900746545832225,
                    value: 9928747371269792033,
                    action: RichTextAction::Delete,
                },
                Checkout {
                    site: 137,
                    to: 16777215,
                },
                RichText {
                    site: 0,
                    pos: 1993875390464,
                    value: 38712713492299776,
                    action: RichTextAction::Mark(12731870418897514672),
                },
                Sync { from: 176, to: 91 },
                Sync { from: 137, to: 137 },
            ],
        );
    }

    #[test]
    fn checkout_5() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 0,
                    pos: 9910603113857775223,
                    value: 8536291640920163781,
                    action: RichTextAction::Mark(516939283022490631),
                },
                Checkout {
                    site: 66,
                    to: 1111638594,
                },
                Checkout {
                    site: 95,
                    to: 65417,
                },
                SyncAll,
                RichText {
                    site: 0,
                    pos: 280965477201664,
                    value: 622770227593,
                    action: RichTextAction::Insert,
                },
                Sync { from: 137, to: 139 },
                RichText {
                    site: 255,
                    pos: 36659862010,
                    value: 14497138624149061632,
                    action: RichTextAction::Insert,
                },
                Checkout {
                    site: 31,
                    to: 773922591,
                },
                RichText {
                    site: 0,
                    pos: 8759942810400289,
                    value: 2377900603251727259,
                    action: RichTextAction::Insert,
                },
                Sync { from: 155, to: 1 },
            ],
        )
    }

    #[test]
    fn allow_overlap() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 255,
                    pos: 562949940576255,
                    value: 10,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 0,
                    pos: 52793738066393,
                    value: 15637060856183783423,
                    action: RichTextAction::Mark(15697817505862638041),
                },
            ],
        );
    }

    #[test]
    fn checkout_err() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 1,
                    pos: 72057594977517568,
                    value: 0,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 1,
                    pos: 279268526740791,
                    value: 18446744069419041023,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 1,
                    pos: 278391190192126,
                    value: 18446744070572146943,
                    action: RichTextAction::Mark(6196952189613637631),
                },
                RichText {
                    site: 251,
                    pos: 863599313408753663,
                    value: 458499228937131,
                    action: RichTextAction::Mark(72308159810888675),
                },
            ],
        )
    }

    #[test]
    fn checkout_err_2() {
        test_multi_sites(
            3,
            &mut [
                RichText {
                    site: 1,
                    pos: 0,
                    value: 14497138626449185274,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 1,
                    pos: 5,
                    value: 10,
                    action: RichTextAction::Mark(8536327904765227054),
                },
                RichText {
                    site: 1,
                    pos: 14,
                    value: 6,
                    action: RichTextAction::Mark(13562224825669899),
                },
                Checkout {
                    site: 1,
                    to: 522133279,
                },
            ],
        )
    }

    #[test]
    fn checkout_err_3() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 25,
                    pos: 18446490194317148160,
                    value: 18446744073709551615,
                    action: RichTextAction::Mark(18446744073709551615),
                },
                SyncAll,
                RichText {
                    site: 25,
                    pos: 48378530044185,
                    value: 9910452455013810176,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 4,
                    pos: 359156590005978116,
                    value: 72057576757069051,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 60,
                    pos: 289360692308608004,
                    value: 359156590005978116,
                    action: RichTextAction::Mark(289360751431254011),
                },
                RichText {
                    site: 4,
                    pos: 18446744073709551364,
                    value: 18446744073709551615,
                    action: RichTextAction::Mark(18446744069482020863),
                },
            ],
        )
    }

    #[test]
    fn iter_range_err() {
        test_multi_sites(
            5,
            &mut [
                RichText {
                    site: 1,
                    pos: 939589632,
                    value: 256,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 1,
                    pos: 279268526740791,
                    value: 18446744069419041023,
                    action: RichTextAction::Insert,
                },
                RichText {
                    site: 1,
                    pos: 278383768546103,
                    value: 18446744069419041023,
                    action: RichTextAction::Mark(6196952189613637631),
                },
                RichText {
                    site: 251,
                    pos: 863599313408753663,
                    value: 458499228937131,
                    action: RichTextAction::Mark(2378151169024582627),
                },
            ],
        );
    }
}
