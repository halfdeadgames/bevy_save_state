use std::{any::TypeId, collections::HashSet};

use bevy::reflect::{
    NamedField, TypeInfo, TypeRegistration, TypeRegistry, UnnamedField, enums::VariantInfo,
};

use crate::{error::SaveStateError, registration::SavedTypes};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const DESCRIPTION_SEPARATOR: u8 = 0xff;
const COMPONENT_KIND: &str = "component";
const RESOURCE_KIND: &str = "resource";

/// FNV-1a is hand-written so the value never depends on the standard library's hasher.
fn fnv1a_64(bytes: &[u8], start: u64) -> u64 {
    bytes.iter().fold(start, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

/// A type found while walking fields, with type information to use when the registry lacks it.
struct NestedType {
    type_id: TypeId,
    fallback_info: Option<&'static TypeInfo>,
}

impl NestedType {
    fn of_field(type_id: TypeId, info: Option<&'static TypeInfo>) -> Self {
        Self {
            type_id,
            fallback_info: info,
        }
    }

    fn of_element(type_id: TypeId) -> Self {
        Self {
            type_id,
            fallback_info: None,
        }
    }
}

/// Hashes every saved component and resource type, and every type nested inside them.
///
/// Saved states are covered because their `State<S>` resource is a saved resource.
pub(crate) fn compute_schema_fingerprint(
    saved_types: &SavedTypes,
    registry: &TypeRegistry,
) -> Result<u64, SaveStateError> {
    let roots: Vec<(&'static str, TypeId)> = saved_types
        .components
        .iter()
        .map(|type_id| (COMPONENT_KIND, *type_id))
        .chain(
            saved_types
                .resources
                .iter()
                .map(|type_id| (RESOURCE_KIND, *type_id)),
        )
        .collect();
    fingerprint_of_roots(registry, &roots)
}

/// Hashes the given root types with their field names and field types, following each field type
/// through the registry (cycles are cut by a visited set) so a change in a nested type changes
/// the result. Takes explicit roots so it can be tested without a full app.
pub(crate) fn fingerprint_of_roots(
    registry: &TypeRegistry,
    roots: &[(&'static str, TypeId)],
) -> Result<u64, SaveStateError> {
    let mut descriptions = Vec::new();
    let mut visited: HashSet<TypeId> = roots.iter().map(|(_, type_id)| *type_id).collect();
    let mut pending = Vec::new();
    for (kind, type_id) in roots {
        let registration = registry
            .get(*type_id)
            .ok_or(SaveStateError::UnregisteredSavedType { type_id: *type_id })?;
        descriptions.push(format!(
            "{kind} {}",
            describe_type(registration.type_info())
        ));
        collect_nested_types(registration.type_info(), &mut pending);
    }
    while let Some(nested) = pending.pop() {
        if !visited.insert(nested.type_id) {
            continue;
        }
        // A nested type with no registration and no field-provided info is a foreign opaque type;
        // its type path is already part of the parent's description.
        let Some(info) = registry
            .get(nested.type_id)
            .map(TypeRegistration::type_info)
            .or(nested.fallback_info)
        else {
            continue;
        };
        descriptions.push(format!("nested {}", describe_type(info)));
        collect_nested_types(info, &mut pending);
    }
    descriptions.sort();
    Ok(descriptions
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, description| {
            fnv1a_64(
                &[DESCRIPTION_SEPARATOR],
                fnv1a_64(description.as_bytes(), hash),
            )
        }))
}

fn collect_nested_types(info: &TypeInfo, pending: &mut Vec<NestedType>) {
    match info {
        TypeInfo::Struct(struct_info) => collect_named_fields(struct_info.iter(), pending),
        TypeInfo::TupleStruct(tuple_struct_info) => {
            collect_unnamed_fields(tuple_struct_info.iter(), pending);
        }
        TypeInfo::Tuple(tuple_info) => collect_unnamed_fields(tuple_info.iter(), pending),
        TypeInfo::Enum(enum_info) => {
            for variant in enum_info.iter() {
                match variant {
                    VariantInfo::Struct(struct_variant) => {
                        collect_named_fields(struct_variant.iter(), pending);
                    }
                    VariantInfo::Tuple(tuple_variant) => {
                        collect_unnamed_fields(tuple_variant.iter(), pending);
                    }
                    VariantInfo::Unit(_) => {}
                }
            }
        }
        TypeInfo::List(list_info) => {
            pending.push(NestedType::of_element(list_info.item_ty().id()));
        }
        TypeInfo::Array(array_info) => {
            pending.push(NestedType::of_element(array_info.item_ty().id()));
        }
        TypeInfo::Set(set_info) => {
            pending.push(NestedType::of_element(set_info.value_ty().id()));
        }
        TypeInfo::Map(map_info) => {
            pending.push(NestedType::of_element(map_info.key_ty().id()));
            pending.push(NestedType::of_element(map_info.value_ty().id()));
        }
        TypeInfo::Opaque(_) => {}
    }
}

fn collect_named_fields<'a>(
    fields: impl Iterator<Item = &'a NamedField>,
    pending: &mut Vec<NestedType>,
) {
    pending.extend(fields.map(|field| NestedType::of_field(field.type_id(), field.type_info())));
}

fn collect_unnamed_fields<'a>(
    fields: impl Iterator<Item = &'a UnnamedField>,
    pending: &mut Vec<NestedType>,
) {
    pending.extend(fields.map(|field| NestedType::of_field(field.type_id(), field.type_info())));
}

fn describe_type(info: &TypeInfo) -> String {
    let type_path = info.type_path();
    match info {
        TypeInfo::Struct(struct_info) => {
            format!(
                "struct {type_path} {{{}}}",
                describe_named_fields(struct_info.iter())
            )
        }
        TypeInfo::TupleStruct(tuple_struct_info) => format!(
            "tuple_struct {type_path} ({})",
            describe_unnamed_fields(tuple_struct_info.iter())
        ),
        TypeInfo::Tuple(tuple_info) => {
            format!(
                "tuple {type_path} ({})",
                describe_unnamed_fields(tuple_info.iter())
            )
        }
        TypeInfo::Enum(enum_info) => {
            let variants: Vec<String> = enum_info.iter().map(describe_variant).collect();
            format!("enum {type_path} [{}]", variants.join(";"))
        }
        TypeInfo::List(list_info) => {
            format!("list {type_path} of {}", list_info.item_ty().path())
        }
        TypeInfo::Array(array_info) => {
            format!("array {type_path} of {}", array_info.item_ty().path())
        }
        TypeInfo::Set(set_info) => format!("set {type_path} of {}", set_info.value_ty().path()),
        TypeInfo::Map(map_info) => format!(
            "map {type_path} of {} to {}",
            map_info.key_ty().path(),
            map_info.value_ty().path()
        ),
        TypeInfo::Opaque(_) => format!("opaque {type_path}"),
    }
}

fn describe_variant(variant: &VariantInfo) -> String {
    match variant {
        VariantInfo::Struct(struct_variant) => format!(
            "{} {{{}}}",
            struct_variant.name(),
            describe_named_fields(struct_variant.iter())
        ),
        VariantInfo::Tuple(tuple_variant) => format!(
            "{} ({})",
            tuple_variant.name(),
            describe_unnamed_fields(tuple_variant.iter())
        ),
        VariantInfo::Unit(unit_variant) => unit_variant.name().to_owned(),
    }
}

fn describe_named_fields<'a>(fields: impl Iterator<Item = &'a NamedField>) -> String {
    let described: Vec<String> = fields
        .map(|field| format!("{}:{}", field.name(), field.type_path()))
        .collect();
    described.join(",")
}

fn describe_unnamed_fields<'a>(fields: impl Iterator<Item = &'a UnnamedField>) -> String {
    let described: Vec<&str> = fields.map(UnnamedField::type_path).collect();
    described.join(",")
}
