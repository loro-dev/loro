use std::path::Path;

static mut GUARD: Option<tracing_chrome::FlushGuard> = None;

pub fn setup_test_log() {
    color_backtrace::install();
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        // suffix should be current date time
        let time_suffix = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();
        // create dir if not exists
        std::fs::create_dir_all("./log").unwrap();
        let (chrome_layer, _guard) = ChromeLayerBuilder::new()
            .include_args(true)
            .include_locations(true)
            .file(Path::new(
                format!("./log/trace-{}.json", time_suffix).as_str(),
            ))
            .build();
        // SAFETY: Test
        unsafe { GUARD = Some(_guard) };
        tracing::subscriber::set_global_default(
            Registry::default()
                .with(
                    tracing_subscriber::fmt::Layer::default()
                        .with_line_number(true)
                        .with_file(true),
                )
                .with(chrome_layer),
        )
        .unwrap();
    }
}
