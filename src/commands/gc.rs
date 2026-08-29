use std::fs;

use anyhow::Result;

use crate::{cache, cli::GcArgs};

pub fn run(args: GcArgs) -> Result<()> {
    let mut registry = cache::read_registry()?;
    let mut kept = Vec::new();
    let mut removed = 0usize;
    for record in registry.records.drain(..) {
        if !record.path.exists() {
            continue;
        }
        if record.workspace.exists() {
            kept.push(record);
            continue;
        }
        println!(
            "cargo-warm: {} orphaned {:?} {}",
            if args.dry_run {
                "would remove"
            } else {
                "removing"
            },
            record.kind,
            record.path.display()
        );
        if args.dry_run {
            kept.push(record);
        } else {
            fs::remove_dir_all(&record.path)?;
            removed += 1;
        }
    }
    registry.records = kept;
    cache::write_registry(&registry)?;
    if !args.dry_run {
        println!("cargo-warm: removed {removed} orphaned cache root(s)");
    }
    Ok(())
}
