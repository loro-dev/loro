use loro_common::LoroResult;
use loro_internal::{loro::ExportMode, LoroDoc};
use rand::{thread_rng, Rng};

fn empty_state() -> Vec<u8> {
    let doc = LoroDoc::new_auto_commit();
    doc.export(ExportMode::StateOnly(None))
}

fn create_list_elem_sequentially(n: usize) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new_auto_commit();
    let list = doc.get_list("list");
    for i in 0..n {
        list.push(i as u32)?;
    }
    let bytes = doc.export(ExportMode::StateOnly(None));
    Ok(bytes)
}

fn create_list_elem_randomly(n: usize) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new_auto_commit();
    let mut rng = thread_rng();
    let list = doc.get_list("list");
    for _ in 0..n {
        let i = rng.gen::<usize>() % (list.len() + 1);
        list.insert(i, i as u32)?;
    }
    let bytes = doc.export(ExportMode::StateOnly(None));
    Ok(bytes)
}

fn main() -> LoroResult<()> {
    for (title, bytes) in [
        "empty",
        "10 items",
        "1000 items sequentially",
        "1000 items randomly",
    ]
    .iter()
    .zip(
        [
            empty_state(),
            create_list_elem_sequentially(10)?,
            create_list_elem_sequentially(1000)?,
            create_list_elem_randomly(1000)?,
        ]
        .iter(),
    ) {
        println!("{}: {} bytes", title, bytes.len())
    }
    Ok(())
}
