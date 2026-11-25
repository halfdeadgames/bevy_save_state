use std::{
    any::{TypeId, type_name},
    collections::{BTreeMap, BTreeSet},
};

use bevy::{
    ecs::reflect::{ReflectComponent, ReflectResource},
    prelude::*,
    reflect::{FromReflect, GetTypeRegistration, TypePath},
    state::state::FreelyMutableState,
};

use crate::error::SaveStateError;

/// Per-state functions the loader needs, stored as plain function pointers because the state type
/// is erased once it is registered.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SavedStateHooks {
    /// Fails when `NextState<S>` is missing, so a load can be refused before the world changes.
    pub(crate) check_next_state_exists: fn(&World) -> Result<(), SaveStateError>,
    /// Puts a restored state machine back to "no transition pending" after a load.
    pub(crate) reset_pending_transition: fn(&mut World) -> Result<(), SaveStateError>,
}

/// Allow-lists that decide what a savestate contains and what the unclassified check accepts.
///
/// Fill it through [`SaveStateAppExt`] rather than by hand.
#[derive(Resource, Default, Clone, Debug)]
pub struct SavedTypes {
    pub(crate) components: BTreeSet<TypeId>,
    pub(crate) resources: BTreeSet<TypeId>,
    pub(crate) rebuilt_components: BTreeSet<TypeId>,
    pub(crate) saved_states: BTreeMap<TypeId, SavedStateHooks>,
}

impl SavedTypes {
    /// Returns true when the component is either saved or declared as rebuilt after a load.
    pub fn classifies_component(&self, type_id: TypeId) -> bool {
        self.components.contains(&type_id) || self.rebuilt_components.contains(&type_id)
    }
}

/// Registration methods that tell the savestate system which game types matter.
pub trait SaveStateAppExt {
    /// Saves component `T` on every [`Saved`](crate::Saved) entity that has it.
    ///
    /// Registers the type in the type registry and guarantees its `ReflectComponent` data.
    fn register_saved_component<T>(&mut self) -> &mut Self
    where
        T: Component + Reflect + TypePath + GetTypeRegistration;

    /// Saves resource `R` and writes it back on load.
    ///
    /// Registers the type in the type registry and guarantees its `ReflectResource` data.
    fn register_saved_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration;

    /// Saves the current value of state machine `S`.
    ///
    /// On load the `State<S>` value is written back directly and `NextState<S>` is reset, so
    /// `OnEnter` and `OnExit` systems do not run. The state machine must already be initialized.
    fn register_saved_state<S>(&mut self) -> &mut Self
    where
        S: FreelyMutableState,
        State<S>: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration;

    /// Declares component `T` as derived or visual: never saved, recreated by the game's own
    /// observers after a load. Only the unclassified check reads this.
    fn register_rebuilt_component<T: Component>(&mut self) -> &mut Self;
}

impl SaveStateAppExt for App {
    fn register_saved_component<T>(&mut self) -> &mut Self
    where
        T: Component + Reflect + TypePath + GetTypeRegistration,
    {
        self.register_type::<T>();
        self.register_type_data::<T, ReflectComponent>();
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .components
            .insert(TypeId::of::<T>());
        self
    }

    fn register_saved_resource<R>(&mut self) -> &mut Self
    where
        R: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration,
    {
        self.register_type::<R>();
        self.register_type_data::<R, ReflectResource>();
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .resources
            .insert(TypeId::of::<R>());
        self
    }

    fn register_saved_state<S>(&mut self) -> &mut Self
    where
        S: FreelyMutableState,
        State<S>: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration,
    {
        self.register_saved_resource::<State<S>>();
        let hooks = SavedStateHooks {
            check_next_state_exists: check_next_state_exists::<S>,
            reset_pending_transition: reset_pending_transition::<S>,
        };
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .saved_states
            .insert(TypeId::of::<State<S>>(), hooks);
        self
    }

    fn register_rebuilt_component<T: Component>(&mut self) -> &mut Self {
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .rebuilt_components
            .insert(TypeId::of::<T>());
        self
    }
}

fn missing_next_state<S: FreelyMutableState>() -> SaveStateError {
    SaveStateError::MissingResource {
        resource_name: type_name::<NextState<S>>(),
    }
}

fn check_next_state_exists<S: FreelyMutableState>(world: &World) -> Result<(), SaveStateError> {
    world
        .get_resource::<NextState<S>>()
        .map(|_| ())
        .ok_or_else(missing_next_state::<S>)
}

fn reset_pending_transition<S: FreelyMutableState>(
    world: &mut World,
) -> Result<(), SaveStateError> {
    let mut next_state = world
        .get_resource_mut::<NextState<S>>()
        .ok_or_else(missing_next_state::<S>)?;
    next_state.reset();
    Ok(())
}
