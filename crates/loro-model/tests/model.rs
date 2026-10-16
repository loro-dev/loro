mod common;

use std::sync::{Arc, Mutex};

use common::*;
use loro::{ExportMode, LoroDoc, LoroList, UndoManager};
use loro_model::{ErrorKind, View};
use rustc_hash::FxHashMap;

fn initial() -> Project {
    Project {
        title: "Issue 888 design".into(),
        todos: vec![
            todo("todo-1", "Write model plan"),
            todo("todo-2", "Validate"),
        ],
        notes: "Design notes\n".into(),
    }
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
}

/// Ops emitted by `f` (committed).
fn ops(doc: &LoroDoc, f: impl FnOnce()) -> usize {
    doc.commit();
    let before = doc.len_ops();
    f();
    doc.commit();
    doc.len_ops() - before
}

/// The plan's end-to-end example, step by step.
#[test]
fn end_to_end_example() {
    // Full reconcile + full projection.
    let alice = LoroDoc::new();
    alice.set_peer_id(1).unwrap();
    let project = ProjectModel::attach(alice.get_map("project"));
    project.update_from(&initial()).unwrap();
    assert_eq!(project.read_as::<Project>().unwrap(), initial());

    // Partial local updates through generated accessors.
    project.title().set("Typed model plan").unwrap();
    let todo1 = project.todos().by_key("todo-1").unwrap().unwrap();
    todo1.title().set("Write the typed model plan").unwrap();
    todo1.done().set(true).unwrap();
    project.notes().splice(0, 0, "Decision log\n").unwrap();
    assert_eq!(project.title().get().unwrap(), "Typed model plan");
    assert!(todo1.done().get().unwrap());
    assert_eq!(
        project.notes().get().unwrap(),
        "Decision log\nDesign notes\n"
    );
    alice.commit();

    // Import: Bob's views read current state; there is no pull() step.
    let bob = LoroDoc::new();
    bob.set_peer_id(2).unwrap();
    bob.import(&alice.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let bob_project = ProjectModel::attach(bob.get_map("project"));
    let bob_todo1 = bob_project.todos().by_key("todo-1").unwrap().unwrap();

    // Watch with old/new values on a keyed path.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let _sub = bob_todo1
        .title()
        .watch(move |old: &String, new: &String| {
            seen2.lock().unwrap().push((old.clone(), new.clone()));
        })
        .unwrap();

    // Alice inserts at the front (shifting indices) and renames todo-1.
    project
        .todos()
        .insert(0, &todo("todo-0", "Prepend"))
        .unwrap();
    todo1.title().set("Renamed by Alice").unwrap();
    alice.commit();
    sync(&alice, &bob);

    // The keyed view is bound to the element container, so the index shift
    // does not redirect it.
    assert_eq!(bob_todo1.title().get().unwrap(), "Renamed by Alice");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(
            "Write the typed model plan".to_string(),
            "Renamed by Alice".to_string()
        )]
    );

    // Project a subtree into an application struct.
    let summaries: Vec<TodoSummary> = bob_project.todos().read_as().unwrap();
    assert_eq!(
        summaries.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["todo-0", "todo-1", "todo-2"]
    );
    // ... or into std collections.
    let plain: Vec<FxHashMap<String, loro::LoroValue>> = bob_project.todos().read_as().unwrap();
    assert_eq!(plain.len(), 3);
}

#[test]
fn update_from_emits_minimal_ops() {
    let doc = LoroDoc::new();
    let project = ProjectModel::attach(doc.get_map("project"));
    let mut p = initial();
    p.todos = (0..100).map(|i| todo(&format!("t{i}"), "x")).collect();
    project.update_from(&p).unwrap();

    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 0);

    p.todos[50].title = "changed".into();
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 1);

    // Insert at the front: order entry + item marker + three fields.
    p.todos.insert(0, todo("new", "front"));
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 5);

    // Delete in the middle: the item and its order entry.
    p.todos.remove(30);
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 2);

    // Move last to first, and first to last: one move op each.
    let last = p.todos.pop().unwrap();
    p.todos.insert(0, last);
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 1);
    let first = p.todos.remove(0);
    p.todos.push(first);
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 1);

    // Reverse: n - 1 moves (LIS of length 1).
    p.todos.reverse();
    let n = p.todos.len();
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), n - 1);
    assert_eq!(project.read_as::<Project>().unwrap(), p);

    // Text field: a one-character edit is a small text op, not a replacement.
    p.notes = "Design notes!\n".into();
    assert_eq!(ops(&doc, || project.update_from(&p).unwrap()), 1);
}

/// Views and watchers stay correct across every state change, not just
/// import: checkout, revert_to, undo.
#[test]
fn views_are_fresh_without_pull() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let project = ProjectModel::attach(doc.get_map("project"));
    project.update_from(&initial()).unwrap();
    doc.commit();
    let v1 = doc.state_frontiers();

    let changes = Arc::new(Mutex::new(Vec::new()));
    let c2 = changes.clone();
    let _w = project
        .title()
        .watch(move |old: &String, new: &String| c2.lock().unwrap().push(format!("{old} -> {new}")))
        .unwrap();

    project.title().set("v2").unwrap();
    // Uncommitted local edits are visible to views immediately ...
    assert_eq!(project.title().get().unwrap(), "v2");
    // ... and watchers fire on commit.
    assert!(changes.lock().unwrap().is_empty());
    doc.commit();
    assert_eq!(changes.lock().unwrap().len(), 1);

    // (Undo first: a checkout clears the UndoManager stacks by design.)
    undo.undo().unwrap();
    assert_eq!(project.title().get().unwrap(), "Issue 888 design");
    undo.redo().unwrap();
    assert_eq!(project.title().get().unwrap(), "v2");

    doc.checkout(&v1).unwrap();
    assert_eq!(project.title().get().unwrap(), "Issue 888 design");
    doc.checkout_to_latest();
    assert_eq!(project.title().get().unwrap(), "v2");

    doc.revert_to(&v1).unwrap();
    doc.commit();
    assert_eq!(project.title().get().unwrap(), "Issue 888 design");

    assert_eq!(
        *changes.lock().unwrap(),
        [
            "Issue 888 design -> v2",
            "v2 -> Issue 888 design", // undo
            "Issue 888 design -> v2", // redo
            "v2 -> Issue 888 design", // checkout
            "Issue 888 design -> v2", // checkout_to_latest
            "v2 -> Issue 888 design", // revert_to
        ]
    );
}

/// Two peers that initialize the same model offline converge to one `todos`
/// list because struct fields use mergeable child containers.
#[test]
fn concurrent_init_merges_instead_of_dropping_a_subtree() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let pa = ProjectModel::attach(a.get_map("project"));
    let pb = ProjectModel::attach(b.get_map("project"));
    let mut init_a = initial();
    init_a.todos = vec![todo("shared", "from a"), todo("only-a", "a")];
    let mut init_b = initial();
    init_b.todos = vec![todo("shared", "from b"), todo("only-b", "b")];
    pa.update_from(&init_a).unwrap();
    pb.update_from(&init_b).unwrap();
    sync(&a, &b);

    let ra: Project = pa.read_as().unwrap();
    let rb: Project = pb.read_as().unwrap();
    assert_eq!(ra, rb);
    let mut ids: Vec<_> = ra.todos.iter().map(|t| t.id.as_str()).collect();
    ids.sort();
    // Both peers' items survive, and "shared" is one element: both peers wrote
    // into the same mergeable child, so its fields merged (LWW per field).
    assert_eq!(ids, ["only-a", "only-b", "shared"]);
    assert_eq!(pa.todos().len().unwrap(), 3);

    // `order` still lists "shared" twice; the next reconcile drops the
    // repeat, then reconcile is a no-op.
    assert_eq!(ops(&a, || pa.update_from(&ra).unwrap()), 1);
    assert_eq!(ops(&a, || pa.update_from(&ra).unwrap()), 0);
}

/// Baseline for the test above: with regular `insert_container`, map LWW keeps
/// one peer's list and the other peer's items are gone.
#[test]
fn baseline_regular_containers_lose_concurrent_init() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    for (doc, item) in [(&a, "only-a"), (&b, "only-b")] {
        let list = doc
            .get_map("project")
            .insert_container("todos", LoroList::new())
            .unwrap();
        list.push(item).unwrap();
    }
    a.commit();
    b.commit();
    sync(&a, &b);
    let v = a.get_map("project").get_deep_value();
    let todos = v.as_map().unwrap().get("todos").unwrap().as_list().unwrap();
    assert_eq!(todos.len(), 1);
}

#[test]
fn errors_have_paths() {
    let doc = LoroDoc::new();
    let project = ProjectModel::attach(doc.get_map("project"));
    project.update_from(&initial()).unwrap();
    // Another client writes a number where a string is expected.
    let todo2 = project.todos().by_key("todo-2").unwrap().unwrap();
    todo2
        .loc()
        .ensure_map()
        .unwrap()
        .insert("title", 42)
        .unwrap();
    let err = project.read_as::<Project>().unwrap_err();
    assert_eq!(err.path_string(), "todos[1].title");
    assert!(matches!(err.kind, ErrorKind::TypeMismatch { .. }));
    assert_eq!(
        err.to_string(),
        "todos[1].title: expected string, found I64(42)"
    );

    let err = project.read_as::<Vec<u8>>().unwrap_err();
    assert!(matches!(err.kind, ErrorKind::TypeMismatch { .. }));
}

/// Struct reconcile keeps fields it does not know about, so an older client
/// does not erase fields written by a newer schema.
#[test]
fn unknown_fields_are_preserved() {
    let doc = LoroDoc::new();
    let root = doc.get_map("project");
    root.insert("priority", "high").unwrap();
    let project = ProjectModel::attach(root.clone());
    project.update_from(&initial()).unwrap();
    project.update_from(&initial()).unwrap();
    assert_eq!(
        root.get("priority").unwrap().get_deep_value(),
        "high".into()
    );
}

#[test]
fn subscription_on_a_container_covers_its_descendants() {
    let doc = LoroDoc::new();
    let project = ProjectModel::attach(doc.get_map("project"));
    project.update_from(&initial()).unwrap();
    doc.commit();
    let hits = Arc::new(Mutex::new(0));
    let h = hits.clone();
    let _s = project.subscribe(move |_| *h.lock().unwrap() += 1).unwrap();
    project
        .todos()
        .by_key("todo-2")
        .unwrap()
        .unwrap()
        .done()
        .set(true)
        .unwrap();
    doc.commit();
    assert_eq!(*hits.lock().unwrap(), 1);
}

#[test]
fn watch_filters_unrelated_fields() {
    let doc = LoroDoc::new();
    let project = ProjectModel::attach(doc.get_map("project"));
    project.update_from(&initial()).unwrap();
    doc.commit();
    let calls = Arc::new(Mutex::new(0));
    let c = calls.clone();
    let _w = project
        .title()
        .subscribe(move |_| *c.lock().unwrap() += 1)
        .unwrap();
    project.notes().splice(0, 0, "x").unwrap();
    project.todos().push(&todo("t3", "x")).unwrap();
    doc.commit();
    assert_eq!(*calls.lock().unwrap(), 0);
    project.title().set("t").unwrap();
    doc.commit();
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn std_collections_roundtrip() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let model = ProjectModel::attach(root.clone());
    let todos = model.todos();
    let rows: Vec<FxHashMap<String, String>> = vec![
        [("a".to_string(), "1".to_string())].into_iter().collect(),
        [("b".to_string(), "2".to_string())].into_iter().collect(),
    ];
    // A `ListView<Todo>` can be written from any ToLoro value; it is the
    // caller's choice to store a different shape.
    let untyped: loro_model::ValueField<Vec<FxHashMap<String, String>>> =
        View::from_loc(todos.loc().clone());
    untyped.update_from(&rows).unwrap();
    assert_eq!(
        untyped.read_as::<Vec<FxHashMap<String, String>>>().unwrap(),
        rows
    );
}

/// An item without an `order` entry (reachable when two peers dedupe a repeated
/// key differently after a concurrent move) stays visible at the end and is
/// re-ordered by the next reconcile.
#[test]
fn unordered_items_stay_visible() {
    let doc = LoroDoc::new();
    let project = ProjectModel::attach(doc.get_map("project"));
    let mut p = initial();
    p.todos.push(todo("todo-0", "zero"));
    project.update_from(&p).unwrap();
    // Drop todo-1's order entry, keeping its item.
    let order = doc
        .get_map("project")
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
        .unwrap();
    order.delete(0, 1).unwrap();

    let read: Project = project.read_as().unwrap();
    let ids: Vec<_> = read.todos.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["todo-2", "todo-0", "todo-1"]);
    assert_eq!(project.todos().keys().unwrap(), ids);
    assert!(project.todos().by_key("todo-1").unwrap().is_some());
    assert_eq!(project.todos().len().unwrap(), 3);

    assert_eq!(ops(&doc, || project.update_from(&read).unwrap()), 1);
    assert_eq!(order.len(), 3);
    project.todos().move_to("todo-1", 0).unwrap();
    assert_eq!(
        project.todos().keys().unwrap(),
        ["todo-1", "todo-2", "todo-0"]
    );
}

/// Reconciling a DTO read before an import reverts the imported change;
/// `edit` reads the current state first.
#[test]
fn stale_dto_reverts_remote_edits_but_edit_does_not() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let pa = ProjectModel::attach(a.get_map("project"));
    pa.update_from(&initial()).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    sync(&a, &b);
    let pb = ProjectModel::attach(b.get_map("project"));

    let mut stale: Project = pa.read_as().unwrap();
    pb.title().set("remote title").unwrap();
    sync(&b, &a);

    pa.edit(|p: &mut Project| p.todos[0].done = true).unwrap();
    assert_eq!(pa.title().get().unwrap(), "remote title");

    stale.todos[0].done = true;
    pa.update_from(&stale).unwrap();
    assert_eq!(pa.title().get().unwrap(), "Issue 888 design");
}
