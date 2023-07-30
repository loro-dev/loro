#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, CStr, CString};

use loro_internal::{LoroDoc, TextHandler};

/// create Loro with a random unique client id
#[no_mangle]
pub extern "C" fn loro_new() -> *mut LoroDoc {
    Box::into_raw(Box::default())
}

/// Release all memory of Loro
#[no_mangle]
pub unsafe extern "C" fn loro_free(loro: *mut LoroDoc) {
    if !loro.is_null() {
        drop(Box::from_raw(loro));
    }
}

#[no_mangle]
pub unsafe extern "C" fn loro_get_text(loro: *mut LoroDoc, id: *const c_char) -> *mut TextHandler {
    assert!(!loro.is_null());
    assert!(!id.is_null());
    let id = CStr::from_ptr(id).to_str().unwrap();
    let text = loro.as_mut().unwrap().get_text(id);
    Box::into_raw(Box::new(text))
}

#[no_mangle]
pub unsafe extern "C" fn text_free(text: *mut TextHandler) {
    if !text.is_null() {
        drop(Box::from_raw(text));
    }
}

#[no_mangle]
pub unsafe extern "C" fn text_insert(
    text: *mut TextHandler,
    ctx: *const LoroDoc,
    pos: usize,
    value: *const c_char,
) {
    assert!(!text.is_null());
    assert!(!ctx.is_null());
    let text = text.as_mut().unwrap();
    let ctx = ctx.as_ref().unwrap();
    let value = CStr::from_ptr(value).to_str().unwrap();
    let mut txn = ctx.txn().unwrap();
    text.insert(&mut txn, pos, value).unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn text_value(text: *mut TextHandler) -> *mut c_char {
    assert!(!text.is_null());
    let text = text.as_mut().unwrap();
    let value = text.get_value().as_string().unwrap().to_string();
    CString::new(value).unwrap().into_raw()
}
