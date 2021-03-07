pub mod generator;
pub mod parser;

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

        if parser::parse_lang(&lang).is_err() {
            // parse only files for which the name matches a language
            // example: wa_BE@euro
            continue;
        }

        let path = entry.path();
        if let Ok(input) = std::fs::read_to_string(&path) {
            eprintln!("{}", path.display());
            let objects = parser::parse(&input)?;
            locales.insert(lang.to_string(), objects);
        }
    }

    let lib_file = metadata.workspace_root.join("src").join("lib.rs");

    if matches!(env::var("CHECK"), Ok(_)) {
        eprintln!("Calculating checksum...");
        let mut f = Sha256::default();

        write!(f, "{}", generator::CodeGenerator(locales))?;

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
        write!(f, "{}", generator::CodeGenerator(locales))?;
    }

    Ok(())
}
