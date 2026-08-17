//! Diagnostic entry point for the typed install/package surface inventory.

#[path = "../tasks/install_surface_registry.rs"]
mod install_surface_registry;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    install_surface_registry::run_cli()
}
