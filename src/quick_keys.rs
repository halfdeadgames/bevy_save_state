use std::path::{Path, PathBuf};

use bevy::{
    log::{error, info},
    prelude::*,
};

use crate::snapshot::{load_world_from_file, save_world_to_file};

/// Name of the savestate file inside the quick-save folder.
pub const QUICK_SAVE_FILE_NAME: &str = "quick.ron";

/// Keys and folder used by [`QuickSaveKeysPlugin`].
#[derive(Resource, Clone, Debug)]
pub struct QuickSaveKeyBindings {
    /// Folder that holds the quick-save file. Created on the first save.
    pub folder: PathBuf,
    /// Pressing this key saves.
    pub save_key: KeyCode,
    /// Pressing this key loads.
    pub load_key: KeyCode,
}

impl QuickSaveKeyBindings {
    fn quick_save_path(&self) -> PathBuf {
        self.folder.join(QUICK_SAVE_FILE_NAME)
    }
}

/// Saves on one key and loads on another, for fast manual testing.
///
/// Defaults to folder `savestates`, F5 to save, and F8 to load. Results are logged and failures
/// never panic. A key press queues the save or load, which runs at the next command sync point
/// in the same frame, not at a time of your choosing.
#[derive(Clone, Debug)]
pub struct QuickSaveKeysPlugin {
    /// Folder that holds `quick.ron`.
    pub folder: PathBuf,
    /// Pressing this key saves.
    pub save_key: KeyCode,
    /// Pressing this key loads.
    pub load_key: KeyCode,
}

impl Default for QuickSaveKeysPlugin {
    fn default() -> Self {
        Self {
            folder: PathBuf::from("savestates"),
            save_key: KeyCode::F5,
            load_key: KeyCode::F8,
        }
    }
}

impl Plugin for QuickSaveKeysPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(QuickSaveKeyBindings {
            folder: self.folder.clone(),
            save_key: self.save_key,
            load_key: self.load_key,
        })
        .add_systems(Update, queue_quick_save_or_load_on_key_press);
    }
}

fn queue_quick_save_or_load_on_key_press(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    bindings: Res<QuickSaveKeyBindings>,
    mut commands: Commands,
) {
    // Headless apps have no keyboard input resource; for them there is nothing to react to.
    let Some(keys) = keys else {
        return;
    };
    if keys.just_pressed(bindings.save_key) {
        let path = bindings.quick_save_path();
        commands.queue(move |world: &mut World| quick_save(world, &path));
    }
    if keys.just_pressed(bindings.load_key) {
        let path = bindings.quick_save_path();
        commands.queue(move |world: &mut World| quick_load(world, &path));
    }
}

fn quick_save(world: &mut World, path: &Path) {
    match save_world_to_file(world, path) {
        Ok(()) => info!("quick saved savestate to {}", path.display()),
        Err(failure) => error!("quick save to {} failed: {failure}", path.display()),
    }
}

fn quick_load(world: &mut World, path: &Path) {
    match load_world_from_file(world, path) {
        Ok(()) => info!("quick loaded savestate from {}", path.display()),
        Err(failure) => error!("quick load from {} failed: {failure}", path.display()),
    }
}
