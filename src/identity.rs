use std::any::TypeId;

use bevy::{ecs::world::DeferredWorld, prelude::*};

/// Marks an entity as game state that savestates capture and restore.
///
/// Adding it gives the entity a [`SaveId`] when it has none. Loading despawns every entity with
/// this marker, including ones hidden by `Disabled` or any other disabling component, together
/// with all of their descendants (whether or not the descendants are `Saved`) before the saved
/// ones are spawned. Non-`Saved` descendants such as visual children are not restored; rebuild
/// them with an observer. Saves and loads queued from keys run at the next command sync point.
#[derive(Component, Reflect, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[reflect(Component, Default)]
pub struct Saved;

/// Marks a non-[`Saved`] child that the game's own observers recreate after a load.
///
/// Saves drop the hierarchy link to every non-`Saved` child. Children with this marker are
/// dropped quietly (a debug log), while any other dropped child is reported with a warning, so a
/// forgotten `Saved` marker stays visible.
#[derive(Component, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebuiltChild;

/// Stable identity of a [`Saved`] entity, used to order entities in the file.
///
/// Bevy hands out new `Entity` ids after every load, so the file is sorted by this id instead.
#[derive(Component, Reflect, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[reflect(Component)]
pub struct SaveId(pub u64);

/// Counter that hands out the next [`SaveId`]. It is saved, so ids stay stable across loads.
#[derive(Resource, Reflect, Default, Clone, Copy, Debug, PartialEq, Eq)]
#[reflect(Resource, Default)]
pub struct NextSaveId(pub u64);

impl NextSaveId {
    fn allocate(&mut self) -> SaveId {
        let allocated = SaveId(self.0);
        self.0 += 1;
        allocated
    }
}

/// Gives every newly marked entity a [`SaveId`] unless it already carries one.
pub(crate) fn assign_save_id_to_new_saved_entity(added: On<Add<Saved>>, mut world: DeferredWorld) {
    // A direct lookup, not a query: queries skip entities hidden by a disabling component, and a
    // hidden entity being loaded must keep the id written from the file.
    if world.entity(added.entity).contains::<SaveId>() {
        return;
    }
    let save_id = world.resource_mut::<NextSaveId>().allocate();
    world.commands().entity(added.entity).insert(save_id);
}

/// Lists every entity that has the marker component, including entities hidden by `Disabled` or
/// by a disabling component the game registered.
///
/// Walks archetypes instead of querying, because every query gets one default filter per
/// registered disabling component, and `Allow` can only lift filters whose types are known here.
pub(crate) fn entities_with_marker<MarkerComponent: Component>(world: &World) -> Vec<Entity> {
    // No component id means no entity has ever had the marker.
    let Some(marker_id) = world
        .components()
        .get_valid_id(TypeId::of::<MarkerComponent>())
    else {
        return Vec::new();
    };
    world
        .archetypes()
        .iter()
        .filter(|archetype| archetype.contains(marker_id))
        .flat_map(|archetype| archetype.entities().iter().map(|entity| entity.id()))
        .collect()
}
