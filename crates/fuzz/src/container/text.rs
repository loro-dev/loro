use std::sync::{Arc, Mutex};

use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroText};

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

const STYLES_NAME: [&str; 4] = ["bold", "comment", "link", "highlight"];

#[derive(Debug, Clone)]
pub struct TextAction {
    pub pos: usize,
    pub len: usize,
    pub action: TextActionInner,
}

#[derive(Debug, Clone)]
pub enum TextActionInner {
    Insert,
    Delete,
    Mark(usize),
    Update,
    InsertUtf8,
    DeleteUtf8,
    MarkUtf8(usize),
    Splice,
    Unmark(usize),
}

pub struct TextActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroText>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl TextActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "text".to_string(),
            Value::empty_container(
                ContainerType::Text,
                ContainerID::new_root("text", ContainerType::Text),
            ),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let text = tracker.clone();
        loro.subscribe(
            &ContainerID::new_root("text", ContainerType::Text),
            Arc::new(move |event| {
                text.lock().unwrap().apply_diff(event);
            }),
        )
        .detach();
        let root = loro.get_text("text");
        TextActor {
            loro,
            containers: vec![root],
            tracker,
        }
    }
}

impl ActorTrait for TextActor {
    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }

    fn check_tracker(&self) {
        let loro = &self.loro;
        let text = loro.get_text("text");
        // check delta
        let value = text.to_delta();
        let tracker = self.tracker.lock().unwrap();
        let text = tracker.as_map().unwrap().get("text").unwrap();
        let text_h = text
            .as_container()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .to_delta();
        assert_eq!(value, text_h);
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_text().unwrap());
    }
}

impl Actionable for TextAction {
    fn pre_process(&mut self, actor: &mut ActionExecutor, container: usize) {
        let actor = actor.as_text_actor_mut().unwrap();
        let text = actor.containers.get(container).unwrap();
        let unicode_len = text.len_unicode();
        let utf8_len = text.len_utf8();
        let TextAction { pos, len, action } = self;
        if matches!(
            action,
            TextActionInner::Delete
                | TextActionInner::Mark(_)
                | TextActionInner::DeleteUtf8
                | TextActionInner::MarkUtf8(_)
                | TextActionInner::Splice
                | TextActionInner::Unmark(_)
        ) && unicode_len == 0
        {
            *action = TextActionInner::Insert;
        }

        match &mut self.action {
            TextActionInner::Insert => {
                *pos %= unicode_len + 1;
            }
            TextActionInner::Delete => {
                *pos %= unicode_len;
                *len %= unicode_len - *pos;
                *len = 1.max(*len);
            }
            TextActionInner::Mark(i) => {
                *pos %= unicode_len;
                *len %= unicode_len - *pos;
                *len = 1.max(*len);
                *i %= STYLES_NAME.len();
            }
            TextActionInner::Update => {}
            TextActionInner::InsertUtf8 => {
                *pos %= utf8_len + 1;
            }
            TextActionInner::DeleteUtf8 => {
                *pos %= utf8_len;
                *len %= utf8_len - *pos;
                *len = 1.max(*len);
            }
            TextActionInner::MarkUtf8(i) => {
                *pos %= utf8_len;
                *len %= utf8_len - *pos;
                *len = 1.max(*len);
                *i %= STYLES_NAME.len();
            }
            TextActionInner::Splice => {
                *pos %= unicode_len;
                *len %= unicode_len - *pos;
                *len = 1.max(*len);
            }
            TextActionInner::Unmark(i) => {
                *pos %= unicode_len;
                *len %= unicode_len - *pos;
                *len = 1.max(*len);
                *i %= STYLES_NAME.len();
            }
        }
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        None
    }

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_text_actor_mut().unwrap();
        let text = actor.containers.get(container).unwrap();
        let TextAction { pos, len, action } = self;
        use super::unwrap;
        match action {
            TextActionInner::Insert => {
                unwrap(text.insert(*pos, &format!("[{len}]")));
            }
            TextActionInner::Delete => {
                unwrap(text.delete(*pos, *len));
            }
            TextActionInner::Mark(i) => {
                unwrap(text.mark(*pos..*pos + *len, STYLES_NAME[*i], *pos as i32));
            }
            TextActionInner::Update => {
                if text.is_deleted() {
                    return None;
                }
                let new_text = format!("u{}", *len);
                text.update(&new_text, Default::default()).ok();
            }
            TextActionInner::InsertUtf8 => {
                unwrap(text.insert_utf8(*pos, &format!("[{len}]")));
            }
            TextActionInner::DeleteUtf8 => {
                unwrap(text.delete_utf8(*pos, *len));
            }
            TextActionInner::MarkUtf8(i) => {
                unwrap(text.mark_utf8(*pos..*pos + *len, STYLES_NAME[*i], *pos as i32));
            }
            TextActionInner::Splice => {
                text.splice(*pos, *len, &format!("s{len}")).ok();
            }
            TextActionInner::Unmark(i) => {
                text.unmark(*pos..*pos + *len, STYLES_NAME[*i]).ok();
            }
        }
        None
    }

    fn ty(&self) -> ContainerType {
        ContainerType::Text
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        let pos = self.pos;
        let len = self.len;
        match self.action {
            TextActionInner::Insert => [format!("insert {}", pos).into(), len.to_string().into()],
            TextActionInner::Delete => ["delete".into(), format!("{} ~ {}", pos, pos + len).into()],
            TextActionInner::Mark(i) => [
                format!("mark {} ", STYLES_NAME[i]).into(),
                format!("{} with-len {}", pos, len).into(),
            ],
            TextActionInner::Update => ["update".into(), format!("to {}", len).into()],
            TextActionInner::InsertUtf8 => [
                format!("insert_utf8 {}", pos).into(),
                len.to_string().into(),
            ],
            TextActionInner::DeleteUtf8 => [
                "delete_utf8".into(),
                format!("{} ~ {}", pos, pos + len).into(),
            ],
            TextActionInner::MarkUtf8(i) => [
                format!("mark_utf8 {} ", STYLES_NAME[i]).into(),
                format!("{} with-len {}", pos, len).into(),
            ],
            TextActionInner::Splice => ["splice".into(), format!("{} ~ {}", pos, pos + len).into()],
            TextActionInner::Unmark(i) => [
                format!("unmark {} ", STYLES_NAME[i]).into(),
                format!("{} with-len {}", pos, len).into(),
            ],
        }
    }

    fn type_name(&self) -> &'static str {
        "Text"
    }
}

impl FromGenericAction for TextAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        let action_inner = match action.prop % 9 {
            0 => TextActionInner::Insert,
            1 => TextActionInner::Delete,
            2 => TextActionInner::Mark((action.key % 4) as usize),
            3 => TextActionInner::Update,
            4 => TextActionInner::InsertUtf8,
            5 => TextActionInner::DeleteUtf8,
            6 => TextActionInner::MarkUtf8((action.key % 4) as usize),
            7 => TextActionInner::Splice,
            8 => TextActionInner::Unmark((action.key % 4) as usize),
            _ => unreachable!(),
        };

        TextAction {
            pos: action.pos,
            len: action.length,
            action: action_inner,
        }
    }
}
