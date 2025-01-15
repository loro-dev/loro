#![allow(unexpected_cfgs)]
use fuzz::{kv_minify_simple, test_mem_kv_fuzzer, KVAction::*};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn add_same_key_twice() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![254],
        },
        Flush,
        Add {
            key: vec![],
            value: vec![],
        },
    ])
}

#[test]
fn add_and_remove() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![238],
        },
        Remove(0),
    ])
}

#[test]
fn add_flush_remove() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![],
        },
        Flush,
        Remove(3791655167),
    ])
}

#[test]
fn export_and_import() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![],
        },
        ExportAndImport,
    ])
}

#[test]
fn add_flush_add_scan() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![],
        },
        Flush,
        Add {
            key: vec![128],
            value: vec![252, 169],
        },
        Scan {
            start: 12249507989402000797,
            end: 18231419743747221929,
            start_include: true,
            end_include: true,
        },
    ])
}

#[test]
fn add_some() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![255, 255, 255, 255, 63],
            value: vec![],
        },
        Add {
            key: vec![255, 3],
            value: vec![255],
        },
        Add {
            key: vec![255],
            value: vec![],
        },
        Add {
            key: vec![],
            value: vec![],
        },
        Flush,
        Scan {
            start: 18446744073709551615,
            end: 18446744073709551615,
            start_include: true,
            end_include: true,
        },
    ])
}

#[test]
fn merge_import() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![205, 197, 255, 12],
            value: vec![0, 0, 9],
        },
        Add {
            key: vec![],
            value: vec![],
        },
        Flush,
        Add {
            key: vec![57],
            value: vec![209, 3, 255, 174, 0, 255],
        },
        Add {
            key: vec![41],
            value: vec![209, 0, 41, 63, 205],
        },
        Add {
            key: vec![0, 0],
            value: vec![1],
        },
        ExportAndImport,
        Flush,
        ExportAndImport,
        Remove(14829789716734785489),
        Remove(13191005920967349589),
        ExportAndImport,
        ExportAndImport,
        Get(13238251090391746632),
        Add {
            key: vec![],
            value: vec![],
        },
    ])
}

#[test]
fn scan_empty() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![0, 255],
            value: vec![],
        },
        Add {
            key: vec![],
            value: vec![],
        },
        Scan {
            start: 129,
            end: 0,
            start_include: false,
            end_include: false,
        },
    ])
}
#[test]
fn minify() {
    kv_minify_simple(test_mem_kv_fuzzer, vec![])
}
