//! Diagnostic entry point for the no-publish side-effect surface inventory.
//!
//! Validates one `no_publish_side_effects.v1` document against the closed
//! schema and fail-closed topology inventory (#9414). It parses no workflow,
//! observes no endpoint, and mutates no public channel.

#![allow(clippy::print_stdout)]

#[path = "../tasks/no_publish_side_effects.rs"]
mod no_publish_side_effects;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    no_publish_side_effects::run_cli()
}
