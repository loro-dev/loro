use loro::{LoroDoc, LoroError, LoroText, UpdateOptions, UpdateTimeoutError};

#[test]
fn test_text_update() -> anyhow::Result<()> {
    let (old, new, new1) = (")+++", "%", "");
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old, Default::default()).unwrap();
    assert_eq!(&text.to_string(), old);
    text.update(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_text_update_by_line() -> anyhow::Result<()> {
    let (old, new, new1) = (
        "Hello\nWorld\n",
        "Hello\nLoro\nWorld\n",
        "Hello Loro!\nAwesome World!\n",
    );
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line(old, Default::default()).unwrap();
    assert_eq!(&text.to_string(), old);
    text.update_by_line(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_text_update_empty_to_nonempty() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("", Default::default()).unwrap();
    assert_eq!(&text.to_string(), "");
    text.update("Hello, Loro!", Default::default()).unwrap();
    assert_eq!(&text.to_string(), "Hello, Loro!");
    Ok(())
}

#[test]
fn test_text_update_nonempty_to_empty() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("Initial content", Default::default()).unwrap();
    assert_eq!(&text.to_string(), "Initial content");
    text.update("", Default::default()).unwrap();
    assert_eq!(&text.to_string(), "");
    Ok(())
}

#[test]
fn test_text_update_with_special_characters() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("Special chars: !@#$%^&*()", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Special chars: !@#$%^&*()");
    text.update("New special chars: ñáéíóú", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "New special chars: ñáéíóú");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_empty_lines() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\n\nLine 3\n", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\n\nLine 3\n");
    text.update_by_line("Line 1\nLine 2\n\nLine 4\n", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\nLine 2\n\nLine 4\n");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_different_line_endings() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\nLine 2\r\nLine 3\n", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\nLine 2\r\nLine 3\n");
    text.update_by_line("Line 1\r\nLine 2\nLine 3\r\n", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\r\nLine 2\nLine 3\r\n");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_no_trailing_newline() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\nLine 2\nLine 3", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\nLine 2\nLine 3");
    text.update_by_line("Line 1\nLine 2\nLine 3\nLine 4", Default::default())
        .unwrap();
    assert_eq!(&text.to_string(), "Line 1\nLine 2\nLine 3\nLine 4");
    Ok(())
}

#[test]
fn weird_char() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "\0好").unwrap();
    text.delete(0, 2).unwrap();
    assert_eq!(&text.to_string(), "");
    Ok(())
}

#[test]
fn test_failed_case_0() -> anyhow::Result<()> {
    let input = ["\u{1}", "\0տ", ""];
    let (old, new, new1) = (input[0], input[1], input[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old, Default::default()).unwrap();
    text.update(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_failed_case_1() -> anyhow::Result<()> {
    let input = ["", "\u{1f}", "\u{b8ef8}"];
    let (old, new, new1) = (input[0], input[1], input[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old, Default::default()).unwrap();
    text.update(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_failed_case_2() -> anyhow::Result<()> {
    let input = ["\0\0\t\0\u{1}", "'\u{1}\u{15}", ""];
    let (old, new, new1) = (input[0], input[1], input[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old, Default::default()).unwrap();
    text.update(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

fn assert_update_fails(
    text: &LoroText,
    expected: impl Fn(&LoroError) -> bool,
) -> anyhow::Result<()> {
    let by_char = text.update("abd", Default::default());
    let by_line = text.update_by_line("abd\n", Default::default());
    for result in [by_char, by_line] {
        match result {
            Err(UpdateTimeoutError::Loro(err)) if expected(&err) => {}
            other => panic!("unexpected result {other:?}"),
        }
    }
    Ok(())
}

#[test]
fn update_of_a_deleted_container_returns_the_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let text = map.insert_container("t", LoroText::new())?;
    text.insert(0, "abc")?;
    doc.commit();
    map.delete("t")?;
    doc.commit();

    assert_update_fails(&text, |err| {
        matches!(err, LoroError::ContainerDeleted { .. })
    })?;

    map.insert("k", 1)?;
    doc.commit();
    assert!(map.get("k").is_some());
    Ok(())
}

#[test]
fn update_while_read_only_detached_returns_the_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    text.insert(0, "abc")?;
    doc.commit();
    doc.detach();

    assert_update_fails(&text, |err| {
        matches!(
            err,
            LoroError::AutoCommitNotStarted | LoroError::EditWhenDetached
        )
    })?;

    doc.attach();
    text.update("abd", Default::default())?;
    assert_eq!(text.to_string(), "abd");
    Ok(())
}

/// The edits are all skipped after the first failure, and a timeout does not hide it.
#[test]
fn update_reports_a_failed_edit_even_with_a_timeout_set() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let text = map.insert_container("t", LoroText::new())?;
    text.insert(0, "one two three")?;
    doc.commit();
    map.delete("t")?;
    doc.commit();

    let options = UpdateOptions {
        timeout_ms: Some(1000.0),
        use_refined_diff: true,
    };
    let result = text.update("0 one 2 two 3 three", options);
    assert!(matches!(
        result,
        Err(UpdateTimeoutError::Loro(LoroError::ContainerDeleted { .. }))
    ));
    Ok(())
}

#[test]
fn update_to_the_same_text_of_a_deleted_container_edits_nothing() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let text = map.insert_container("t", LoroText::new())?;
    text.insert(0, "abc")?;
    doc.commit();
    map.delete("t")?;
    doc.commit();

    let vv = doc.oplog_vv();
    text.update("abc", Default::default())?;
    doc.commit();
    assert_eq!(doc.oplog_vv(), vv);
    Ok(())
}
