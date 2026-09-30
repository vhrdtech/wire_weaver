use crate::device::{CONFIG_FILE, DEVICE_TABLE, KEYS, Selection, Source, env_var};
use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Table, value};

#[derive(Subcommand)]
pub(crate) enum ConfigCommand {
    /// Show resolved device selection and where each setting comes from
    Show,

    /// Save device selection flags given on the command line into ww.toml
    ///
    /// Saves into --config file if given, otherwise into ww.toml found in the current directory or its closest
    /// parent, otherwise creates ww.toml in the current directory. Environment variables are not saved.
    ///
    /// Example: ww config save --api blinky_api@^0.1 --serial 1234
    Save,

    /// Remove settings from ww.toml
    Unset {
        /// Settings to remove
        #[arg(required = true, value_parser = clap::builder::PossibleValuesParser::new(KEYS))]
        keys: Vec<String>,
    },
}

pub(crate) fn config(cmd: ConfigCommand, selection: &Selection) -> Result<()> {
    match cmd {
        ConfigCommand::Show => show(selection),
        ConfigCommand::Save => save(selection),
        ConfigCommand::Unset { keys } => unset(selection, &keys),
    }
}

fn show(selection: &Selection) -> Result<()> {
    match (&selection.file, &selection.explicit_file) {
        (Some(path), _) => println!("Config file: {}", path.display()),
        (None, Some(path)) => println!("Config file: {} (not found)", path.display()),
        (None, None) => println!("Config file: none"),
    }
    println!();
    let width = KEYS.iter().map(|k| k.len()).max().unwrap_or(0);
    for key in KEYS {
        match selection.settings.iter().find(|s| s.key == key) {
            Some(s) => println!("{key:width$}  {}  ({})", s.value, s.source.describe(key)),
            None => println!(
                "{key:width$}  -  (--{} / {})",
                key.replace('_', "-"),
                env_var(key)
            ),
        }
    }
    Ok(())
}

fn save(selection: &Selection) -> Result<()> {
    let flags: Vec<_> = selection
        .settings
        .iter()
        .filter(|s| matches!(s.source, Source::Flag))
        .collect();
    if flags.is_empty() {
        bail!("nothing to save, pass device selection flags, e.g. 'ww config save --serial 1234'");
    }
    let path = target_file(selection);
    let mut doc = read_doc(&path)?;
    let table = device_table(&mut doc, &path)?;
    for s in flags {
        table[s.key] = if s.key == "timeout_ms" {
            value(s.value.parse::<i64>()?)
        } else {
            value(s.value.as_str())
        };
    }
    write_doc(&path, &doc)?;
    println!("Saved to {}", path.display());
    Ok(())
}

fn unset(selection: &Selection, keys: &[String]) -> Result<()> {
    let path = selection
        .file
        .clone()
        .ok_or(anyhow!("no {CONFIG_FILE} found"))?;
    let mut doc = read_doc(&path)?;
    let table = device_table(&mut doc, &path)?;
    for key in keys {
        table.remove(key);
    }
    write_doc(&path, &doc)?;
    println!("Updated {}", path.display());
    Ok(())
}

fn target_file(selection: &Selection) -> PathBuf {
    selection
        .explicit_file
        .clone()
        .or(selection.file.clone())
        .unwrap_or(PathBuf::from(CONFIG_FILE))
}

fn read_doc(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let contents = std::fs::read_to_string(path).context(format!("reading {}", path.display()))?;
    contents
        .parse()
        .context(format!("parsing {}", path.display()))
}

fn device_table<'a>(doc: &'a mut DocumentMut, path: &Path) -> Result<&'a mut Table> {
    doc.entry(DEVICE_TABLE)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or(anyhow!(
            "{}: '{DEVICE_TABLE}' must be a table",
            path.display()
        ))
}

fn write_doc(path: &Path, doc: &DocumentMut) -> Result<()> {
    std::fs::write(path, doc.to_string()).context(format!("writing {}", path.display()))
}
