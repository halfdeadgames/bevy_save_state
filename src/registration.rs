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
    /// Fails when the `NextState` resource of the registered state type is missing, so a load can be refused before the world changes.
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
    /// Saves component `ComponentType` on every [`Saved`](crate::Saved) entity that has it.
    ///
    /// Registers the type in the type registry and guarantees its `ReflectComponent` data.
    fn register_saved_component<ComponentType>(&mut self) -> &mut Self
    where
        ComponentType: Component + Reflect + TypePath + GetTypeRegistration;

    /// Saves resource `ResourceType` and writes it back on load.
    ///
    /// Registers the type in the type registry and guarantees its `ReflectResource` data.
    fn register_saved_resource<ResourceType>(&mut self) -> &mut Self
    where
        ResourceType: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration;

    /// Saves the current value of state machine `StateType`.
    ///
    /// On load the `State<StateType>` value is written back directly and `NextState<StateType>` is
    /// reset, so `OnEnter` and `OnExit` systems do not run. The state machine must already be
    /// initialized.
    fn register_saved_state<StateType>(&mut self) -> &mut Self
    where
        StateType: FreelyMutableState,
        State<StateType>: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration;

    /// Declares component `ComponentType` as derived or visual: never saved, recreated by the game's own
    /// observers after a load. Only the unclassified check reads this.
    fn register_rebuilt_component<ComponentType: Component>(&mut self) -> &mut Self;
}

impl SaveStateAppExt for App {
    fn register_saved_component<ComponentType>(&mut self) -> &mut Self
    where
        ComponentType: Component + Reflect + TypePath + GetTypeRegistration,
    {
        self.register_type::<ComponentType>();
        self.register_type_data::<ComponentType, ReflectComponent>();
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .components
            .insert(TypeId::of::<ComponentType>());
        self
    }

    fn register_saved_resource<ResourceType>(&mut self) -> &mut Self
    where
        ResourceType: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration,
    {
        self.register_type::<ResourceType>();
        self.register_type_data::<ResourceType, ReflectResource>();
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .resources
            .insert(TypeId::of::<ResourceType>());
        self
    }

    fn register_saved_state<StateType>(&mut self) -> &mut Self
    where
        StateType: FreelyMutableState,
        State<StateType>: Resource + Reflect + FromReflect + TypePath + GetTypeRegistration,
    {
        self.register_saved_resource::<State<StateType>>();
        let hooks = SavedStateHooks {
            check_next_state_exists: check_next_state_exists::<StateType>,
            reset_pending_transition: reset_pending_transition::<StateType>,
        };
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .saved_states
            .insert(TypeId::of::<State<StateType>>(), hooks);
        self
    }

    fn register_rebuilt_component<ComponentType: Component>(&mut self) -> &mut Self {
        self.init_resource::<SavedTypes>();
        self.world_mut()
            .resource_mut::<SavedTypes>()
            .rebuilt_components
            .insert(TypeId::of::<ComponentType>());
        self
    }
}

fn missing_next_state<StateType: FreelyMutableState>() -> SaveStateError {
    SaveStateError::MissingResource {
        resource_name: type_name::<NextState<StateType>>(),
    }
}

fn check_next_state_exists<StateType: FreelyMutableState>(
    world: &World,
) -> Result<(), SaveStateError> {
    world
        .get_resource::<NextState<StateType>>()
        .map(|_| ())
        .ok_or_else(missing_next_state::<StateType>)
}

fn reset_pending_transition<StateType: FreelyMutableState>(
    world: &mut World,
) -> Result<(), SaveStateError> {
    let mut next_state = world
        .get_resource_mut::<NextState<StateType>>()
        .ok_or_else(missing_next_state::<StateType>)?;
    next_state.reset();
    Ok(())
}
