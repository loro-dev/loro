use loro_common::{LoroResult, LoroValue};
use loro_internal::{loro::ExportMode, LoroDoc};
use rand::{thread_rng, Rng};

#[derive(Debug, Clone, Copy)]
enum Kind {
    Map,
    List,
    MovableList,
    Text,
    Tree,
    Counter,
}

fn empty_state() -> Vec<u8> {
    let doc = LoroDoc::new_auto_commit();
    doc.export(ExportMode::StateOnly(None)).unwrap()
}

fn create_elem_sequentially(n: usize, kind: Kind) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new_auto_commit();
    for i in 0..n {
        match kind {
            Kind::Map => {
                doc.get_map("map").insert(&i.to_string(), i as u32)?;
            }
            Kind::List => {
                doc.get_list("list").push(i as u32)?;
            }
            Kind::MovableList => {
                doc.get_movable_list("movable_list")
                    .push(LoroValue::from(i as u32))?;
            }
            Kind::Text => {
                let text = doc.get_text("text");
                text.insert_unicode(text.len_unicode(), &i.to_string())?;
            }
            Kind::Tree => {
                doc.get_tree("tree").enable_fractional_index(0);
                doc.get_tree("tree")
                    .create(loro_internal::TreeParentId::Root)?;
            }
            Kind::Counter => {
                #[cfg(feature = "counter")]
                {
                    doc.get_counter("counter").increment(i as f64)?;
                }
            }
        }
    }
    let bytes = doc.export(ExportMode::StateOnly(None)).unwrap();
    Ok(bytes)
}

fn create_elem_randomly(n: usize, kind: Kind) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new_auto_commit();
    let mut rng = thread_rng();
    for _ in 0..n / 5 {
        for _ in 0..5 {
            match kind {
                Kind::Map => {
                    let i = rng.gen::<usize>() % 1000;
                    doc.get_map("map").insert(&i.to_string(), i as u32)?;
                }
                Kind::List => {
                    let list = doc.get_list("list");
                    let i = rng.gen::<usize>() % (list.len() + 1);
                    list.insert(i, i as u32)?;
                }
                Kind::MovableList => {
                    let list = doc.get_movable_list("movable_list");
                    let i = rng.gen::<usize>() % (list.len() + 1);
                    list.insert(i, i as u32)?;
                }
                Kind::Text => {
                    let text = doc.get_text("text");
                    let i = rng.gen::<usize>() % (text.len_unicode() + 1);
                    text.insert_unicode(i, &i.to_string())?;
                }
                Kind::Tree => {
                    doc.get_tree("tree").enable_fractional_index(0);
                    doc.get_tree("tree")
                        .create(loro_internal::TreeParentId::Root)?;
                }
                Kind::Counter => {
                    #[cfg(feature = "counter")]
                    {
                        doc.get_counter("counter").increment(rng.gen::<f64>())?;
                    }
                }
            }
        }
        doc.set_peer_id(rng.gen::<u64>()).unwrap();
    }
    let bytes = doc.export(ExportMode::StateOnly(None)).unwrap();
    Ok(bytes)
}

fn main() -> LoroResult<()> {
    for kind in [
        Kind::Map,
        Kind::List,
        Kind::MovableList,
        Kind::Text,
        #[allow(unused)]
        Kind::Tree,
        #[cfg(feature = "counter")]
        Kind::Counter,
    ] {
        println!("=============================");
        println!("{kind:?}");
        println!("=============================");

        for (title, bytes) in [
            ("empty", empty_state()),
            ("10 items", create_elem_sequentially(10, kind)?),
            (
                "1000 items sequentially",
                create_elem_sequentially(1000, kind)?,
            ),
            ("1000 items randomly", create_elem_randomly(1000, kind)?),
        ] {
            println!("{}: {} bytes", title, bytes.len())
        }
    }
    Ok(())
}
