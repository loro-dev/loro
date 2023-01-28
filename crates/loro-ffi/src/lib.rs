use std::ffi::{c_char, c_uint, CStr, CString};

use loro_internal::{LoroCore, Text};

/// create Loro with a random unique client id
#[no_mangle]
pub extern "C" fn loro_new() -> *mut LoroCore {
    Box::into_raw(Box::new(LoroCore::default()))
}

/// Release all memory of Loro
#[no_mangle]
pub unsafe extern "C" fn loro_free(loro: *mut LoroCore) {
    if !loro.is_null() {
        drop(Box::from_raw(loro));
    }
}

#[no_mangle]
pub unsafe extern "C" fn loro_get_text(loro: *mut LoroCore, id: *const c_char) -> *mut Text {
    assert!(!loro.is_null());
    assert!(!id.is_null());
    let id = CStr::from_ptr(id).to_str().unwrap();
    let text = loro.as_mut().unwrap().get_text(id);
    Box::into_raw(Box::new(text))
}

#[no_mangle]
pub unsafe extern "C" fn text_free(text: *mut Text) {
    if !text.is_null() {
        drop(Box::from_raw(text));
    }
}

#[no_mangle]
pub unsafe extern "C" fn text_insert(
    text: *mut Text,
    ctx: *const LoroCore,
    pos: usize,
    value: *const c_char,
) {
    assert!(!text.is_null());
    assert!(!ctx.is_null());
    let text = text.as_mut().unwrap();
    let ctx = ctx.as_ref().unwrap();
    let value = CStr::from_ptr(value).to_str().unwrap();
    text.insert(ctx, pos, value).unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn text_value(text: *mut Text) -> *mut c_char {
    assert!(!text.is_null());
    let text = text.as_mut().unwrap();
    let value = text.get_value().as_string().unwrap().to_string();
    CString::new(value).unwrap().into_raw()
}
