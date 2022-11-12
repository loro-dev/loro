use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};
pub mod recursive;

use crate::{
    array_mut_ref, container::registry::ContainerWrapper, debug_log, id::ClientID, LoroCore,
};

#[derive(arbitrary::Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins { content: u32, pos: usize, site: u8 },
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
                text.insert(site, *pos, &content.to_string());
            }
            Action::Del { pos, len, site } => {
                let site = &mut self[*site as usize];
                let mut text = site.get_text("text");
                text.delete(site, *pos, *len);
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
                change_pos_to_char_boundary(pos, text.text_len());
            }
            Action::Del { pos, len, site } => {
                *site %= self.len() as u8;
                let text = self[*site as usize].get_text("text");
                if text.text_len() == 0 {
                    *len = 0;
                    *pos = 0;
                    return;
                }

                change_delete_to_char_boundary(pos, len, text.text_len());
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
            debug_log!("-------------------------------");
            debug_log!("checking {} with {}", i, j);
            debug_log!("-------------------------------");

            let (a, b) = array_mut_ref!(sites, [i, j]);
            a.import(b.export(a.vv()));
            b.import(a.export(b.vv()));
            check_eq(a, b)
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
                text_container.insert(&store, *pos, &content.to_string());
            }
            Action::Del { pos, len, .. } => {
                if text_container.text_len() == 0 {
                    return;
                }

                text_container.delete(&store, *pos, *len);
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

pub fn test_multi_sites(site_num: u8, mut actions: Vec<Action>) {
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

    debug_log!("=================================");
    // println!("{}", actions.table());
    check_synced(&mut sites);
}

#[cfg(test)]
mod test {}
