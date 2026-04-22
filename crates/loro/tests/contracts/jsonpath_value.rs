#![cfg(feature = "jsonpath")]

use loro::{
    ContainerTrait, ExportMode, Index, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroText,
    ToJson, TreeParentId, ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn results_json(results: &[ValueOrContainer]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect(),
    )
}

fn build_doc() -> anyhow::Result<LoroDoc> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;

    let workspace = doc.get_map("workspace");
    workspace.insert("title", "Spec")?;

    let body = workspace.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello world")?;

    let tasks = workspace.insert_container("tasks", LoroList::new())?;
    let first_task = tasks.insert_container(0, LoroMap::new())?;
    first_task.insert("title", "draft")?;
    first_task.insert("done", false)?;
    tasks.insert(1, "loose note")?;

    let order = workspace.insert_container("order", LoroMovableList::new())?;
    order.push("todo")?;
    order.push("doing")?;
    order.push("done")?;
    order.mov(2, 1)?;

    let outline = doc.get_tree("outline");
    outline.enable_fractional_index(0);
    let root = outline.create(TreeParentId::Root)?;
    let child = outline.create_at(root, 0)?;
    outline.get_meta(root)?.insert("title", "Root")?;
    outline.get_meta(child)?.insert("title", "Child")?;

    doc.commit();
    Ok(doc)
}

#[test]
fn jsonpath_returns_container_and_value_nodes_for_nested_state() -> anyhow::Result<()> {
    let doc = build_doc()?;
    let expected = deep_json(&doc);

    let body = doc.jsonpath("$.workspace.body")?;
    assert_eq!(body.len(), 1);
    let body_container = body[0]
        .as_container()
        .expect("text container should be returned as a container");
    assert_eq!(
        body[0].get_deep_value().to_json_value(),
        json!("Hello world")
    );
    assert_eq!(
        doc.get_path_to_container(&body_container.id())
            .expect("body container should be attached")
            .last()
            .map(|(_, index)| index),
        Some(&Index::Key("body".into()))
    );

    let task = doc.jsonpath("$.workspace.tasks[0]")?;
    assert_eq!(task.len(), 1);
    let task_container = task[0]
        .as_container()
        .expect("nested map should be returned as a container");
    assert_eq!(
        task[0].get_deep_value().to_json_value(),
        json!({"done": false, "title": "draft"})
    );
    assert_eq!(
        doc.get_path_to_container(&task_container.id())
            .expect("task container should be attached")
            .last()
            .map(|(_, index)| index),
        Some(&Index::Seq(0))
    );

    let task_title = doc.jsonpath("$.workspace.tasks[0].title")?;
    assert_eq!(results_json(&task_title), json!(["draft"]));
    assert!(task_title[0].as_value().is_some());

    let movable = doc.jsonpath("$.workspace.order[1]")?;
    assert_eq!(results_json(&movable), json!(["done"]));
    assert!(movable[0].as_value().is_some());

    assert_eq!(
        doc.get_by_path(&[Index::Key("workspace".into()), Index::Key("body".into()),])
            .expect("body should resolve by path")
            .get_deep_value()
            .to_json_value(),
        json!("Hello world")
    );
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("workspace".into()),
            Index::Key("tasks".into()),
            Index::Seq(0),
            Index::Key("title".into()),
        ])
        .expect("nested task title should resolve by path")
        .get_deep_value()
        .to_json_value(),
        json!("draft")
    );
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Key("title".into()),
        ])
        .expect("tree root title should resolve by path")
        .get_deep_value()
        .to_json_value(),
        json!("Root")
    );
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Seq(0),
            Index::Key("title".into()),
        ])
        .expect("tree child title should resolve by path")
        .get_deep_value()
        .to_json_value(),
        json!("Child")
    );

    let snapshot = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(deep_json(&snapshot), expected);
    assert_eq!(
        results_json(&snapshot.jsonpath("$.workspace.tasks[0].title")?),
        json!(["draft"])
    );
    assert_eq!(
        results_json(&snapshot.jsonpath("$.workspace.order[1]")?),
        json!(["done"])
    );
    assert!(snapshot
        .jsonpath("$.workspace.body")?
        .first()
        .and_then(|x| x.as_container())
        .is_some());

    let replica = LoroDoc::new();
    replica.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(deep_json(&replica), expected);
    assert_eq!(
        results_json(&replica.jsonpath("$.workspace.body")?),
        json!(["Hello world"])
    );
    assert_eq!(
        results_json(&replica.jsonpath("$.workspace.tasks[0].title")?),
        json!(["draft"])
    );
    assert_eq!(
        replica
            .get_by_path(&[
                Index::Key("workspace".into()),
                Index::Key("tasks".into()),
                Index::Seq(0),
                Index::Key("done".into()),
            ])
            .expect("task completion should survive import")
            .get_deep_value()
            .to_json_value(),
        json!(false)
    );

    Ok(())
}

#[test]
fn path_queries_keep_tree_and_nested_container_contracts_after_roundtrip() -> anyhow::Result<()> {
    let doc = build_doc()?;
    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;

    let restored_body = restored.jsonpath("$.workspace.body")?;
    let restored_task = restored.jsonpath("$.workspace.tasks[0]")?;

    assert_eq!(
        restored_body[0].get_deep_value().to_json_value(),
        json!("Hello world")
    );
    assert_eq!(
        restored_task[0].get_deep_value().to_json_value(),
        json!({"done": false, "title": "draft"})
    );

    let restored_body_path = restored
        .get_path_to_container(&restored_body[0].as_container().unwrap().id())
        .expect("restored body should still be attached");
    assert_eq!(
        restored_body_path.last().map(|(_, index)| index),
        Some(&Index::Key("body".into()))
    );

    let restored_task_path = restored
        .get_path_to_container(&restored_task[0].as_container().unwrap().id())
        .expect("restored task should still be attached");
    assert_eq!(
        restored_task_path.last().map(|(_, index)| index),
        Some(&Index::Seq(0))
    );

    let tree_root_title = restored
        .get_by_path(&[
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Key("title".into()),
        ])
        .expect("tree root title should resolve after snapshot");
    assert_eq!(
        tree_root_title.get_deep_value().to_json_value(),
        json!("Root")
    );

    let tree_child_title = restored
        .get_by_path(&[
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Seq(0),
            Index::Key("title".into()),
        ])
        .expect("tree child title should resolve after snapshot");
    assert_eq!(
        tree_child_title.get_deep_value().to_json_value(),
        json!("Child")
    );

    Ok(())
}
