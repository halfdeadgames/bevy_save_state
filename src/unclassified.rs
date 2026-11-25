use std::{any::TypeId, collections::BTreeSet};

use bevy::{log::warn, prelude::*};

use crate::{identity::Saved, registration::SavedTypes};

/// Lists component types found on [`Saved`] entities that are neither saved nor marked rebuilt.
///
/// The result is sorted and deduplicated. It is empty when `SaveStatePlugin` is not installed,
/// because nothing is classified then. Disabled entities are included.
pub fn unclassified_component_names(world: &World) -> Vec<String> {
    match world.get_resource::<SavedTypes>() {
        Some(saved_types) => unclassified_names_with(world, saved_types),
        None => Vec::new(),
    }
}

pub(crate) fn unclassified_names_with(world: &World, saved_types: &SavedTypes) -> Vec<String> {
    // No Saved component id means no Saved entity has ever existed, which is a normal empty case.
    let Some(saved_marker_id) = world.components().get_valid_id(TypeId::of::<Saved>()) else {
        return Vec::new();
    };
    let mut names = BTreeSet::new();
    for archetype in world.archetypes().iter() {
        if archetype.is_empty() || !archetype.contains(saved_marker_id) {
            continue;
        }
        for component_id in archetype.iter_components() {
            let Some(info) = world.components().get_info(component_id) else {
                continue;
            };
            let is_classified = info
                .type_id()
                .is_some_and(|type_id| saved_types.classifies_component(type_id));
            if !is_classified {
                names.insert(info.name().to_string());
            }
        }
    }
    names.into_iter().collect()
}

pub(crate) fn warn_about_unclassified_components(world: &World, saved_types: &SavedTypes) {
    let names = unclassified_names_with(world, saved_types);
    if names.is_empty() {
        return;
    }
    warn!(
        "these components on Saved entities are not saved and not marked rebuilt: {}; register \
         them with register_saved_component or register_rebuilt_component",
        names.join(", ")
    );
}
