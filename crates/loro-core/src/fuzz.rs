use std::{fmt::Debug, time::Instant};

use debug_log::debug_log;
use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};
pub mod recursive;

use crate::{array_mut_ref, id::ClientID, LoroCore, VersionVector};

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

impl Actionable for Vec<LoroCore> {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos, site } => {
                let site = &mut self[*site as usize];
                let mut text = site.get_text("text");
                text.insert(site, *pos, &content.to_string()).unwrap();
            }
            Action::Del { pos, len, site } => {
                let site = &mut self[*site as usize];
                let mut text = site.get_text("text");
                text.delete(site, *pos, *len).unwrap();
            }
            Action::Sync { from, to } => {
                let to_vv = self[*to as usize].vv();
                let from_exported = self[*from as usize].export(to_vv);
                self[*to as usize].import(from_exported);
            }
            Action::SyncAll => {
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    a.import(b.export(a.vv()));
                }
                for i in 1..self.len() {
                    let (a, b) = array_mut_ref!(self, [0, i]);
                    b.import(a.export(b.vv()));
                }
            }
        }
    }

    fn preprocess(&mut self, action: &mut Action) {
        match action {
            Action::Ins { pos, site, .. } => {
                *site %= self.len() as u8;
                let text = self[*site as usize].get_text("text");
                change_pos_to_char_boundary(pos, text.len());
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let text = self[*site as usize].get_text("text");
                if text.is_empty() {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                change_delete_to_char_boundary(pos, len, text.len());
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

fn check_eq(site_a: &mut LoroCore, site_b: &mut LoroCore) {
    let a = site_a.get_text("text");
    let b = site_b.get_text("text");
    let value_a = a.get_value();
    let value_b = b.get_value();
    assert_eq!(value_a.as_string().unwrap(), value_b.as_string().unwrap());
}

fn check_synced(sites: &mut [LoroCore]) {
    for i in 0..sites.len() - 1 {
        for j in i + 1..sites.len() {
            debug_log::group!("checking {} with {}", i, j);
            let (a, b) = array_mut_ref!(sites, [i, j]);
            {
                debug_log::group!("Import {}", i);
                a.import_updates(&b.export_updates(&a.vv()).unwrap())
                    .unwrap();
                debug_log::group_end!();
            }
            {
                debug_log::group!("Import {}", j);
                b.import_updates(&a.export_updates(&b.vv()).unwrap())
                    .unwrap();
                debug_log::group_end!();
            }
            check_eq(a, b);
            debug_log::group_end!();
        }
    }
}

pub fn test_single_client(mut actions: Vec<Action>) {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut text_container = store.get_text("haha");
    let mut ground_truth = String::new();
    let mut applied = Vec::new();
    for action in actions
        .iter_mut()
        .filter(|x| x.as_del().is_some() || x.as_ins().is_some())
    {
        ground_truth.preprocess(action);
        applied.push(action.clone());
        // println!("{}", (&applied).table());
        ground_truth.apply_action(action);
        match action {
            Action::Ins { content, pos, .. } => {
                text_container
                    .insert(&store, *pos, &content.to_string())
                    .unwrap();
            }
            Action::Del { pos, len, .. } => {
                if text_container.is_empty() {
                    return;
                }

                text_container.delete(&store, *pos, *len).unwrap();
            }
            _ => {}
        }
        assert_eq!(
            ground_truth.as_str(),
            &**text_container.get_value().as_string().unwrap(),
            "{}",
            applied.table()
        );
    }
}

pub fn test_single_client_encode(mut actions: Vec<Action>) {
    let mut store = LoroCore::new(Default::default(), None);
    let mut text_container = store.get_text("hello");
    let mut ground_truth = String::new();
    let mut applied = Vec::new();
    for action in actions
        .iter_mut()
        .filter(|x| x.as_del().is_some() || x.as_ins().is_some())
    {
        ground_truth.preprocess(action);
        applied.push(action.clone());
        // println!("{}", (&applied).table());
        ground_truth.apply_action(action);
        match action {
            Action::Ins { content, pos, .. } => {
                text_container
                    .insert(&store, *pos, &content.to_string())
                    .unwrap();
            }
            Action::Del { pos, len, .. } => {
                if text_container.is_empty() {
                    return;
                }

                text_container.delete(&store, *pos, *len).unwrap();
            }
            _ => {}
        }
    }
    let encode_bytes = store.encode_snapshot(&VersionVector::new());
    let json1 = store.to_json();
    let mut store2 = LoroCore::new(Default::default(), None);
    store2.decode_snapshot(&encode_bytes);
    let _encode_bytes2 = store2.encode_snapshot(&VersionVector::new());
    let json2 = store2.to_json();
    // state encode will change mergable range
    // assert_eq!(encode_bytes, encode_bytes2);
    assert_eq!(json1, json2);
}

pub fn minify_error<T, F, N>(site_num: u8, actions: Vec<T>, f: F, normalize: N)
where
    F: Fn(u8, &mut [T]),
    N: Fn(u8, &mut [T]) -> Vec<T>,
    T: Clone + Debug,
{
    std::panic::set_hook(Box::new(|_info| {
        // ignore panic output
    }));

    let f_ref: *const _ = &f;
    let f_ref: usize = f_ref as usize;
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

pub fn normalize(site_num: u8, actions: &mut [Action]) -> Vec<Action> {
    let mut sites = Vec::new();
    for i in 0..site_num {
        sites.push(LoroCore::new(Default::default(), Some(i as ClientID)));
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
        sites.push(LoroCore::new(Default::default(), Some(i as ClientID)));
    }

    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        sites.preprocess(action);
        applied.push(action.clone());
        debug_log!("\n{}", (&applied).table());
        sites.apply_action(action);
    }

    debug_log::group!("CheckSynced");
    // println!("{}", actions.table());
    check_synced(&mut sites);
    debug_log::group_end!();
}

#[cfg(test)]
mod test {
    use super::Action::*;
    use super::*;
    #[test]
    fn case1() {
        test_multi_sites(
            8,
            &mut vec![
                Ins {
                    content: 35108,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 18218,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 35624,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 38400,
                    pos: 0,
                    site: 6,
                },
                Ins {
                    content: 65280,
                    pos: 2,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 5,
                    site: 0,
                },
                Ins {
                    content: 60672,
                    pos: 0,
                    site: 1,
                },
                Ins {
                    content: 35072,
                    pos: 1,
                    site: 2,
                },
                Ins {
                    content: 15035,
                    pos: 3,
                    site: 0,
                },
                Ins {
                    content: 65280,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 201,
                    pos: 2,
                    site: 2,
                },
                Ins {
                    content: 65377,
                    pos: 3,
                    site: 1,
                },
                Ins {
                    content: 9988,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 14,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 11,
                    site: 7,
                },
                Ins {
                    content: 1070,
                    pos: 0,
                    site: 5,
                },
                Ins {
                    content: 27421,
                    pos: 7,
                    site: 1,
                },
                Ins {
                    content: 65121,
                    pos: 22,
                    site: 0,
                },
                Ins {
                    content: 65462,
                    pos: 1,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 0,
                    site: 4,
                },
                Ins {
                    content: 4626,
                    pos: 16,
                    site: 0,
                },
                Ins {
                    content: 65462,
                    pos: 11,
                    site: 2,
                },
                Ins {
                    content: 48009,
                    pos: 10,
                    site: 0,
                },
                Ins {
                    content: 23277,
                    pos: 7,
                    site: 0,
                },
                Ins {
                    content: 60672,
                    pos: 13,
                    site: 1,
                },
                Ins {
                    content: 4626,
                    pos: 2,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 2,
                    site: 0,
                },
                Ins {
                    content: 2606,
                    pos: 0,
                    site: 3,
                },
                Ins {
                    content: 65270,
                    pos: 10,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 65462,
                    pos: 107,
                    site: 4,
                },
                SyncAll,
                Ins {
                    content: 4626,
                    pos: 98,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 0,
                    pos: 0,
                    site: 0,
                },
                Del {
                    pos: 0,
                    len: 147,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 146,
                    site: 4,
                },
            ],
        )
    }

    #[test]
    fn case0() {
        test_multi_sites(
            4,
            &mut [
                Ins {
                    content: 31800,
                    pos: 723390690148040714,
                    site: 137,
                },
                Ins {
                    content: 2560,
                    pos: 12826352382887627018,
                    site: 178,
                },
                Sync { from: 178, to: 0 },
                Ins {
                    content: 35082,
                    pos: 12876550765177602139,
                    site: 178,
                },
            ],
        )
    }

    #[test]
    fn case_new_cache() {
        test_multi_sites(
            3,
            &mut [
                Ins {
                    content: 35108,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 35624,
                    pos: 0,
                    site: 0,
                },
                Del {
                    pos: 0,
                    len: 5,
                    site: 0,
                },
            ],
        )
    }

    #[test]
    fn case_two() {
        test_multi_sites(
            3,
            &mut [
                Ins {
                    content: 35108,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 18218,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 65280,
                    pos: 2,
                    site: 7,
                },
            ],
        )
    }

    #[test]
    fn mini() {
        minify_error(
            8,
            vec![
                Ins {
                    content: 35108,
                    pos: 0,
                    site: 2,
                },
                Ins {
                    content: 18218,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 35624,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 38400,
                    pos: 0,
                    site: 6,
                },
                Ins {
                    content: 65280,
                    pos: 2,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 5,
                    site: 0,
                },
                Ins {
                    content: 60672,
                    pos: 0,
                    site: 1,
                },
                Ins {
                    content: 35072,
                    pos: 1,
                    site: 2,
                },
                Ins {
                    content: 15035,
                    pos: 3,
                    site: 0,
                },
                Ins {
                    content: 65280,
                    pos: 0,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 201,
                    pos: 2,
                    site: 2,
                },
                Ins {
                    content: 65377,
                    pos: 3,
                    site: 1,
                },
                Ins {
                    content: 9988,
                    pos: 0,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 14,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 11,
                    site: 7,
                },
                Ins {
                    content: 1070,
                    pos: 0,
                    site: 5,
                },
                Ins {
                    content: 27421,
                    pos: 7,
                    site: 1,
                },
                Ins {
                    content: 65121,
                    pos: 22,
                    site: 0,
                },
                Ins {
                    content: 65462,
                    pos: 1,
                    site: 0,
                },
                Ins {
                    content: 4626,
                    pos: 0,
                    site: 4,
                },
                Ins {
                    content: 4626,
                    pos: 16,
                    site: 0,
                },
                Ins {
                    content: 65462,
                    pos: 11,
                    site: 2,
                },
                Ins {
                    content: 48009,
                    pos: 10,
                    site: 0,
                },
                Ins {
                    content: 23277,
                    pos: 7,
                    site: 0,
                },
                Ins {
                    content: 60672,
                    pos: 13,
                    site: 1,
                },
                Ins {
                    content: 4626,
                    pos: 2,
                    site: 7,
                },
                Ins {
                    content: 4626,
                    pos: 2,
                    site: 0,
                },
                Ins {
                    content: 2606,
                    pos: 0,
                    site: 3,
                },
                Ins {
                    content: 65270,
                    pos: 10,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 65462,
                    pos: 107,
                    site: 4,
                },
                SyncAll,
                Ins {
                    content: 4626,
                    pos: 98,
                    site: 0,
                },
                SyncAll,
                Ins {
                    content: 0,
                    pos: 0,
                    site: 0,
                },
                Del {
                    pos: 0,
                    len: 147,
                    site: 0,
                },
                Ins {
                    content: 0,
                    pos: 146,
                    site: 4,
                },
            ],
            test_multi_sites,
            normalize,
        )
    }

    #[test]
    fn case_encode() {
        test_single_client_encode(vec![Ins {
            content: 49087,
            pos: 4631600097073807295,
            site: 191,
        }])
    }
}
