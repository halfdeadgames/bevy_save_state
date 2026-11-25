use std::path::PathBuf;

use bevy::prelude::*;

use crate::snapshot::{load_world_from_file, save_world_to_file};

/// One-shot exclusive system that saves the world to the given file.
///
/// Shaped for an external input driver's state handler: pass it as the save handler.
///
/// # Errors
/// Returns the failure message of [`save_world_to_file`].
pub fn save_state_system(In(path): In<PathBuf>, world: &mut World) -> Result<(), String> {
    save_world_to_file(world, &path).map_err(|error| error.to_string())
}

/// One-shot exclusive system that loads the world from the given file.
///
/// Shaped for an external input driver's state handler: pass it as the load handler.
///
/// # Errors
/// Returns the failure message of [`load_world_from_file`].
pub fn load_state_system(In(path): In<PathBuf>, world: &mut World) -> Result<(), String> {
    load_world_from_file(world, &path).map_err(|error| error.to_string())
}
