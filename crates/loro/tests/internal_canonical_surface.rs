use loro::internal::{Handler, LoroDoc, MapHandler, TextHandler, ValueOrHandler};

#[test]
fn canonical_internal_handler_surface_covers_read_paths() {
    let doc = LoroDoc::new_auto_commit();
    let list = doc.get_list("list");
    let nested = list
        .insert_container(0, MapHandler::new_detached())
        .unwrap();
    nested.insert("flag", true).unwrap();
    let text = nested
        .insert_container("text", TextHandler::new_detached())
        .unwrap();
    text.insert_unicode(0, "hello").unwrap();

    assert!(matches!(
        list.get_(0),
        Some(ValueOrHandler::Handler(Handler::Map(_)))
    ));
    assert!(matches!(
        nested.get_("text"),
        Some(ValueOrHandler::Handler(Handler::Text(_)))
    ));
    assert!(matches!(
        doc.get_by_str_path("list/0/text"),
        Some(ValueOrHandler::Handler(Handler::Text(_)))
    ));

    let mut iterated = Vec::new();
    nested.for_each(|_, value| iterated.push(value));
    assert!(iterated
        .iter()
        .any(|value| matches!(value, ValueOrHandler::Handler(Handler::Text(_)))));

    let values: Vec<_> = nested.values().collect();
    assert!(values
        .iter()
        .any(|value| matches!(value, ValueOrHandler::Handler(Handler::Text(_)))));
}
