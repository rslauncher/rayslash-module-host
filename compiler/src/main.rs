use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use wasmtime::{Config, Engine};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    module: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rayslash-module-compiler: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let module = fs::read(&args.module)
        .with_context(|| format!("failed to read {}", args.module.display()))?;
    if module.len() > 64 * 1024 * 1024 {
        bail!("module exceeds the 64 MiB compiler input limit");
    }

    let mut config = Config::new();
    config.wasm_component_model(true).consume_fuel(true);
    let engine = Engine::new(&config)?;
    let compiled = engine
        .precompile_component(&module)
        .map_err(|error| anyhow::anyhow!("failed to precompile module component: {error}"))?;
    atomic_write_private(&args.output, &compiled)
}

fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("compiled output path has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("module"),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
