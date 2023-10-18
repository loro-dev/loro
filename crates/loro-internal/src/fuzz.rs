pub mod recursive_refactored;

use crate::{array_mut_ref, loro::LoroDoc};
use debug_log::debug_log;
use enum_as_inner::EnumAsInner;
use std::{fmt::Debug, time::Instant};
use tabled::{TableIteratorExt, Tabled};

#[derive(arbitrary::Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins { content: u16, pos: usize, site: u8 },
    Del { pos: usize, len: usize, site: u8 },
    Sync { from: u8, to: u8 },
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
                text.insert(&mut txn, *pos, &format!("[{}]", content))
                    .unwrap();
            }
            Action::Del { pos, len, site } => {
                let site = &mut self[*site as usize];
                let mut txn = site.txn().unwrap();
                let text = txn.get_text("text");
                text.delete(&mut txn, *pos, *len).unwrap();
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

fn check_synced_refactored(sites: &mut [LoroDoc]) {
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
            check_eq_refactored(a, b);
            debug_log::group_end!();
        }
    }
}

fn check_eq_refactored(site_a: &mut LoroDoc, site_b: &mut LoroDoc) {
    let a = site_a.txn().unwrap();
    let text_a = a.get_text("text");
    let b = site_b.txn().unwrap();
    let text_b = b.get_text("text");
    let value_a = text_a.get_value();
    let value_b = text_b.get_value();
    assert_eq!(
        value_a,
        value_b,
        "peer{}={:?}, peer{}={:?}",
        site_a.peer_id(),
        value_a,
        site_b.peer_id(),
        value_b
    );
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

pub fn test_multi_sites_refactored(site_num: u8, actions: &mut [Action]) {
    let mut sites = Vec::new();
    for i in 0..site_num {
        let loro = LoroDoc::new();
        loro.set_peer_id(i as u64);
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
    }

    debug_log::group!("CheckSynced");
    // println!("{}", actions.table());
    check_synced_refactored(&mut sites);
    debug_log::group_end!();
}

#[cfg(test)]
mod test {
    use super::Action::*;
    use super::*;

    #[test]
    fn fuzz_r1() {
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
        test_multi_sites_refactored(
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
    fn mini_r() {
        minify_error(2, vec![], test_multi_sites_refactored, |_, ans| {
            ans.to_vec()
        })
    }
}
