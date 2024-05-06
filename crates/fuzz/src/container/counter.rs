use std::sync::{Arc, Mutex};

use loro::{event::Diff, Container, ContainerID, ContainerType, LoroCounter, LoroDoc, LoroValue};
use tracing::debug_span;

use crate::{
    actions::{Actionable, FromGenericAction, GenericAction},
    actor::{ActionExecutor, ActorTrait},
    value::{ApplyDiff, ContainerTracker, MapTracker, Value},
};

#[derive(Debug, Clone)]
pub struct CounterAction(i32);

pub struct CounterActor {
    loro: Arc<LoroDoc>,
    containers: Vec<LoroCounter>,
    tracker: Arc<Mutex<ContainerTracker>>,
}

impl CounterActor {
    pub fn new(loro: Arc<LoroDoc>) -> Self {
        let mut tracker = MapTracker::empty(ContainerID::new_root("sys:root", ContainerType::Map));
        tracker.insert(
            "counter".to_string(),
            Value::empty_container(
                ContainerType::Counter,
                ContainerID::new_root("counter", ContainerType::Counter),
            ),
        );
        let tracker = Arc::new(Mutex::new(ContainerTracker::Map(tracker)));
        let counter = tracker.clone();

        let peer_id = loro.peer_id();
        loro.subscribe(
            &ContainerID::new_root("counter", ContainerType::Counter),
            Arc::new(move |event| {
                let s = debug_span!("Counter event", peer = peer_id);
                let _g = s.enter();
                let mut counter = counter.lock().unwrap();
                counter.apply_diff(event);
            }),
        );

        let root = loro.get_counter("counter");
        Self {
            loro,
            containers: vec![root],
            tracker,
        }
    }
}

impl ActorTrait for CounterActor {
    fn container_len(&self) -> u8 {
        self.containers.len() as u8
    }

    #[doc = " check the value of root container is equal to the tracker"]
    fn check_tracker(&self) {
        let loro = &self.loro;
        let counter = loro.get_counter("counter");
        let result = counter.get_value();
        let tracker = self.tracker.lock().unwrap().to_value();
        assert_eq!(&result, tracker.into_map().unwrap().get("counter").unwrap());
    }

    fn add_new_container(&mut self, container: Container) {
        self.containers.push(container.into_counter().unwrap());
    }
}

impl Actionable for CounterAction {
    fn pre_process(&mut self, _actor: &mut ActionExecutor, _container: usize) {
        self.0 = self.0.rem_euclid(100)
    }

    fn apply(&self, actor: &mut ActionExecutor, container: usize) -> Option<Container> {
        let actor = actor.as_counter_actor_mut().unwrap();
        let counter = actor.containers.get(container).unwrap();
        counter.increment(self.0 as i64);
        None
    }

    fn ty(&self) -> ContainerType {
        ContainerType::Counter
    }

    fn table_fields(&self) -> [std::borrow::Cow<'_, str>; 2] {
        ["increment".into(), self.0.to_string().into()]
    }

    fn type_name(&self) -> &'static str {
        "Counter"
    }

    fn pre_process_container_value(&mut self) -> Option<&mut ContainerType> {
        None
    }
}

impl FromGenericAction for CounterAction {
    fn from_generic_action(action: &GenericAction) -> Self {
        let pos = action.bool;
        let v = if pos {
            action.prop as i32
        } else {
            -(action.prop as i32)
        };
        CounterAction(v)
    }
}

#[derive(Debug)]
pub struct CounterTracker {
    v: i64,
    id: ContainerID,
}

impl ApplyDiff for CounterTracker {
    fn empty(id: ContainerID) -> Self {
        Self { v: 0, id }
    }

    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn apply_diff(&mut self, diff: loro::event::Diff) {
        if let Diff::Counter(v) = diff {
            self.v += v;
        }
    }

    fn to_value(&self) -> LoroValue {
        LoroValue::I64(self.v)
    }
}
