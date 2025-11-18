use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use wasm2map::WASM;
use wasmparser::{Parser as WasmParser, Payload};

#[derive(Parser)]
#[command(author, version, about = "Utility helpers for loro-wasm artifacts")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a source map for a Wasm binary and patch the module to reference it.
    Sourcemap {
        /// Input Wasm file.
        wasm: PathBuf,
        /// Output source map file path.
        map: PathBuf,
        /// Source map URL to embed inside the Wasm module.
        url: String,
    },
    /// Split DWARF sections into a separate Wasm debug companion file.
    SplitDebug {
        /// Input Wasm file to split.
        input: PathBuf,
        /// Output Wasm file with DWARF removed (often the same as input).
        output: PathBuf,
        /// Path to write the debug companion module.
        debug_output: PathBuf,
        /// Relative URL to the debug companion to embed via `external_debug_info`.
        url: String,
    },
    /// Strip DWARF sections without producing a companion module.
    StripDebug {
        /// Input Wasm file to strip.
        input: PathBuf,
        /// Output Wasm file path.
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Sourcemap { wasm, map, url } => generate_sourcemap(wasm, map, url),
        Command::SplitDebug {
            input,
            output,
            debug_output,
            url,
        } => split_debug(input, output, debug_output, url),
        Command::StripDebug { input, output } => strip_debug(input, output),
    }
}

fn generate_sourcemap(wasm_path: PathBuf, map_path: PathBuf, url: String) -> Result<()> {
    let mut wasm = WASM::load(&wasm_path)
        .with_context(|| format!("failed to load Wasm module at {}", wasm_path.display()))?;

    if let Some(parent) = map_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let map_contents = wasm.map_v3();
    fs::write(&map_path, map_contents)
        .with_context(|| format!("failed to write source map {}", map_path.display()))?;

    wasm.patch(&url).with_context(|| {
        format!(
            "failed to patch sourceMappingURL in {}",
            wasm_path.display()
        )
    })?;

    Ok(())
}

fn split_debug(input: PathBuf, output: PathBuf, debug_output: PathBuf, url: String) -> Result<()> {
    let bytes = fs::read(&input)
        .with_context(|| format!("failed to read Wasm module {}", input.display()))?;
    ensure!(
        bytes.starts_with(&[0x00, 0x61, 0x73, 0x6d]),
        "input {} is not a valid WebAssembly binary",
        input.display()
    );

    let (remove_ranges, debug_ranges) = collect_debug_sections(&bytes)?;
    ensure!(
        !debug_ranges.is_empty(),
        "no DWARF custom sections found in {}; build with debuginfo enabled",
        input.display()
    );

    let mut stripped = strip_custom_sections(&bytes, &remove_ranges)?;

    append_custom_section(&mut stripped, "external_debug_info", url.as_bytes());

    let mut debug =
        Vec::with_capacity(8 + debug_ranges.iter().map(|r| r.end - r.start).sum::<usize>());
    debug.extend_from_slice(&bytes[..8]);
    for range in debug_ranges {
        debug.extend_from_slice(&bytes[range]);
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(&output, &stripped)
        .with_context(|| format!("failed to write stripped Wasm {}", output.display()))?;

    if let Some(parent) = debug_output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create debug artifact directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&debug_output, &debug)
        .with_context(|| format!("failed to write debug Wasm {}", debug_output.display()))?;

    Ok(())
}

fn strip_debug(input: PathBuf, output: PathBuf) -> Result<()> {
    let bytes = fs::read(&input)
        .with_context(|| format!("failed to read Wasm module {}", input.display()))?;
    ensure!(
        bytes.starts_with(&[0x00, 0x61, 0x73, 0x6d]),
        "input {} is not a valid WebAssembly binary",
        input.display()
    );

    let (remove_ranges, _debug_ranges) = collect_debug_sections(&bytes)?;
    if remove_ranges.is_empty() {
        // Nothing to strip; just copy input to output if they differ.
        if input != output {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            fs::write(&output, &bytes)
                .with_context(|| format!("failed to write Wasm {}", output.display()))?;
        }
        return Ok(());
    }

    let stripped = strip_custom_sections(&bytes, &remove_ranges)?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(&output, &stripped)
        .with_context(|| format!("failed to write stripped Wasm {}", output.display()))?;

    Ok(())
}

#[allow(clippy::type_complexity)]
fn collect_debug_sections(bytes: &[u8]) -> Result<(Vec<Range<usize>>, Vec<Range<usize>>)> {
    let mut remove_ranges = Vec::new();
    let mut debug_ranges = Vec::new();

    for payload in WasmParser::new(0).parse_all(bytes) {
        let payload = payload.context("failed to parse Wasm payload")?;
        if let Payload::CustomSection(section) = payload {
            let payload_range = section.range();
            let name = section.name();
            let payload_len = payload_range
                .end
                .checked_sub(payload_range.start)
                .context("custom section range underflow")?;
            // Compute length-in-bytes of the u32 LEB-encoded `size`.
            let mut leb_len = 1usize;
            let mut tmp = payload_len;
            while tmp >= 0x80 {
                leb_len += 1;
                tmp >>= 7;
            }
            let header_len = 1 + leb_len; // section id + size
            let start = payload_range
                .start
                .checked_sub(header_len)
                .context("custom section header underflow")?;
            let full_range = start..payload_range.end;

            let is_debug_like = name.starts_with(".debug") || name == "name";
            if is_debug_like {
                debug_ranges.push(full_range.clone());
                remove_ranges.push(full_range);
            } else if name == "external_debug_info" {
                remove_ranges.push(full_range);
            }
        }
    }

    remove_ranges.sort_by_key(|r| r.start);
    Ok((remove_ranges, debug_ranges))
}

fn strip_custom_sections(bytes: &[u8], remove_ranges: &[Range<usize>]) -> Result<Vec<u8>> {
    let mut stripped = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    for range in remove_ranges {
        ensure!(
            range.start >= cursor,
            "overlapping custom section ranges encountered"
        );
        stripped.extend_from_slice(&bytes[cursor..range.start]);
        cursor = range.end;
    }
    stripped.extend_from_slice(&bytes[cursor..]);
    Ok(stripped)
}

fn append_custom_section(buf: &mut Vec<u8>, name: &str, payload: &[u8]) {
    let mut section_payload = Vec::with_capacity(name.len() + payload.len() + 5);
    write_u32_leb(&mut section_payload, name.len() as u32);
    section_payload.extend_from_slice(name.as_bytes());
    section_payload.extend_from_slice(payload);

    buf.push(0); // custom section id
    write_u32_leb(buf, section_payload.len() as u32);
    buf.extend_from_slice(&section_payload);
}

fn write_u32_leb(buf: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}
