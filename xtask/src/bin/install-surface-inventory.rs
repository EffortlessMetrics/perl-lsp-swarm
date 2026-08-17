//! Diagnostic entry point for the typed install/package surface inventory.

#[path = "../tasks/install_surface_inventory.rs"]
mod install_surface_inventory;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    install_surface_inventory::run_cli()
}
