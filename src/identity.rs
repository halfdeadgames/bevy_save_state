use bevy::{ecs::entity_disabling::Disabled, prelude::*};

/// Marks an entity as game state that savestates capture and restore.
///
/// Adding it gives the entity a [`SaveId`] when it has none. Loading despawns every entity with
/// this marker, including disabled ones, together with all of their descendants (whether or not
/// the descendants are `Saved`) before the saved ones are spawned. Non-`Saved` descendants such as
/// visual children are not restored; rebuild them with an observer. Saves and loads queued from
/// keys run at the next command sync point.
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
pub(crate) fn assign_save_id_to_new_saved_entity(
    added: On<Add<Saved>>,
    // Allow<Disabled> so a hidden entity being loaded keeps the id written from the file.
    entities_with_id: Query<(), (With<SaveId>, Allow<Disabled>)>,
    mut next_id: ResMut<NextSaveId>,
    mut commands: Commands,
) {
    if entities_with_id.contains(added.entity) {
        return;
    }
    commands.entity(added.entity).insert(next_id.allocate());
}
