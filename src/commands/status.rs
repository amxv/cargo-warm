use anyhow::Result;

use crate::cache;

pub fn run() -> Result<()> {
    let registry = cache::read_registry()?;
    if registry.records.is_empty() {
        println!("cargo-warm: no recorded seeded caches");
        return Ok(());
    }
    for record in registry.records {
        let state = if !record.path.exists() {
            "missing"
        } else if !record.workspace.exists() {
            "orphaned"
        } else {
            "available"
        };
        println!(
            "{state:9} {:?} {}  workspace={}",
            record.kind,
            record.path.display(),
            record.workspace.display()
        );
    }
    Ok(())
}
