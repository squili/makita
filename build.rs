use serde_json::Value;
use std::env::var;
use std::fs::{read_to_string, write};
use anyhow::Result;

fn main() -> Result<()> {
    // recompile on sql migration changes
    println!("cargo:rerun-if-changed=migrations");
    // recompile on command definition changes
    println!("cargo:rerun-if-changed=commands.json5");

    // convert json5 commands definition to regular json
    let target_file = format!("{}/commands.json", var("OUT_DIR")?);

    println!("cargo:rustc-env=MAKITA_SLASH_LOCATION={}", target_file);

    let source = json5::from_str::<Value>(&*read_to_string("commands.json5")?)?;
    let mut data = Vec::new();

    if let Value::Array(commands) = source {
        for command in commands {
            data.push(serde_json::to_string(&command)?);
        }
    }

    write(target_file, data.join("\n"))?;

    Ok(())
}
