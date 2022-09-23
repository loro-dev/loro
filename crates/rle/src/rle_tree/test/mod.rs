mod notify_prop_test;
mod range_rle_test;
mod string_prop_test;
use ctor::ctor;

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
