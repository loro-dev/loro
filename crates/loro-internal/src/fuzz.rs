pub mod recursive_refactored;
pub mod tree;

use crate::{
    array_mut_ref,
    container::richtext::TextStyleInfoFlag,
    delta::{Delta, DeltaItem, StyleMeta},
    event::Diff,
    loro::LoroDoc,
    state::ContainerState,
    utils::string_slice::StringSlice,
};
use debug_log::debug_log;
use enum_as_inner::EnumAsInner;
use loro_common::{ContainerID, LoroValue};
use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
    time::Instant,
};
use tabled::{TableIteratorExt, Tabled};

const STYLES: [TextStyleInfoFlag; 8] = [
    TextStyleInfoFlag::BOLD,
    TextStyleInfoFlag::COMMENT,
    TextStyleInfoFlag::LINK,
    TextStyleInfoFlag::LINK.to_delete(),
    TextStyleInfoFlag::BOLD.to_delete(),
    TextStyleInfoFlag::COMMENT.to_delete(),
    TextStyleInfoFlag::from_byte(0),
    TextStyleInfoFlag::from_byte(0).to_delete(),
];

#[derive(arbitrary::Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins {
        content: u16,
        pos: usize,
        site: u8,
    },
    Del {
        pos: usize,
        len: usize,
        site: u8,
    },
    Mark {
        pos: usize,
        len: usize,
        site: u8,
        style_key: u8,
    },
    Sync {
        from: u8,
        to: u8,
    },
    SyncAll,
}

impl Tabled for Action {
    const LENGTH: usize = 5;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Ins { content, pos, site } => vec![
                "ins".into(),
                site.to_string().into(),
                pos.to_string().into(),
                content.to_string().len().to_string().into(),
                content.to_string().into(),
            ],
            Action::Del { pos, len, site } => vec![
                "del".into(),
                site.to_string().into(),
                pos.to_string().into(),
                len.to_string().into(),
                "".into(),
            ],
            Action::Sync { from, to } => vec![
                "sync".into(),
                format!("{} to {}", from, to).into(),
                "".into(),
                "".into(),
                "".into(),
            ],
            Action::SyncAll => vec![
                "sync all".into(),
                "".into(),
                "".into(),
                "".into(),
                "".into(),
            ],
            Action::Mark {
                pos,
                len,
                site,
                style_key,
            } => vec![
                "mark".into(),
                site.to_string().into(),
                pos.to_string().into(),
                len.to_string().into(),
                format!("style {}", style_key).into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec![
            "type".into(),
            "site".into(),
            "pos".into(),
            "len".into(),
            "content".into(),
        ]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&mut self, action: &mut Action);
}

impl Action {
    pub fn preprocess(&mut self, max_len: usize, max_users: u8) {
        match self {
            Action::Ins { pos, site, .. } => {
                *pos %= max_len + 1;
                *site %= max_users;
            }
            Action::Del { pos, len, site } => {
                if max_len == 0 {
                    *pos = 0;
                    *len = 0;
                } else {
                    *pos %= max_len;
                    *len = (*len).min(max_len - (*pos));
                }
                *site %= max_users;
            }
            Action::Sync { from, to } => {
                *from %= max_users;
                *to %= max_users;
            }
            Action::SyncAll => {}
            Action::Mark {
                pos,
                len,
                site,
                style_key,
            } => {
                if max_len == 0 {
                    *pos = 0;
                    *len = 0;
                } else {
                    *pos %= max_len;
                    *len = (*len).min(max_len - (*pos));
                }

                *site %= max_users;
                *style_key %= 8;
            }
        }
    }
}

impl Actionable for String {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, .. } => {
                self.insert_str(*pos, &content.to_string());
            }
            &Action::Del { pos, len, .. } => {
                if self.is_empty() {
                    return;
                }

                self.drain(pos..pos + len);
            }
            _ => {}
        }
    }

    fn preprocess(&mut self, action: &mut Action) {
        action.preprocess(self.len(), 1);
        match action {
            Action::Ins { pos, .. } => {
                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (self.len() + 1)
                }
            }
            Action::Del { pos, len, .. } => {
                if self.is_empty() {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % self.len();
                }

                *len = (*len).min(self.len() - (*pos));
                while !self.is_char_boundary(*pos + *len) {
                    *len += 1;
                }
            }
            _ => {}
        }
    }
}

impl Actionable for Vec<LoroDoc> {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, site } => {
                let site = &mut self[*site as usize];
                let mut txn = site.txn().unwrap();
                let text = txn.get_text("text");
                text.insert_with_txn(&mut txn, *pos, &format!("[{}]", content))
                    .unwrap();
            }
            Action::Del { pos, len, site } => {
                let site = &mut self[*site as usize];
                let mut txn = site.txn().unwrap();
                let text = txn.get_text("text");
                text.delete_with_txn(&mut txn, *pos, *len).unwrap();
            }
            Action::Sync { from, to } => {
                if from != to {
                    let (from, to) = arref::array_mut_ref!(self, [*from as usize, *to as usize]);
                    let to_vv = to.oplog_vv();
                    to.import(&from.export_from(&to_vv)).unwrap();
                }
            }
            Action::SyncAll => {
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.import(&b.export_from(&a.oplog_vv())).unwrap();
                }
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.import(&a.export_from(&b.oplog_vv())).unwrap();
                }
            }
            Action::Mark {
                pos,
                len,
                site,
                style_key,
            } => {
                if *len == 0 {
                    return;
                }

                let site = &mut self[*site as usize];
                let mut txn = site.txn().unwrap();
                let text = txn.get_text("text");
                let style = STYLES[*style_key as usize];
                text.mark_with_txn(
                    &mut txn,
                    *pos,
                    *pos + *len,
                    &style_key.to_string(),
                    if style.is_delete() {
                        LoroValue::Null
                    } else {
                        true.into()
                    },
                    style,
                )
                .unwrap();
            }
        }
    }

    fn preprocess(&mut self, action: &mut Action) {
        match action {
            Action::Ins { pos, site, .. } => {
                *site %= self.len() as u8;
                *pos %= self[*site as usize]
                    .app_state()
                    .lock()
                    .unwrap()
                    .get_text("text")
                    .unwrap()
                    .len_unicode()
                    + 1;
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let app_state = &mut self[*site as usize].app_state().lock().unwrap();
                let text = app_state.get_text("text").unwrap();
                if text.is_empty() {
                    *len = 0;
                    *pos = 0;
                } else {
                    *pos %= text.len_unicode();
                    *len = (*len).min(text.len_unicode() - (*pos));
                }
            }
            Action::Sync { from, to } => {
                *from %= self.len() as u8;
                *to %= self.len() as u8;
            }
            Action::SyncAll => {}
            Action::Mark {
                pos,
                len,
                site,
                style_key,
            } => {
                *site %= self.len() as u8;
                let app_state = &mut self[*site as usize].app_state().lock().unwrap();
                let text = app_state.get_text("text").unwrap();
                if text.is_empty() {
                    *len = 0;
                    *pos = 0;
                } else {
                    *pos %= text.len_unicode();
                    *len = (*len).min(text.len_unicode() - (*pos));
                }

                *style_key %= 8;
            }
        }
    }
}

pub fn change_delete_to_char_boundary(pos: &mut usize, len: &mut usize, str_len: usize) {
    *pos %= str_len + 1;
    *len = (*len).min(str_len - (*pos));
}

pub fn change_pos_to_char_boundary(pos: &mut usize, len: usize) {
    *pos %= len + 1;
}

fn check_synced(sites: &mut [LoroDoc], _: &[Arc<Mutex<Delta<StringSlice, StyleMeta>>>]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log::group!("checking {} with {}", i, j);
            let (a, b) = array_mut_ref!(sites, [i, j]);
            {
                if (i + j) % 2 == 1 {
                    debug_log::group!("Import {}'s Snapshot to {}", j, i);
                    a.import(&b.export_snapshot()).unwrap();
                    debug_log::group_end!();
                } else {
                    debug_log::group!("Import {} to {}", j, i);
                    a.import(&b.export_from(&a.oplog_vv())).unwrap();
                    debug_log::group_end!();
                }
            }
            {
                debug_log::group!("Import {} to {}", i, j);
                b.import(&a.export_from(&b.oplog_vv())).unwrap();
                debug_log::group_end!();
            }
            check_eq(a, b);
            debug_log::group_end!();

            // for (x, (site, text)) in sites.iter().zip(texts.iter()).enumerate() {
            //     if x != i && x != j {
            //         continue;
            //     }

            //     debug_log::group!("Check {}", x);
            //     let diff = site.get_text("text").with_state_mut(|s| s.to_diff());
            //     let mut diff = diff.into_text().unwrap();
            //     compact(&mut diff);
            //     let mut text = text.lock().unwrap();
            //     compact(&mut text);
            //     assert_eq!(
            //         &diff, &*text,
            //         "site:{}\nEXPECTED {:#?}\nACTUAL {:#?}",
            //         x, diff, text
            //     );
            //     debug_log::group_end!();
            // }
        }
    }
}

fn check_eq(site_a: &mut LoroDoc, site_b: &mut LoroDoc) {
    let a = site_a.txn().unwrap();
    let text_a = a.get_text("text");
    let b = site_b.txn().unwrap();
    let text_b = b.get_text("text");
    let value_a = text_a.get_richtext_value();
    let value_b = text_b.get_richtext_value();
    if value_a != value_b {
        {
            // compare plain text value
            let value_a = text_a.get_value();
            let value_b = text_b.get_value();
            assert_eq!(
                value_a,
                value_b,
                "Plain Text not equal. peer{}={:?}, peer{}={:?}",
                site_a.peer_id(),
                value_a,
                site_b.peer_id(),
                value_b
            );
        }

        text_a.with_state(|s| {
            dbg!(&s.state);
        });
        text_b.with_state(|s| {
            dbg!(&s.state);
        });
        assert_eq!(
            value_a,
            value_b,
            "Richtext Style not equal. peer{}={:?}, peer{}={:?}, \n",
            site_a.peer_id(),
            value_a,
            site_b.peer_id(),
            value_b
        );
    }
}

pub fn minify_error<T, F, N>(site_num: u8, actions: Vec<T>, f: F, normalize: N)
where
    F: Fn(u8, &mut [T]),
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
        // println!("{:?}", _info);
    }));

    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
    #[allow(clippy::redundant_clone)]
    let actions_clone = actions.clone();
    let action_ref: usize = (&actions_clone) as *const _ as usize;
    #[allow(clippy::blocks_in_if_conditions)]
    if std::panic::catch_unwind(|| {
        // SAFETY: test
        let f = unsafe { &*(f_ref as *const F) };
        // SAFETY: test
        let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
        f(site_num, actions_ref);
    })
    .is_ok()
    {
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
        let f_ref: *const _ = &f;
        let f_ref: usize = f_ref as usize;
        let actions_clone = candidate.clone();
        let action_ref: usize = (&actions_clone) as *const _ as usize;
        #[allow(clippy::blocks_in_if_conditions)]
        if std::panic::catch_unwind(|| {
            // SAFETY: test
            let f = unsafe { &*(f_ref as *const F) };
            // SAFETY: test
            let actions_ref = unsafe { &mut *(action_ref as *mut Vec<T>) };
            f(site_num, actions_ref);
        })
        .is_err()
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
        if start.elapsed().as_secs() > 10 && minified.len() <= 4 {
            break;
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
        minify_error(site_num, minified, f, normalize);
    }
}

pub fn test_multi_sites(site_num: u8, actions: &mut [Action]) {
    let mut sites = Vec::new();
    let mut texts = Vec::new();
    for i in 0..site_num {
        let loro = LoroDoc::new();
        let text: Arc<Mutex<Delta<StringSlice, StyleMeta>>> = Arc::new(Mutex::new(Delta::new()));
        let text_clone = text.clone();
        loro.set_peer_id(i as u64).unwrap();
        loro.subscribe(
            &ContainerID::new_root("text", loro_common::ContainerType::Text),
            Arc::new(move |event| {
                if let Diff::Text(t) = &event.container.diff {
                    let mut text = text_clone.lock().unwrap();
                    debug_log::debug_log!(
                        "RECEIVE site:{} event:{:#?}\nCURRENT: {:#?}",
                        i,
                        t,
                        &text
                    );
                    *text = text.clone().compose(t.clone());
                    debug_log::debug_log!("new:{:#?}", &text);
                }
            }),
        );
        texts.push(text);
        sites.push(loro);
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        debug_log!("\n{}", (&applied).table());
        debug_log::group!("ApplyAction {:?}", &action);
        sites.apply_action(action);
        debug_log::group_end!();

        // for (i, (site, text)) in sites.iter().zip(texts.iter()).enumerate() {
        //     debug_log::group!("Check {}", i);
        //     let diff = site.get_text("text").with_state_mut(|s| s.to_diff());
        //     let mut diff = diff.into_text().unwrap();
        //     compact(&mut diff);
        //     let mut text = text.lock().unwrap();
        //     compact(&mut text);
        //     assert_eq!(
        //         &diff, &*text,
        //         "site:{}\nEXPECTED{:#?}\nACTUAL{:#?}",
        //         i, diff, text
        //     );
        //     debug_log::group_end!();
        // }
    }

    debug_log::group!("CheckSynced");
    // println!("{}", actions.table());
    check_synced(&mut sites, &texts);
    debug_log::group_end!();
    debug_log::group!("CheckTextEvent");
    for (i, (site, text)) in sites.iter().zip(texts.iter()).enumerate() {
        debug_log::group!("Check {}", i);
        let diff = site.get_text("text").with_state_mut(|s| s.to_diff());
        let mut diff = diff.into_text().unwrap();
        compact(&mut diff);
        let mut text = text.lock().unwrap();
        compact(&mut text);
        assert_eq!(
            &diff, &*text,
            "site:{}\nEXPECTED{:#?}\nACTUAL{:#?}",
            i, diff, text
        );
        debug_log::group_end!();
    }

    debug_log::group_end!();
}

pub fn compact(delta: &mut Delta<StringSlice, StyleMeta>) {
    let mut ops: Vec<DeltaItem<StringSlice, StyleMeta>> = vec![];
    for op in delta.vec.drain(..) {
        match (ops.last_mut(), op) {
            (
                Some(DeltaItem::Retain {
                    retain: last_retain,
                    attributes: last_attr,
                }),
                DeltaItem::Retain { retain, attributes },
            ) if &attributes == last_attr => {
                *last_retain += retain;
            }
            (
                Some(DeltaItem::Insert {
                    insert: last_insert,
                    attributes: last_attr,
                }),
                DeltaItem::Insert { insert, attributes },
            ) if last_attr == &attributes => {
                last_insert.extend(insert.as_str());
            }
            (
                Some(DeltaItem::Delete {
                    delete: last_delete,
                    attributes: _,
                }),
                DeltaItem::Delete {
                    delete,
                    attributes: _,
                },
            ) => {
                *last_delete += delete;
            }
            (_, a) => {
                ops.push(a);
            }
        }
    }

    delta.vec = ops;
}

#[cfg(test)]
mod test {
    use super::Action::*;
    use super::*;

    #[test]
    fn fuzz_r1() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 3871,
                    pos: 20971570,
                    site: 0,
                },
                Sync { from: 0, to: 31 },
                Ins {
                    content: 0,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 58502001197056,
                    site: 0,
                },
                Ins {
                    content: 13599,
                    pos: 36261893487333151,
                    site: 31,
                },
            ],
        )
    }

    #[test]
    fn fuzz_r() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 5225,
                    pos: 0,
                    site: 4,
                },
                Ins {
                    content: 53,
                    pos: 4,
                    site: 4,
                },
                Ins {
                    content: 10284,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 8234,
                    pos: 0,
                    site: 6,
                },
                Ins {
                    content: 7710,
                    pos: 1,
                    site: 6,
                },
                Ins {
                    content: 0,
                    pos: 7,
                    site: 2,
                },
                Ins {
                    content: 127,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 2560,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 10794,
                    pos: 4,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 1,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 29,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 4,
                    site: 6,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 4626,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 4626,
                    pos: 2,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 54826,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 12800,
                    pos: 9,
                    site: 6,
                },
                Ins {
                    content: 3598,
                    pos: 0,
                    site: 4,
                },
                Ins {
                    content: 11308,
                    pos: 2,
                    site: 4,
                },
                Ins {
                    content: 10284,
                    pos: 3,
                    site: 4,
                },
                Ins {
                    content: 11308,
                    pos: 10,
                    site: 4,
                },
                Ins {
                    content: 11308,
                    pos: 24,
                    site: 4,
                },
                Ins {
                    content: 11308,
                    pos: 28,
                    site: 4,
                },
                Ins {
                    content: 11312,
                    pos: 16,
                    site: 4,
                },
                Ins {
                    content: 11308,
                    pos: 5,
                    site: 4,
                },
                Ins {
                    content: 15420,
                    pos: 9,
                    site: 2,
                },
                Ins {
                    content: 12800,
                    pos: 0,
                    site: 5,
                },
                Ins {
                    content: 10794,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 21,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 34,
                    site: 2,
                },
                Ins {
                    content: 12850,
                    pos: 10,
                    site: 2,
                },
                Ins {
                    content: 12850,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 21,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 6,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 56,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 2,
                    site: 6,
                },
                Ins {
                    content: 7710,
                    pos: 2,
                    site: 6,
                },
                Ins {
                    content: 10794,
                    pos: 27,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 70,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 69,
                    site: 2,
                },
                SyncAll,
                Ins {
                    content: 0,
                    pos: 184,
                    site: 0,
                },
                Del {
                    pos: 18,
                    len: 191,
                    site: 0,
                },
                Del {
                    pos: 4,
                    len: 204,
                    site: 4,
                },
                Del {
                    pos: 90,
                    len: 118,
                    site: 5,
                },
            ],
        );
    }

    #[test]
    fn new_encode() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 3871,
                    pos: 2755657778,
                    site: 0,
                },
                Sync { from: 0, to: 31 },
                Ins {
                    content: 3840,
                    pos: 55040529478965,
                    site: 212,
                },
                Ins {
                    content: 0,
                    pos: 17381979229574397952,
                    site: 15,
                },
                Ins {
                    content: 12815,
                    pos: 2248762090699358208,
                    site: 15,
                },
                Sync { from: 0, to: 212 },
                Ins {
                    content: 25896,
                    pos: 14090375187464448,
                    site: 64,
                },
                Ins {
                    content: 0,
                    pos: 6067790159959556096,
                    site: 212,
                },
            ],
        )
    }

    #[test]
    fn snapshot() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 32818,
                    pos: 0,
                    site: 1,
                },
                Ins {
                    content: 12850,
                    pos: 0,
                    site: 3,
                },
                Ins {
                    content: 13621,
                    pos: 3,
                    site: 1,
                },
            ],
        )
    }

    #[test]
    fn snapshot_2() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 12850,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 1,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 10,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 4,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 28,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 29,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 36,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 42,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 24,
                    site: 2,
                },
                Ins {
                    content: 10833,
                    pos: 36,
                    site: 2,
                },
                Ins {
                    content: 1,
                    pos: 5,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 42,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 66,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 70,
                    site: 2,
                },
                Ins {
                    content: 24106,
                    pos: 0,
                    site: 6,
                },
                Ins {
                    content: 64001,
                    pos: 47,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 5,
                },
                Ins {
                    content: 10794,
                    pos: 62,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 13,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 10794,
                    pos: 88,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 42,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 24,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 32,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 18,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 21,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 0,
                    site: 6,
                },
                Ins {
                    content: 10794,
                    pos: 115,
                    site: 2,
                },
                Ins {
                    content: 42,
                    pos: 14,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 50,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 76,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 132,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 5,
                    site: 6,
                },
                Del {
                    pos: 106,
                    len: 57,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 58,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 106,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 9,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 24,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 21,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 98,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 21,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 26,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 63,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 122,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 28,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 138,
                    site: 2,
                },
                Ins {
                    content: 10833,
                    pos: 19,
                    site: 2,
                },
                Ins {
                    content: 1,
                    pos: 36,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 129,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 96,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 24,
                    site: 2,
                },
                Ins {
                    content: 10794,
                    pos: 14,
                    site: 6,
                },
                Del {
                    pos: 10,
                    len: 177,
                    site: 2,
                },
                Ins {
                    content: 5,
                    pos: 9,
                    site: 2,
                },
                SyncAll,
                Ins {
                    content: 10794,
                    pos: 32,
                    site: 0,
                },
                Ins {
                    content: 10794,
                    pos: 30,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn checkout() {
        test_multi_sites(
            4,
            &mut [
                Ins {
                    content: 53,
                    pos: 4,
                    site: 2,
                },
                SyncAll,
                Ins {
                    content: 0,
                    pos: 1,
                    site: 0,
                },
                Del {
                    pos: 4,
                    len: 1,
                    site: 2,
                },
            ],
        )
    }

    #[test]
    fn text_fuzz_2() {
        test_multi_sites(
            8,
            &mut [
                Ins {
                    content: 111,
                    pos: 0,
                    site: 1,
                },
                Ins {
                    content: 222,
                    pos: 0,
                    site: 0,
                },
                Del {
                    pos: 3,
                    len: 2,
                    site: 0,
                },
                Ins {
                    content: 332,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 163,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 163,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 163,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 163,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 113,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 888,
                    pos: 4268070197446523707,
                    site: 3,
                },
                Ins {
                    content: 999,
                    pos: 3,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn text_fuzz_3() {
        test_multi_sites(
            2,
            &mut [
                Ins {
                    content: 12850,
                    pos: 3617008641906848306,
                    site: 10,
                },
                SyncAll,
                Ins {
                    content: 12850,
                    pos: 4048798961467195395,
                    site: 255,
                },
                SyncAll,
                Ins {
                    content: 12850,
                    pos: 3280475,
                    site: 0,
                },
                Ins {
                    content: 12851,
                    pos: 89064736817458,
                    site: 0,
                },
                Ins {
                    content: 13077,
                    pos: 4557431322972336690,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 16191,
                    pos: 4557430888798830399,
                    site: 63,
                },
                Ins {
                    content: 42148,
                    pos: 171061810,
                    site: 12,
                },
                Ins {
                    content: 3675,
                    pos: 336666162111538,
                    site: 0,
                },
                Sync { from: 164, to: 164 },
                Ins {
                    content: 4112,
                    pos: 1157442765409226768,
                    site: 16,
                },
                Ins {
                    content: 4112,
                    pos: 3732657327335018512,
                    site: 45,
                },
                Ins {
                    content: 52530,
                    pos: 3906253595950239181,
                    site: 15,
                },
                Ins {
                    content: 5911,
                    pos: 18446743885640439575,
                    site: 255,
                },
                Ins {
                    content: 11822,
                    pos: 3327609269301292590,
                    site: 46,
                },
                SyncAll,
                Ins {
                    content: 1568,
                    pos: 13836783189022944192,
                    site: 43,
                },
                SyncAll,
                Del {
                    pos: 16789419410837209344,
                    len: 14774117067329253930,
                    site: 0,
                },
                Ins {
                    content: 27242,
                    pos: 7668058320836127338,
                    site: 106,
                },
                Del {
                    pos: 7639230867934177898,
                    len: 7668058320836127338,
                    site: 106,
                },
            ],
        )
    }

    #[test]
    fn text_fuzz_4() {
        test_multi_sites(
            2,
            &mut [
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3607136139932740138,
                    site: 0,
                },
                Ins {
                    content: 13071,
                    pos: 14976676367971893,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 0,
                    pos: 0,
                    site: 68,
                },
                Ins {
                    content: 52532,
                    pos: 6629299531957718322,
                    site: 92,
                },
                Del {
                    pos: 6655295901103053916,
                    len: 6655295901103053916,
                    site: 92,
                },
                Del {
                    pos: 6655295901103053916,
                    len: 14738250545257564,
                    site: 0,
                },
                Ins {
                    content: 52309,
                    pos: 3038287258827541861,
                    site: 255,
                },
                Ins {
                    content: 15159,
                    pos: 298,
                    site: 108,
                },
                Ins {
                    content: 10782,
                    pos: 18446602417805601322,
                    site: 255,
                },
                Ins {
                    content: 771,
                    pos: 3027266685993419523,
                    site: 42,
                },
                Ins {
                    content: 0,
                    pos: 2560,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 7740398491872002048,
                    site: 107,
                },
                Del {
                    pos: 7740398493674204011,
                    len: 7740398493674204011,
                    site: 107,
                },
                Del {
                    pos: 7740398493674204011,
                    len: 7740398493674204011,
                    site: 107,
                },
                Del {
                    pos: 7812738666512280684,
                    len: 10634005385065557100,
                    site: 147,
                },
                Ins {
                    content: 108,
                    pos: 3830030471958364160,
                    site: 224,
                },
                SyncAll,
                Ins {
                    content: 52730,
                    pos: 3442658613,
                    site: 11,
                },
                Ins {
                    content: 3871,
                    pos: 8444828877984183605,
                    site: 117,
                },
                Del {
                    pos: 8463800222054970741,
                    len: 3635941421513799029,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 12884901966,
                    site: 0,
                },
                Ins {
                    content: 11308,
                    pos: 143833902818798636,
                    site: 0,
                },
                Ins {
                    content: 65535,
                    pos: 1567663063039,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 4121356608997425152,
                    site: 37,
                },
                Ins {
                    content: 24576,
                    pos: 3182967475521064462,
                    site: 44,
                },
                Ins {
                    content: 47148,
                    pos: 1021239783737535532,
                    site: 44,
                },
                Ins {
                    content: 11283,
                    pos: 1021239783737535532,
                    site: 14,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 15420,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 15420,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 15420,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 15420,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 15420,
                    pos: 4340410370284600380,
                    site: 60,
                },
                Ins {
                    content: 11324,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 3617346860830436396,
                    site: 170,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 57927929864792618,
                    site: 0,
                },
                Ins {
                    content: 64012,
                    pos: 900719925474099199,
                    site: 12,
                },
                Ins {
                    content: 3084,
                    pos: 789516,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 86131342873526272,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 3038287259199220224,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 10794,
                    pos: 3038287259199220266,
                    site: 42,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875111468,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 3182967604875373612,
                    site: 44,
                },
                Ins {
                    content: 11308,
                    pos: 48568231275564,
                    site: 0,
                },
                Ins {
                    content: 13568,
                    pos: 8463800223526997301,
                    site: 117,
                },
                Del {
                    pos: 216718996853,
                    len: 335007449137,
                    site: 3,
                },
                Ins {
                    content: 0,
                    pos: 3819052576016433152,
                    site: 53,
                },
                Ins {
                    content: 0,
                    pos: 4548370641345118208,
                    site: 212,
                },
                Ins {
                    content: 15104,
                    pos: 144115188075855872,
                    site: 20,
                },
                Ins {
                    content: 63999,
                    pos: 327908794171647,
                    site: 0,
                },
                Ins {
                    content: 65388,
                    pos: 3038287258997104158,
                    site: 42,
                },
                SyncAll,
                Ins {
                    content: 771,
                    pos: 217020518514230019,
                    site: 42,
                },
                Ins {
                    content: 0,
                    pos: 655360,
                    site: 0,
                },
                Del {
                    pos: 7710162562058289173,
                    len: 7740398493674204011,
                    site: 107,
                },
                Del {
                    pos: 7740398493674204011,
                    len: 7039851,
                    site: 49,
                },
                Ins {
                    content: 54335,
                    pos: 64872246332757,
                    site: 0,
                },
                Ins {
                    content: 5122,
                    pos: 72050993380600362,
                    site: 50,
                },
                Ins {
                    content: 0,
                    pos: 2170452909760708608,
                    site: 30,
                },
                Ins {
                    content: 10794,
                    pos: 217021605090393898,
                    site: 3,
                },
                Ins {
                    content: 763,
                    pos: 2606223169945797379,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 7799953079628988416,
                    site: 21,
                },
                Ins {
                    content: 0,
                    pos: 7740398493674204011,
                    site: 107,
                },
                Del {
                    pos: 7740398493674204011,
                    len: 7740398493674204011,
                    site: 107,
                },
                Del {
                    pos: 7740398493674204011,
                    len: 7782338265204091755,
                    site: 108,
                },
                Del {
                    pos: 7812738666512280684,
                    len: 10664523451937821582,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 18302637545179512940,
                    site: 255,
                },
                Ins {
                    content: 52730,
                    pos: 3442658613,
                    site: 11,
                },
                Ins {
                    content: 3871,
                    pos: 8444828877984183605,
                    site: 117,
                },
                Del {
                    pos: 8463800222054970741,
                    len: 10049067290889385333,
                    site: 138,
                },
                Sync { from: 138, to: 129 },
            ],
        )
    }

    #[test]
    fn richtext_fuzz_0() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 9728,
                    pos: 3829748534148603701,
                    site: 31,
                },
                SyncAll,
                Mark {
                    pos: 144373576751199690,
                    len: 39583260855602,
                    site: 38,
                    style_key: 227,
                },
                Del {
                    pos: 18446521092732302645,
                    len: 15028556109460991,
                    site: 53,
                },
            ],
        )
    }

    #[test]
    fn richtext_fuzz_1() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 41009,
                    pos: 10884953820616207167,
                    site: 151,
                },
                Mark {
                    pos: 150995095,
                    len: 7502773972505002496,
                    site: 0,
                    style_key: 0,
                },
                Mark {
                    pos: 11821702543106517760,
                    len: 4251403153421165732,
                    site: 151,
                    style_key: 151,
                },
                Mark {
                    pos: 589824,
                    len: 2233786514697303298,
                    site: 51,
                    style_key: 151,
                },
            ],
        )
    }

    #[test]
    fn richtext_fuzz_2() {
        test_multi_sites(
            5,
            &mut [
                Del {
                    pos: 3617008641902972703,
                    len: 3617008641903833650,
                    site: 50,
                },
                Ins {
                    content: 12850,
                    pos: 7668058320836112946,
                    site: 106,
                },
                Mark {
                    pos: 7667941315552635498,
                    len: 7668058320836127338,
                    site: 106,
                    style_key: 106,
                },
                Mark {
                    pos: 1096122290479458922,
                    len: 17749549354505267,
                    site: 1,
                    style_key: 0,
                },
                Mark {
                    pos: 7668058389825092218,
                    len: 7668058320836127338,
                    site: 106,
                    style_key: 50,
                },
            ],
        )
    }

    #[test]
    fn richtext_fuzz_3() {
        test_multi_sites(
            5,
            &mut [Del {
                pos: 36310271995488768,
                len: 5859553690644468061,
                site: 81,
            }],
        );
    }

    #[test]
    fn fuzz_4() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 17733,
                    pos: 4991471925827290437,
                    site: 69,
                },
                Del {
                    pos: 4991471925827290437,
                    len: 4991471925827290437,
                    site: 69,
                },
                Del {
                    pos: 4991471925827290437,
                    len: 4991471925827290437,
                    site: 69,
                },
            ],
        )
    }

    #[test]
    fn fuzz_5() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 13877,
                    pos: 0,
                    site: 0,
                },
                Del {
                    pos: 3,
                    len: 4,
                    site: 0,
                },
                Del {
                    pos: 2,
                    len: 1,
                    site: 0,
                },
                Ins {
                    content: 12850,
                    pos: 0,
                    site: 1,
                },
                Ins {
                    content: 52487,
                    pos: 0,
                    site: 4,
                },
            ],
        );
    }

    #[test]
    fn fuzz_6() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 0,
                    pos: 16384000,
                    site: 0,
                },
                Mark {
                    pos: 4503599627370752,
                    len: 14829735428355981312,
                    site: 0,
                    style_key: 0,
                },
                Ins {
                    content: 10624,
                    pos: 1182309699815473152,
                    site: 0,
                },
                Ins {
                    content: 52685,
                    pos: 3474262130214096333,
                    site: 128,
                },
                Mark {
                    pos: 3607102274975328360,
                    len: 7812629349709198644,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
                Mark {
                    pos: 7812738666512280684,
                    len: 7812738666512280684,
                    site: 108,
                    style_key: 108,
                },
            ],
        );
    }

    #[test]
    fn fuzz_7() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 23507,
                    pos: 18446694595669524485,
                    site: 255,
                },
                SyncAll,
                Mark {
                    pos: 281474976710504,
                    len: 2018765,
                    site: 0,
                    style_key: 0,
                },
                Ins {
                    content: 29812,
                    pos: 13238251629368014964,
                    site: 183,
                },
                Del {
                    pos: 222575692683,
                    len: 8391339105262239744,
                    site: 120,
                },
            ],
        )
    }

    #[test]
    fn fuzz_8() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 0,
                    pos: 16384000,
                    site: 0,
                },
                Mark {
                    pos: 4503599627370752,
                    len: 14829735428355981312,
                    site: 0,
                    style_key: 0,
                },
                Ins {
                    content: 52685,
                    pos: 3474262130214096333,
                    site: 128,
                },
                Mark {
                    pos: 3607102274975328360,
                    len: 7812629349709198644,
                    site: 108,
                    style_key: 108,
                },
            ],
        )
    }

    #[test]
    fn fuzz_9() {
        test_multi_sites(
            5,
            &mut [
                Ins {
                    content: 3855,
                    pos: 0,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 59648,
                    pos: 3,
                    site: 0,
                },
                Ins {
                    content: 12815,
                    pos: 2,
                    site: 0,
                },
                Ins {
                    content: 65535,
                    pos: 12,
                    site: 0,
                },
                Del {
                    pos: 9,
                    len: 18,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn mini_r() {
        minify_error(5, vec![], test_multi_sites, |_, ans| ans.to_vec())
    }
}
