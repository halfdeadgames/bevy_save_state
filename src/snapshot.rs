use std::{
    any::TypeId,
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};

use bevy::{
    asset::{AssetPath, LoadFromPath, UntypedHandle},
    ecs::{
        entity::EntityHashMap,
        entity_disabling::Disabled,
        query::Allow,
        reflect::{AppTypeRegistry, ReflectComponent, ReflectResource},
        relationship::RelationshipTarget,
    },
    log::warn,
    prelude::*,
    reflect::{PartialReflect, TypeData, TypeRegistration, TypeRegistry},
    world_serialization::{
        DynamicWorld, DynamicWorldBuilder, WorldFilter, serde::WorldDeserializer,
    },
};
use serde::de::DeserializeSeed;

use crate::{
    error::SaveStateError,
    file_format::{
        CURRENT_FORMAT_VERSION, SaveHeader, join_header_and_world, split_header_and_world,
    },
    fingerprint::compute_schema_fingerprint,
    identity::{SaveId, Saved},
    registration::SavedTypes,
    unclassified::warn_about_unclassified_components,
};

/// Stands in for an asset server: savestates must not contain asset handles, so any handle found
/// while parsing becomes a default handle instead of starting a load.
struct PlaceholderHandleLoader;

impl LoadFromPath for PlaceholderHandleLoader {
    fn load_from_path_erased(
        &mut self,
        type_id: TypeId,
        _path: AssetPath<'static>,
    ) -> UntypedHandle {
        UntypedHandle::default_for_type(type_id)
    }
}

/// Serializes every [`Saved`] entity and every registered saved resource into savestate text.
///
/// Entities are written in `SaveId` order with sequential entity ids, and components and
/// resources in type path order, so saving, loading, and saving again gives identical text, even
/// across processes. Disabled entities are included. Only hierarchy between `Saved` entities is
/// kept: a `ChildOf` whose parent is not `Saved` is dropped, and `Children` lists only `Saved`
/// children. Logs one warning when unclassified components exist or relationships were dropped.
///
/// # Errors
/// Fails when the plugin is missing, a saved entity has no `SaveId`, or serialization fails.
pub fn save_world_to_string(world: &mut World) -> Result<String, SaveStateError> {
    let saved_types = required_resource::<SavedTypes>(world)?.clone();
    let app_registry = required_resource::<AppTypeRegistry>(world)?.clone();
    warn_about_unclassified_components(world, &saved_types);
    let ordered_entities = collect_saved_entities_in_save_id_order(world)?;
    let registry = app_registry.read();

    let extracted = extract_in_save_id_order(world, &registry, &saved_types, &ordered_entities);
    let mut canonical = rebuild_with_sequential_entities(&extracted, &registry, &saved_types)?;
    // Extraction order follows component ids, which differ between processes.
    sort_by_type_path(&mut canonical);
    let header = SaveHeader {
        format_version: CURRENT_FORMAT_VERSION,
        schema_fingerprint: compute_schema_fingerprint(&saved_types, &registry)?,
    };
    let world_text = canonical
        .serialize(&registry)
        .map_err(|error| SaveStateError::SerializeWorld(error.to_string()))?;
    join_header_and_world(&header, &world_text)
}

/// Restores the world from savestate text produced by [`save_world_to_string`].
///
/// The header and world text are parsed, and everything that could fail while writing is checked,
/// before anything is despawned, so a bad or mismatched file leaves the world unchanged. Saved
/// states are written back directly, so `OnEnter` and `OnExit` systems do not run. Every
/// descendant of a `Saved` entity, `Saved` or not, is despawned. Saved resources that are absent
/// from the file are removed so the world matches the file.
///
/// # Errors
/// Fails on a malformed file, a version or schema mismatch, a type in the file that cannot be
/// written back, or a registered saved state whose `NextState` is missing.
pub fn load_world_from_str(world: &mut World, text: &str) -> Result<(), SaveStateError> {
    let saved_types = required_resource::<SavedTypes>(world)?.clone();
    let app_registry = required_resource::<AppTypeRegistry>(world)?.clone();
    let registry = app_registry.read();

    let (header, world_text) = split_header_and_world(text)?;
    header.verify_against(compute_schema_fingerprint(&saved_types, &registry)?)?;
    let dynamic_world = deserialize_dynamic_world(world_text, &registry)?;
    let resource_types_in_file =
        check_world_is_loadable(world, &dynamic_world, &registry, &saved_types)?;

    despawn_every_saved_entity(world);
    remove_saved_resources_missing_from_file(world, &saved_types, &resource_types_in_file);
    // Entity references cannot fail to map: the mapper hands out a fresh entity for an id that is
    // not in the file, so nothing here can error except the type checks done above.
    dynamic_world
        .write_to_world_with(world, &mut EntityHashMap::default(), &registry)
        .map_err(|error| SaveStateError::WriteWorld(error.to_string()))?;
    for hooks in saved_types.saved_states.values() {
        (hooks.reset_pending_transition)(world)?;
    }
    Ok(())
}

/// Saves the world with [`save_world_to_string`] into a file, creating parent folders.
///
/// # Errors
/// Fails when saving fails or the folder or file cannot be written.
pub fn save_world_to_file(world: &mut World, path: &Path) -> Result<(), SaveStateError> {
    let text = save_world_to_string(world)?;
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        fs::create_dir_all(folder).map_err(|source| SaveStateError::Io {
            path: folder.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, text).map_err(|source| SaveStateError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads a file and restores the world with [`load_world_from_str`].
///
/// # Errors
/// Fails when the file cannot be read or loading fails.
pub fn load_world_from_file(world: &mut World, path: &Path) -> Result<(), SaveStateError> {
    let text = fs::read_to_string(path).map_err(|source| SaveStateError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    load_world_from_str(world, &text)
}

fn required_resource<R: Resource>(world: &World) -> Result<&R, SaveStateError> {
    world
        .get_resource::<R>()
        .ok_or(SaveStateError::MissingResource {
            resource_name: std::any::type_name::<R>(),
        })
}

fn collect_saved_entities_in_save_id_order(
    world: &mut World,
) -> Result<Vec<(Entity, SaveId)>, SaveStateError> {
    let mut query =
        world.query_filtered::<(Entity, Option<&SaveId>), (With<Saved>, Allow<Disabled>)>();
    let mut ordered = Vec::new();
    for (entity, save_id) in query.iter(world) {
        let save_id = save_id.ok_or(SaveStateError::EntityWithoutSaveId { entity })?;
        ordered.push((entity, *save_id));
    }
    ordered.sort_by_key(|(_, save_id)| *save_id);
    Ok(ordered)
}

fn allow_only(type_ids: &BTreeSet<TypeId>) -> WorldFilter {
    type_ids
        .iter()
        .fold(WorldFilter::deny_all(), |filter, type_id| {
            filter.allow_by_id(*type_id)
        })
}

fn extract_saved_types(
    world: &World,
    registry: &TypeRegistry,
    saved_types: &SavedTypes,
    entities: impl Iterator<Item = Entity>,
) -> DynamicWorld {
    DynamicWorldBuilder::from_world(world, registry)
        .with_component_filter(allow_only(&saved_types.components))
        .with_resource_filter(allow_only(&saved_types.resources))
        .extract_entities(entities)
        .extract_resources()
        .build()
}

fn extract_in_save_id_order(
    world: &World,
    registry: &TypeRegistry,
    saved_types: &SavedTypes,
    ordered_entities: &[(Entity, SaveId)],
) -> DynamicWorld {
    let entities = ordered_entities.iter().map(|(entity, _)| *entity);
    let mut extracted = extract_saved_types(world, registry, saved_types, entities);
    // The builder orders entities by entity index, which changes after every load.
    let save_ids: HashMap<Entity, SaveId> = ordered_entities.iter().copied().collect();
    extracted
        .entities
        .sort_by_key(|dynamic_entity| save_ids.get(&dynamic_entity.entity).copied());
    keep_only_saved_relationships(&mut extracted);
    sort_by_type_path(&mut extracted);
    extracted
}

/// Orders resources and each entity's components by reflected type path.
fn sort_by_type_path(dynamic_world: &mut DynamicWorld) {
    dynamic_world
        .resources
        .sort_by(|left, right| left.reflect_type_path().cmp(right.reflect_type_path()));
    for dynamic_entity in &mut dynamic_world.entities {
        dynamic_entity
            .components
            .sort_by(|left, right| left.reflect_type_path().cmp(right.reflect_type_path()));
    }
}

#[derive(Default)]
struct DroppedRelationships {
    child_of_components: usize,
    children_entries: usize,
}

/// Removes hierarchy links that point at entities outside the save, because their ids would be
/// dead after a load. Only `Saved`-to-`Saved` hierarchy survives.
fn keep_only_saved_relationships(extracted: &mut DynamicWorld) {
    let saved: HashSet<Entity> = extracted
        .entities
        .iter()
        .map(|dynamic_entity| dynamic_entity.entity)
        .collect();
    let mut dropped = DroppedRelationships::default();
    for dynamic_entity in &mut extracted.entities {
        dynamic_entity
            .components
            .retain_mut(|component| keep_component(&mut **component, &saved, &mut dropped));
    }
    if dropped.child_of_components > 0 || dropped.children_entries > 0 {
        warn!(
            "dropped {} ChildOf components and {} Children entries that point at entities that \
             are not Saved; only Saved-to-Saved hierarchy is kept",
            dropped.child_of_components, dropped.children_entries
        );
    }
}

fn keep_component(
    component: &mut dyn PartialReflect,
    saved: &HashSet<Entity>,
    dropped: &mut DroppedRelationships,
) -> bool {
    if let Some(child_of) = component.try_downcast_ref::<ChildOf>() {
        let parent_is_saved = saved.contains(&child_of.0);
        if !parent_is_saved {
            dropped.child_of_components += 1;
        }
        return parent_is_saved;
    }
    if let Some(children) = component.try_downcast_mut::<Children>() {
        let before = children.len();
        children
            .collection_mut_risky()
            .retain(|child| saved.contains(child));
        let remaining = children.len();
        dropped.children_entries += before - remaining;
        return remaining > 0;
    }
    true
}

/// Writing into a fresh world spawns entities one after another, which gives every entity a
/// sequential id and remaps entity references to match. Extracting again then yields a result
/// that does not depend on the ids the live world happened to use.
fn rebuild_with_sequential_entities(
    extracted: &DynamicWorld,
    registry: &TypeRegistry,
    saved_types: &SavedTypes,
) -> Result<DynamicWorld, SaveStateError> {
    let mut scratch_world = World::new();
    let mut entity_map = EntityHashMap::<Entity>::default();
    extracted
        .write_to_world_with(&mut scratch_world, &mut entity_map, registry)
        .map_err(|error| SaveStateError::WriteWorld(error.to_string()))?;
    let scratch_entities = extracted
        .entities
        .iter()
        .map(|dynamic_entity| {
            entity_map
                .get(&dynamic_entity.entity)
                .copied()
                .ok_or(SaveStateError::UnmappedEntity {
                    entity: dynamic_entity.entity,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(extract_saved_types(
        &scratch_world,
        registry,
        saved_types,
        scratch_entities.into_iter(),
    ))
}

fn deserialize_dynamic_world(
    world_text: &str,
    registry: &TypeRegistry,
) -> Result<DynamicWorld, SaveStateError> {
    let mut deserializer = ron::de::Deserializer::from_str(world_text)
        .map_err(|error| SaveStateError::ParseWorld(error.to_string()))?;
    let mut handle_loader = PlaceholderHandleLoader;
    let world_deserializer = WorldDeserializer {
        type_registry: registry,
        load_from_path: &mut handle_loader,
    };
    world_deserializer
        .deserialize(&mut deserializer)
        .map_err(|error| SaveStateError::ParseWorld(deserializer.span_error(error).to_string()))
}

fn represented_registration<'a>(
    value: &dyn PartialReflect,
    registry: &'a TypeRegistry,
) -> Result<&'a TypeRegistration, SaveStateError> {
    let unloadable = || SaveStateError::UnloadableType {
        type_path: value.reflect_type_path().to_owned(),
    };
    let info = value.get_represented_type_info().ok_or_else(unloadable)?;
    registry.get(info.type_id()).ok_or_else(unloadable)
}

fn require_type_data<T: TypeData>(registration: &TypeRegistration) -> Result<(), SaveStateError> {
    registration
        .data::<T>()
        .map(|_| ())
        .ok_or_else(|| SaveStateError::UnloadableType {
            type_path: registration.type_info().type_path().to_owned(),
        })
}

/// Checks everything that could fail once despawning has started, so a failure leaves the world
/// untouched. Returns the type ids of the resources stored in the file.
fn check_world_is_loadable(
    world: &World,
    dynamic_world: &DynamicWorld,
    registry: &TypeRegistry,
    saved_types: &SavedTypes,
) -> Result<BTreeSet<TypeId>, SaveStateError> {
    for dynamic_entity in &dynamic_world.entities {
        for component in &dynamic_entity.components {
            let registration = represented_registration(&**component, registry)?;
            require_type_data::<ReflectComponent>(registration)?;
        }
    }
    let mut resource_types = BTreeSet::new();
    for resource in &dynamic_world.resources {
        let registration = represented_registration(&**resource, registry)?;
        require_type_data::<ReflectResource>(registration)?;
        resource_types.insert(registration.type_id());
    }
    for hooks in saved_types.saved_states.values() {
        (hooks.check_next_state_exists)(world)?;
    }
    Ok(resource_types)
}

fn remove_saved_resources_missing_from_file(
    world: &mut World,
    saved_types: &SavedTypes,
    resource_types_in_file: &BTreeSet<TypeId>,
) {
    for type_id in saved_types.resources.difference(resource_types_in_file) {
        // A type the world has never registered cannot be present, so there is nothing to remove.
        if let Some(component_id) = world.components().get_valid_id(*type_id) {
            world.remove_resource_by_id(component_id);
        }
    }
}

fn despawn_every_saved_entity(world: &mut World) {
    let saved_entities: Vec<Entity> = world
        .query_filtered::<Entity, (With<Saved>, Allow<Disabled>)>()
        .iter(world)
        .collect();
    for entity in saved_entities {
        // Despawning a parent already removed its Saved descendants, so some are gone by now.
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
}
