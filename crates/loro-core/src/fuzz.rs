use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use tabled::{TableIteratorExt, Tabled};

use crate::{
    container::{text::text_container::TextContainer, Container},
    LoroCore,
};

#[derive(Arbitrary, EnumAsInner, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Ins { content: String, pos: usize },
    Del { pos: usize, len: usize },
}

impl Tabled for Action {
    const LENGTH: usize = 4;

    fn fields(&self) -> Vec<std::borrow::Cow<'_, str>> {
        match self {
            Action::Ins { content, pos } => vec![
                "ins".into(),
                pos.to_string().into(),
                content.len().to_string().into(),
                content.into(),
            ],
            Action::Del { pos, len } => vec![
                "del".into(),
                pos.to_string().into(),
                len.to_string().into(),
                "".into(),
            ],
        }
    }

    fn headers() -> Vec<std::borrow::Cow<'static, str>> {
        vec!["type".into(), "pos".into(), "len".into(), "content".into()]
    }
}

trait Actionable {
    fn apply_action(&mut self, action: &Action);
    fn preprocess(&self, action: &mut Action);
}

impl Action {
    pub fn preprocess(&mut self, max_len: usize) {
        match self {
            Action::Ins { pos, .. } => {
                *pos %= max_len + 1;
            }
            Action::Del { pos, len, .. } => {
                if max_len == 0 {
                    *pos = 0;
                    *len = 0;
                } else {
                    *pos %= max_len;
                    *len = (*len).min(max_len - (*pos));
                }
            }
        }
    }
}

impl Actionable for String {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos } => {
                self.insert_str(*pos, content);
            }
            &Action::Del { pos, len } => {
                if self.len() == 0 {
                    return;
                }

                self.drain(pos..pos + len);
            }
        }
    }

    fn preprocess(&self, action: &mut Action) {
        action.preprocess(self.len());
        match action {
            Action::Ins { content, pos } => {
                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % (self.len() + 1)
                }
            }
            Action::Del { pos, len } => {
                if self.len() == 0 {
                    *len = 0;
                    return;
                }

                let mut changed = false;
                while !self.is_char_boundary(*pos) {
                    *pos = (*pos + 1) % self.len();
                    changed = true;
                }

                if changed {
                    *len = 1;
                    while !self.is_char_boundary(*pos + *len) {
                        *len += 1;
                    }
                }
            }
        }
    }
}

impl Actionable for TextContainer {
    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Ins { content, pos } => {
                self.insert(*pos, content);
            }
            &Action::Del { pos, len } => {
                if self.text_len() == 0 {
                    return;
                }

                self.delete(pos, len);
            }
        }
    }

    fn preprocess(&self, action: &mut Action) {
        unreachable!();
    }
}

pub fn test_single_client(mut actions: Vec<Action>) {
    let mut store = LoroCore::new(Default::default(), None);
    let mut text_container = store.get_text_container("haha".into());
    let mut ground_truth = String::new();
    let mut applied = Vec::new();
    for action in actions.iter_mut() {
        ground_truth.preprocess(action);
        applied.push(action.clone());
        // println!("{}", (&applied).table());
        ground_truth.apply_action(action);
        text_container.apply_action(action);
        assert_eq!(
            ground_truth.as_str(),
            text_container.get_value().as_string().unwrap().as_str(),
            "{}",
            applied.table()
        );
    }
}

#[cfg(test)]
mod test {
    use ctor::ctor;

    use super::Action::*;
    use super::*;

    #[test]
    fn test() {
        test_single_client(vec![
            Ins {
                content: "璤\u{13}\u{13}\u{13}".into(),
                pos: 243,
            },
            Ins {
                content: "\0\0\0\0?\0\0\0".into(),
                pos: 11240984669950312448,
            },
            Ins {
                content: "\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".into(),
                pos: 129,
            },
            Ins {
                content: "".into(),
                pos: 91344728162323,
            },
            Ins {
                content: "".into(),
                pos: 6148914691236517150,
            },
            Del {
                pos: 12460594852558187539,
                len: 1430476722683303114,
            },
        ])
    }

    #[ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }
}
