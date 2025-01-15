#![allow(unexpected_cfgs)]
use loro::LoroDoc;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn issue_0() {
    let bytes = include_bytes!("./issue_0.bin");
    let doc = LoroDoc::new();
    doc.import_batch(&[bytes.into()]).unwrap();
    #[allow(deprecated)]
    doc.export_snapshot();
    doc.export(loro::ExportMode::Snapshot).unwrap();
}
