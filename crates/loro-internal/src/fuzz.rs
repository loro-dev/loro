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
                text.insert(&mut txn, *pos, &content.to_string()).unwrap();
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
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let app_state = &mut self[*site as usize].app_state().lock().unwrap();
                let text = app_state.get_text("text").unwrap();
                if text.is_empty() {
                    *len = 0;
                    *pos = 0;
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
    fn mini_r() {
        minify_error(8, vec![], test_multi_sites_refactored, |_, ans| {
            ans.to_vec()
        })
    }
}
