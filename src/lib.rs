//! Emulator-style savestates for testing and debugging a Bevy game.
//!
//! Mark game entities with [`Saved`], register every game-state type through
//! [`SaveStateAppExt`], and add [`SaveStatePlugin`]. Saving between frames captures the exact game
//! state; loading in the same build puts it back. Files from other builds are refused with a clear
//! [`SaveStateError`]. This is not meant for player save slots.

#![warn(missing_docs)]

mod driver_systems;
mod error;
mod file_format;
mod fingerprint;
mod identity;
mod quick_keys;
mod registration;
mod snapshot;
mod unclassified;

#[cfg(test)]
mod tests;

use bevy::{
    ecs::entity_disabling::Disabled,
    prelude::*,
    transform::components::{GlobalTransform, TransformTreeChanged},
};

pub use driver_systems::{load_state_system, save_state_system};
pub use error::SaveStateError;
pub use file_format::{CURRENT_FORMAT_VERSION, SaveHeader};
pub use identity::{NextSaveId, SaveId, Saved};
pub use quick_keys::{QUICK_SAVE_FILE_NAME, QuickSaveKeyBindings, QuickSaveKeysPlugin};
pub use registration::{SaveStateAppExt, SavedTypes};
pub use snapshot::{
    load_world_from_file, load_world_from_str, save_world_to_file, save_world_to_string,
};
pub use unclassified::unclassified_component_names;

/// Registers the savestate resources, the [`SaveId`] observer, and the built-in saved types.
///
/// Saved by default: [`Saved`], [`SaveId`], [`NextSaveId`], `Name`, `Disabled`, `ChildOf`, and
/// `Children`. `Disabled` is saved so disabled entities come back disabled. `Children` is saved
/// because loading skips relationship hooks, so nothing else would refill it. Only hierarchy
/// between `Saved` entities is kept.
/// Treated as rebuilt by default: `GlobalTransform` and `TransformTreeChanged`.
pub struct SaveStatePlugin;

impl Plugin for SaveStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SavedTypes>()
            .init_resource::<NextSaveId>()
            .add_observer(identity::assign_save_id_to_new_saved_entity)
            .register_saved_component::<Saved>()
            .register_saved_component::<SaveId>()
            .register_saved_resource::<NextSaveId>()
            .register_saved_component::<Name>()
            .register_saved_component::<Disabled>()
            .register_saved_component::<ChildOf>()
            .register_saved_component::<Children>()
            .register_rebuilt_component::<GlobalTransform>()
            .register_rebuilt_component::<TransformTreeChanged>();
    }
}
