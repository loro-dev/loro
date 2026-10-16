mod common;
use common::*;
use loro::{ContainerTrait, LoroDoc, LoroMap};
use loro_model::{ErrorKind, Loc, ValueField, View};
use std::sync::{Arc, Mutex};

fn setup() -> (LoroDoc, ProjectModel) {
    let d = LoroDoc::new();
    let m = ProjectModel::attach(d.get_map("project"));
    m.update_from(&Project {
        title: "p".into(),
        todos: vec![todo("a", "A"), todo("b", "B")],
        notes: "".into(),
    })
    .unwrap();
    d.commit();
    (d, m)
}

fn order(d: &LoroDoc) -> loro::LoroMovableList {
    d.get_map("project")
        .get("todos")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
        .get("order")
        .unwrap()
        .into_container()
        .unwrap()
        .into_movable_list()
        .unwrap()
}

#[test]
fn move_with_repeated_order_key() {
    let (d, m) = setup();
    let o = order(&d);
    o.insert(1, "a").unwrap(); // [a, a, b], a state allowed after concurrent creation
    assert_eq!(m.todos().keys().unwrap(), ["a", "b"]);
    m.todos().move_to("a", 1).unwrap();
    assert_eq!(m.todos().keys().unwrap(), ["b", "a"]);
}

#[test]
fn insert_before_unordered_item() {
    let (d, m) = setup();
    let o = order(&d);
    o.delete(0, o.len()).unwrap(); // live items remain visible, sorted
    assert_eq!(m.todos().keys().unwrap(), ["a", "b"]);
    m.todos().insert(1, &todo("x", "X")).unwrap();
    assert_eq!(m.todos().keys().unwrap(), ["a", "x", "b"]);
}

#[test]
fn watch_after_parent_replacement() {
    let (d, m) = setup();
    let t = m.todos().by_key("a").unwrap().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let _sub = t
        .title()
        .watch(move |old: &String, new: &String| s.lock().unwrap().push((old.clone(), new.clone())))
        .unwrap();
    let items = d
        .get_map("project")
        .get("todos")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
        .get("items")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let new = items.insert_container("a", LoroMap::new()).unwrap();
    new.insert("id", "a").unwrap();
    new.insert("title", "replaced").unwrap();
    new.insert("done", false).unwrap();
    d.commit();
    assert_eq!(t.title().get().unwrap(), "replaced");
    new.insert("title", "again").unwrap();
    d.commit();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            ("A".to_string(), "replaced".to_string()),
            ("replaced".to_string(), "again".to_string())
        ]
    );
}

#[test]
fn update_keyed_item_cannot_change_identity() {
    let (d, m) = setup();
    let t = m.todos().by_key("a").unwrap().unwrap();
    let before = d.len_ops();
    let err = t.update_from(&todo("b", "changed")).unwrap_err();
    assert!(
        matches!(err.kind, ErrorKind::KeyMismatch { expected, found } if expected == "a" && found == "b")
    );
    assert!(t.edit(|v: &mut Todo| v.id = "b".into()).is_err());
    assert_eq!(t.id().unwrap(), "a");
    assert_eq!(t.title().get().unwrap(), "A");
    assert_eq!(d.len_ops(), before);
    t.update_from(&todo("a", "allowed")).unwrap();
    assert_eq!(t.title().get().unwrap(), "allowed");
}

#[test]
fn projection_of_plain_map_value() {
    let d = LoroDoc::new();
    let root = d.get_map("root");
    root.insert(
        "todo",
        loro::loro_value!({"id":"a", "title":"A", "done":false}),
    )
    .unwrap();
    let loc = loro_model::Loc::field(&loro_model::Loc::root(root.to_container()), "todo");
    let view = TodoModel::from_loc(loc);
    assert_eq!(view.read_as::<Todo>().unwrap(), todo("a", "A"));
    let summary: TodoSummary = view.read_as().unwrap();
    assert_eq!(summary.id, "a");
    assert_eq!(summary.title, "A");
    root.insert(
        "todo",
        loro::loro_value!({"id":"a", "title":42, "done":false}),
    )
    .unwrap();
    let err = view.read_as::<Todo>().unwrap_err();
    assert_eq!(err.path_string(), "title");
    assert!(matches!(err.kind, ErrorKind::TypeMismatch { .. }));

    let project = Project {
        title: "P".into(),
        todos: vec![todo("a", "A")],
        notes: "notes".into(),
    };
    root.insert("project", loro::loro_value!({"title":"P", "todos":[{"id":"a", "title":"A", "done":false}], "notes":"notes"})).unwrap();
    let loc = Loc::field(&Loc::root(root.to_container()), "project");
    assert_eq!(loc.read::<Project>().unwrap(), project);
    root.insert("project", 42).unwrap();
    assert!(matches!(
        loc.read::<Project>().unwrap_err().kind,
        ErrorKind::TypeMismatch { .. }
    ));
}

#[test]
fn move_after_real_concurrent_creation() {
    for (peer_a, peer_b) in [(1, 2), (2, 1)] {
        let a = LoroDoc::new();
        a.set_peer_id(peer_a).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(peer_b).unwrap();
        let ma = ProjectModel::attach(a.get_map("project"));
        let mb = ProjectModel::attach(b.get_map("project"));
        ma.update_from(&Project {
            title: "p".into(),
            todos: vec![todo("a", "A")],
            notes: "".into(),
        })
        .unwrap();
        mb.update_from(&Project {
            title: "p".into(),
            todos: vec![todo("a", "A"), todo("b", "B")],
            notes: "".into(),
        })
        .unwrap();
        a.import(&b.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        ma.todos().move_to("a", 1).unwrap();
        assert_eq!(ma.todos().keys().unwrap(), ["b", "a"]);
        b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(mb.todos().keys().unwrap(), ["b", "a"]);
    }
}

#[test]
fn indexed_edits_preserve_positions_in_irregular_orders() {
    let layouts: &[&[&str]] = &[
        &[],
        &["a"],
        &["b"],
        &["a", "a", "b"],
        &["b", "a", "b"],
        &["dead", "b", "a", "a", "dead"],
        &["b", "b"],
    ];
    for layout in layouts {
        for at in 0..=2 {
            let (d, m) = setup();
            let o = order(&d);
            o.delete(0, o.len()).unwrap();
            for k in *layout {
                o.push(*k).unwrap();
            }
            // Invalid order values, like dead keys, must stay invisible.
            o.push(42).unwrap();
            let mut expected = m.todos().keys().unwrap();
            expected.insert(at, "x".into());
            m.todos().insert(at, &todo("x", "X")).unwrap();
            assert_eq!(
                m.todos().keys().unwrap(),
                expected,
                "insert {at} into {layout:?}"
            );
            assert_eq!(o.len(), 3);
        }
        for key in ["a", "b"] {
            for to in 0..2 {
                let (d, m) = setup();
                let o = order(&d);
                o.delete(0, o.len()).unwrap();
                for k in *layout {
                    o.push(*k).unwrap();
                }
                let mut expected = m.todos().keys().unwrap();
                let from = expected.iter().position(|k| k == key).unwrap();
                let k = expected.remove(from);
                expected.insert(to, k);
                m.todos().move_to(key, to).unwrap();
                assert_eq!(
                    m.todos().keys().unwrap(),
                    expected,
                    "move {key} to {to} in {layout:?}"
                );
            }
        }
    }
}

#[test]
fn nested_subscription_filters_siblings_and_observes_descendants() {
    let (d, m) = setup();
    let a = m.todos().by_key("a").unwrap().unwrap();
    let calls = Arc::new(Mutex::new(0));
    let c = calls.clone();
    let sub = a
        .title()
        .subscribe(move |_| *c.lock().unwrap() += 1)
        .unwrap();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let c = changes.clone();
    let _watch = m
        .todos()
        .watch(move |_: &Vec<Todo>, new: &Vec<Todo>| c.lock().unwrap().push(new.clone()))
        .unwrap();
    a.done().set(true).unwrap();
    m.todos()
        .by_key("b")
        .unwrap()
        .unwrap()
        .title()
        .set("other")
        .unwrap();
    m.title().set("other project title").unwrap();
    d.commit();
    assert_eq!(*calls.lock().unwrap(), 0);
    assert_eq!(changes.lock().unwrap().len(), 1);
    a.title().set("changed").unwrap();
    d.commit();
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(changes.lock().unwrap().len(), 2);
    sub.unsubscribe();
    a.title().set("unsubscribed").unwrap();
    d.commit();
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn watch_missing_ancestors_and_remote_replacements() {
    let d = LoroDoc::new();
    d.set_peer_id(1).unwrap();
    let root = d.get_map("root");
    let loc = Loc::field(
        &Loc::field(&Loc::root(root.to_container()), "nested"),
        "value",
    );
    let field: ValueField<Option<String>> = View::from_loc(loc);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let _sub = field
        .watch(move |old: &Option<String>, new: &Option<String>| {
            s.lock().unwrap().push((old.clone(), new.clone()))
        })
        .unwrap();
    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    let parent = remote
        .get_map("root")
        .insert_container("nested", LoroMap::new())
        .unwrap();
    parent.insert("value", "one").unwrap();
    d.import(&remote.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    let v1 = d.state_frontiers();
    let replacement = remote
        .get_map("root")
        .insert_container("nested", LoroMap::new())
        .unwrap();
    replacement.insert("value", "two").unwrap();
    d.import(&remote.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    d.checkout(&v1).unwrap();
    d.checkout_to_latest();
    root.delete("nested").unwrap();
    d.commit();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (None, Some("one".into())),
            (Some("one".into()), Some("two".into())),
            (Some("two".into()), Some("one".into())),
            (Some("one".into()), Some("two".into())),
            (Some("two".into()), None),
        ]
    );
}
