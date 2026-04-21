use loro_internal::{
    handler::{Handler, ListHandler, MapHandler, MovableListHandler, TextHandler, TreeHandler},
    HandlerTrait, LoroDoc, ToJson, TreeID, TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn detached_handlers_expose_local_values_ids_and_clear_contracts() -> loro_internal::LoroResult<()>
{
    let map = MapHandler::new_detached();
    map.insert("title", "draft")?;
    let text = map.insert_container("body", TextHandler::new_detached())?;
    text.insert_unicode(0, "hello")?;

    let list = ListHandler::new_detached();
    list.insert(0, "a")?;
    list.insert_container(1, MapHandler::new_detached())?
        .insert("kind", "nested")?;

    let movable = MovableListHandler::new_detached();
    movable.insert(0, "first")?;
    movable.insert(1, "second")?;
    movable.mov(1, 0)?;

    let tree = TreeHandler::new_detached();
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("name", "root")?;

    assert_eq!(format!("{map:?}"), "MapHandler Detached");
    assert_eq!(format!("{list:?}"), "ListHandler Detached");
    assert_eq!(
        format!("{movable:?}"),
        format!("MovableListHandler {}", movable.id())
    );
    assert_eq!(format!("{text:?}"), "TextHandler(Unattached)");
    assert_eq!(format!("{tree:?}"), "TreeHandler Detached");

    assert!(!map.is_attached());
    assert!(!list.is_attached());
    assert!(!movable.is_attached());
    assert!(!text.is_attached());
    assert!(!tree.is_attached());
    assert!(map.attached_handler().is_none());
    assert!(list.attached_handler().is_none());
    assert!(movable.attached_handler().is_none());
    assert!(text.attached_handler().is_none());
    assert!(tree.attached_handler().is_none());
    assert!(map.doc().is_none());
    assert!(list.doc().is_none());
    assert!(movable.doc().is_none());
    assert!(text.doc().is_none());
    assert!(tree.doc().is_none());
    assert!(map.parent().is_none());
    assert!(list.parent().is_none());
    assert!(movable.parent().is_none());
    assert!(text.parent().is_none());
    assert!(tree.parent().is_none());

    assert_eq!(
        map.get_deep_value().to_json_value(),
        json!({"body": "hello", "title": "draft"})
    );
    assert_eq!(
        list.get_deep_value().to_json_value(),
        json!(["a", {"kind": "nested"}])
    );
    assert_eq!(
        movable.get_deep_value().to_json_value(),
        json!(["second", "first"])
    );
    assert_eq!(text.get_deep_value().to_json_value(), json!("hello"));
    assert_eq!(
        tree.get_deep_value().to_json_value()[0]["meta"],
        json!({"name": "root"})
    );

    for handler in [
        Handler::Map(map.clone()),
        Handler::List(list.clone()),
        Handler::MovableList(movable.clone()),
        Handler::Text(text.clone()),
        Handler::Tree(tree.clone()),
    ] {
        assert!(!handler.is_attached());
        assert!(handler.attached_handler().is_none());
        assert!(handler.doc().is_none());
        assert!(handler.get_attached().is_none());
        assert_eq!(handler.id().container_type(), handler.kind());
        assert_eq!(handler.c_type(), handler.kind());
        assert_eq!(handler.to_handler().kind(), handler.kind());
        assert!(!handler.get_value().is_null());
    }

    Handler::Text(text).clear()?;
    Handler::List(list).clear()?;
    Handler::MovableList(movable).clear()?;
    Handler::Tree(tree).clear()?;
    Handler::Map(map).clear()?;

    Ok(())
}

#[test]
fn attached_handlers_report_parent_types_and_roundtrip_through_handler_enum(
) -> loro_internal::LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(66)?;

    let root = doc.get_map("root");
    root.insert("title", "draft")?;
    let text = root.insert_container("text", TextHandler::new_detached())?;
    text.insert_unicode(0, "hello")?;

    let list = root.insert_container("list", ListHandler::new_detached())?;
    let map_under_list = list.insert_container(0, MapHandler::new_detached())?;
    map_under_list.insert("kind", "list-child")?;

    let movable = root.insert_container("movable", MovableListHandler::new_detached())?;
    let text_under_movable = movable.insert_container(0, TextHandler::new_detached())?;
    text_under_movable.insert_unicode(0, "move-child")?;

    let tree = root.insert_container("tree", TreeHandler::new_detached())?;
    let node = tree.create(TreeParentId::Root)?;
    let meta = tree.get_meta(node)?;
    meta.insert("kind", "tree-meta")?;

    doc.commit_then_renew();

    assert!(matches!(text.parent(), Some(Handler::Map(_))));
    assert!(matches!(list.parent(), Some(Handler::Map(_))));
    assert!(matches!(map_under_list.parent(), Some(Handler::List(_))));
    assert!(matches!(
        text_under_movable.parent(),
        Some(Handler::MovableList(_))
    ));
    assert!(matches!(tree.parent(), Some(Handler::Map(_))));
    assert!(matches!(meta.parent(), Some(Handler::Tree(_))));

    assert!(<TextHandler as HandlerTrait>::from_handler(text.to_handler()).is_some());
    assert!(<MapHandler as HandlerTrait>::from_handler(map_under_list.to_handler()).is_some());
    assert!(<ListHandler as HandlerTrait>::from_handler(list.to_handler()).is_some());
    assert!(<MovableListHandler as HandlerTrait>::from_handler(movable.to_handler()).is_some());
    assert!(<TreeHandler as HandlerTrait>::from_handler(tree.to_handler()).is_some());
    assert!(<TreeHandler as HandlerTrait>::from_handler(root.to_handler()).is_none());

    for handler in [
        root.to_handler(),
        text.to_handler(),
        list.to_handler(),
        map_under_list.to_handler(),
        movable.to_handler(),
        text_under_movable.to_handler(),
        tree.to_handler(),
        meta.to_handler(),
    ] {
        assert!(handler.is_attached());
        assert!(handler.attached_handler().is_some());
        assert!(handler.doc().is_some());
        assert!(handler.get_attached().is_some());
        assert_eq!(handler.to_handler().id(), handler.id());
        assert_eq!(handler.c_type(), handler.kind());
        assert!(!handler.get_value().is_null());
        assert!(!handler.get_deep_value().is_null());
    }

    Handler::Text(text).clear()?;
    Handler::List(list).clear()?;
    Handler::MovableList(movable).clear()?;
    Handler::Tree(tree).clear()?;
    Handler::Map(root).clear()?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"root": {}}));

    assert!(doc
        .get_handler(TreeID::new(123, 0).associated_meta_container())
        .is_none());

    Ok(())
}
