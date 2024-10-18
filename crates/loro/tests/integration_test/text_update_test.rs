use loro::LoroDoc;

#[test]
fn test_text_update() -> anyhow::Result<()> {
    let (old, new, new1) = (")+++", "%", "");
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old);
    assert_eq!(&text.to_string(), old);
    text.update(new);
    assert_eq!(&text.to_string(), new);
    text.update(new1);
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
    text.update_by_line(old);
    assert_eq!(&text.to_string(), old);
    text.update_by_line(new);
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1);
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_text_update_empty_to_nonempty() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("");
    assert_eq!(&text.to_string(), "");
    text.update("Hello, Loro!");
    assert_eq!(&text.to_string(), "Hello, Loro!");
    Ok(())
}

#[test]
fn test_text_update_nonempty_to_empty() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("Initial content");
    assert_eq!(&text.to_string(), "Initial content");
    text.update("");
    assert_eq!(&text.to_string(), "");
    Ok(())
}

#[test]
fn test_text_update_with_special_characters() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update("Special chars: !@#$%^&*()");
    assert_eq!(&text.to_string(), "Special chars: !@#$%^&*()");
    text.update("New special chars: ñáéíóú");
    assert_eq!(&text.to_string(), "New special chars: ñáéíóú");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_empty_lines() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\n\nLine 3\n");
    assert_eq!(&text.to_string(), "Line 1\n\nLine 3\n");
    text.update_by_line("Line 1\nLine 2\n\nLine 4\n");
    assert_eq!(&text.to_string(), "Line 1\nLine 2\n\nLine 4\n");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_different_line_endings() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\nLine 2\r\nLine 3\n");
    assert_eq!(&text.to_string(), "Line 1\nLine 2\r\nLine 3\n");
    text.update_by_line("Line 1\r\nLine 2\nLine 3\r\n");
    assert_eq!(&text.to_string(), "Line 1\r\nLine 2\nLine 3\r\n");
    Ok(())
}

#[test]
fn test_text_update_by_line_with_no_trailing_newline() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update_by_line("Line 1\nLine 2\nLine 3");
    assert_eq!(&text.to_string(), "Line 1\nLine 2\nLine 3");
    text.update_by_line("Line 1\nLine 2\nLine 3\nLine 4");
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
    text.update(old);
    text.update(new);
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1);
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_failed_case_1() -> anyhow::Result<()> {
    let input = ["", "\u{1f}", "\u{b8ef8}"];
    let (old, new, new1) = (input[0], input[1], input[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old);
    text.update(new);
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1);
    assert_eq!(&text.to_string(), new1);
    Ok(())
}

#[test]
fn test_failed_case_2() -> anyhow::Result<()> {
    let input = ["\0\0\t\0\u{1}", "'\u{1}\u{15}", ""];
    let (old, new, new1) = (input[0], input[1], input[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old);
    text.update(new);
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1);
    assert_eq!(&text.to_string(), new1);
    Ok(())
}
