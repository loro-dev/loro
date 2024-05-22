use std::{fs::File, io::Write};

use loro_common::LoroResult;
use loro_internal::{LoroDoc, MapHandler, TextHandler};

fn main() -> LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    let list = doc.get_list("list");
    list.insert(0, "item")?;
    let map = list.insert_container(1, MapHandler::new_detached())?;
    map.insert("key", "value")?;
    let text = map.insert_container("text", TextHandler::new_detached())?;
    text.insert(0, "hello")?;
    text.insert(5, " world")?;
    text.mark(0, 5, "bold", true.into())?;
    let tree = doc.get_tree("tree");
    let root = tree.create(None)?;
    let node = tree.create(None)?;
    tree.mov(node, root)?;
    let movable_list = doc.get_movable_list("movable_list");
    movable_list.insert(0, 1)?;
    movable_list.insert(0, 2)?;
    movable_list.insert(0, 3)?;
    movable_list.mov(2, 0)?;
    let json = doc.export_json(&Default::default());
    let new_doc = LoroDoc::new_auto_commit();
    new_doc.import_json(&json)?;
    File::create("json_schema.json")
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    Ok(())
}
