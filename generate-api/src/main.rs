pub mod generator;
pub mod parser;

use crate::parser::{Object, Value};
use anyhow::{bail, Result};
use cargo_metadata::MetadataCommand;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::io::{BufWriter, Write};

fn main() -> Result<()> {
    let metadata = MetadataCommand::new().exec()?;

    eprintln!("Reading data...");

    let locales_path = metadata.workspace_root.join("localedata").join("locales");
    let mut locales = HashMap::new();

    for entry in fs::read_dir(locales_path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let lang = file_name.to_str().unwrap();

        if parser::parse_lang(lang).is_err() {
            // parse only files for which the name matches a language
            // example: wa_BE@euro
            continue;
        }

        let path = entry.path();
        if let Ok(input) = std::fs::read_to_string(&path) {
            eprintln!("{}", path.display());
            let mut objects = parser::parse(&input)?;
            validate_and_fix(&mut objects);
            locales.insert(lang.to_string(), objects);
        }
    }

    let lib_file = metadata.workspace_root.join("src").join("lib.rs");

    if env::var("CHECK").is_ok() {
        eprintln!("Calculating checksum...");
        let mut f = Sha256::default();

        write!(f, "{}", generator::CodeGenerator::new(locales))?;

        let expected = f.finalize();
        eprintln!("expected: {:x}", expected);

        let mut hasher = Sha256::default();
        io::copy(&mut fs::File::open(&lib_file)?, &mut hasher)?;
        let got = hasher.finalize();
        eprintln!("got: {:x}", got);

        if expected != got {
            bail!(
                "lib.rs file has been modified! Please run `cargo run -p generate-api --release`",
            );
        }
    } else {
        eprintln!("Writing to file `{}`...", lib_file.display());
        let mut f = BufWriter::new(fs::File::create(&lib_file)?);
        write!(f, "{}", generator::CodeGenerator::new(locales))?;
    }

    Ok(())
}

fn validate_and_fix(objects: &mut [Object]) {
    validate_and_fix_t_fmt_ampm(objects);
    validate_and_fix_d_t_fmt(objects);
}

/// Add a `T_FMT_AMPM` item if it is missing (happens for 3 locales).
///
/// If the locale has non-empty values for `AM_PM` we assume the correct string to be the same as
/// for POSIX: `%l:%M:%S %p`.
/// If the locale has empty values for `AM_PM` we set `T_FMT_AMPM` to an empty value, similar to
/// other locales that don't have a 12-hour clock format.
fn validate_and_fix_t_fmt_ampm(objects: &mut [Object]) {
    for object in objects.iter_mut() {
        if object.name != "LC_TIME" {
            continue;
        }
        let mut found_t_fmt_ampm = false;
        let mut am_pm_empty = false;
        for (key, value) in object.values.iter() {
            match (key.as_str(), value.as_slice()) {
                ("t_fmt_ampm" | "copy" | "insert", _) => found_t_fmt_ampm = true,
                ("am_pm", &[Value::String(ref am), Value::String(ref pm)]) => {
                    am_pm_empty = am.is_empty() && pm.is_empty()
                }
                _ => {}
            }
        }
        if !found_t_fmt_ampm {
            let value = match am_pm_empty {
                true => vec![Value::String(String::new())],
                false => vec![Value::String("%l:%M:%S %p".to_string())],
            };
            object.values.push(("t_fmt_ampm".to_string(), value));
        }
    }
}

/// In some locales `D_T_FMT` refers to other items:
/// to `D_FMT` with `%x`, `T_FMT` with `%X`, and/or `T_FMT_AMPM` with `%r`.
/// Inlining these strings simplifies the implementation of the strftime parser in chrono.
fn validate_and_fix_d_t_fmt(objects: &mut [Object]) {
    for object in objects.iter_mut() {
        if object.name != "LC_TIME" {
            continue;
        }
        let mut d_fmt = String::new();
        let mut t_fmt = String::new();
        let mut t_fmt_ampm = String::new();
        for (key, value) in object.values.iter() {
            match (key.as_str(), value.as_slice()) {
                ("d_fmt", &[Value::String(ref value)]) => d_fmt = value.clone(),
                ("t_fmt", &[Value::String(ref value)]) => t_fmt = value.clone(),
                ("t_fmt_ampm", &[Value::String(ref value)]) => t_fmt_ampm = value.clone(),
                _ => {}
            }
        }
        for (key, ref mut value) in object.values.iter_mut() {
            if let ("d_t_fmt", vec) = (key.as_str(), value) {
                if let Value::String(ref val) = vec[0] {
                    let d_t_fmt = val
                        .replace("%x", &d_fmt)
                        .replace("%X", &t_fmt)
                        .replace("%r", &t_fmt_ampm);
                    vec[0] = Value::String(d_t_fmt);
                }
            }
        }
    }
}
