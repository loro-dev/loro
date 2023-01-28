use loro_core::LoroCore;

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
