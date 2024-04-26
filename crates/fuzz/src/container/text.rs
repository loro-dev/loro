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
    pos: usize,
    len: usize,
    action: TextActionInner,
}

#[derive(Debug, Clone)]
pub enum TextActionInner {
    Insert,
    Delete,
    Mark(usize),
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
        );
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
        let length = text.len_unicode();
        let TextAction { pos, len, action } = self;
        if matches!(action, TextActionInner::Delete | TextActionInner::Mark(_)) && length == 0 {
            *action = TextActionInner::Insert;
        }

        match &mut self.action {
            TextActionInner::Insert => {
                *pos %= length + 1;
            }
            TextActionInner::Delete => {
                *pos %= length;
                *len %= length - *pos;
                *len = 1.max(*len);
            }
            TextActionInner::Mark(i) => {
                *pos %= length;
                *len %= length - *pos;
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
        match action {
            TextActionInner::Insert => text.insert(*pos, &format!("[{}]", len)).unwrap(),
            TextActionInner::Delete => {
                text.delete(*pos, *len).unwrap();
            }
            TextActionInner::Mark(i) => {
                text.mark(*pos..*pos + *len, STYLES_NAME[*i], *pos as i32)
                    .unwrap();
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
                format!("{} ~ {}", pos, pos + len).into(),
            ],
        }
    }

    fn type_name(&self) -> &'static str {
        "Text"
    }
}

impl FromGenericAction for TextAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        let action_inner = match action.prop % 3 {
            0 => TextActionInner::Insert,
            1 => TextActionInner::Delete,
            2 => TextActionInner::Mark((action.key % 4) as usize),
            _ => unreachable!(),
        };

        TextAction {
            pos: action.pos,
            len: action.length,
            action: action_inner,
        }
    }
}
