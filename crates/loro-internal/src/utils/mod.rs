pub(crate) mod kv_wrapper;
pub(crate) mod lazy;
pub(crate) mod query_by_len;
pub mod string_slice;
pub(crate) mod subscription;
pub(crate) mod utf16;

#[cfg(feature = "min")]
pub(crate) fn source_position_info(_file: &'static str, _line: u32) -> Box<str> {
    "Position".into()
}

#[cfg(not(feature = "min"))]
pub(crate) fn source_position_info(file: &'static str, line: u32) -> Box<str> {
    format!("Position: {file}:{line}").into_boxed_str()
}
