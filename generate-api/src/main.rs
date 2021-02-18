pub mod generator;
pub mod parser;

use anyhow::{bail, Result};
use cargo_metadata::MetadataCommand;
use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};

fn main() -> Result<()> {
    let metadata = MetadataCommand::new().exec()?;

    println!("Reading data...");

    let locales_path = metadata.workspace_root.join("localedata").join("locales");
    let mut locales = HashMap::new();

    for entry in fs::read_dir(locales_path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let lang = file_name.to_str().unwrap();

        if parser::parse_lang(&lang).is_err() {
            // parse only files for which the name matches a language
            // example: wa_BE@euro
            continue;
        }

        let path = entry.path();
        if let Ok(input) = std::fs::read_to_string(&path) {
            println!("{}", path.display());
            let objects = parser::parse(&input)?;
            locales.insert(lang.to_string(), objects);
        }
    }

    let dest_path = metadata.workspace_root.join("src").join("lib.rs");
    let mut f = BufWriter::new(fs::File::create(&dest_path)?);

    println!("Writing to file `{}`...", dest_path.display());

    write!(f, "{}", generator::CodeGenerator(locales))?;

    drop(f);

    let status = std::process::Command::new("cargo")
        .current_dir(metadata.workspace_root)
        .args(&["fmt", "--"])
        .arg(dest_path)
        .status()
        .unwrap();

    if status.success() {
        Ok(())
    } else {
        bail!("command `cargo fmt` failed");
    }
}
