use rustc_hash::FxHashSet;
use loro::ContainerType;

#[test]
fn test_fxhashset_order() {
    let mut set = FxHashSet::default();
    set.insert(ContainerType::Map);
    set.insert(ContainerType::List);
    set.insert(ContainerType::Text);
    set.insert(ContainerType::Tree);
    set.insert(ContainerType::MovableList);
    set.insert(ContainerType::Counter);
    let v: Vec<_> = set.iter().copied().collect();
    for (i, ty) in v.iter().enumerate() {
        eprintln!("{}: {:?}", i, ty);
    }
    // target=1 corresponds to:
    eprintln!("target=1 -> {:?}", v[1]);
}
