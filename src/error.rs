use std::{any::TypeId, error::Error, fmt, io, path::PathBuf};

use bevy::prelude::Entity;

/// Everything that can go wrong while saving or loading a savestate.
///
/// Errors from reading, parsing, and checking the file are returned before the world is touched, so
/// a failed load of a bad or mismatched file leaves the world unchanged.
#[derive(Debug)]
pub enum SaveStateError {
    /// A resource the savestate machinery needs is missing, usually because `SaveStatePlugin` was
    /// not added.
    MissingResource {
        /// Type name of the missing resource.
        resource_name: &'static str,
    },
    /// An entity marked `Saved` has no `SaveId`, so its place in the file is undefined.
    EntityWithoutSaveId {
        /// The offending entity.
        entity: Entity,
    },
    /// A type on the saved allow-list is not in the type registry.
    UnregisteredSavedType {
        /// Type id of the missing registration.
        type_id: TypeId,
    },
    /// An extracted entity vanished while the world was being rewritten into canonical order.
    UnmappedEntity {
        /// The entity that has no mapping.
        entity: Entity,
    },
    /// The file holds a type that cannot be written back: it has no type information, is not in
    /// the type registry, or lacks `ReflectComponent` or `ReflectResource` data.
    UnloadableType {
        /// Type path of the offending type.
        type_path: String,
    },
    /// The header could not be turned into text.
    SerializeHeader(String),
    /// The world could not be turned into text.
    SerializeWorld(String),
    /// The file does not have a header line followed by a world section.
    MalformedFile {
        /// What is wrong with the file layout.
        reason: &'static str,
    },
    /// The header line could not be parsed.
    ParseHeader(String),
    /// The world section could not be parsed.
    ParseWorld(String),
    /// The file was written by an incompatible savestate format version.
    FormatVersionMismatch {
        /// Version stored in the file.
        saved: u32,
        /// Version this build understands.
        current: u32,
    },
    /// The file was written with a different set of saved types.
    SchemaMismatch {
        /// Fingerprint stored in the file.
        saved: u64,
        /// Fingerprint of this build.
        current: u64,
    },
    /// Writing the parsed world into the live world failed.
    WriteWorld(String),
    /// Reading or writing the savestate file failed.
    Io {
        /// File or folder that was being accessed.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
}

impl fmt::Display for SaveStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingResource { resource_name } => write!(
                formatter,
                "resource {resource_name} is missing; add SaveStatePlugin to the app"
            ),
            Self::EntityWithoutSaveId { entity } => {
                write!(formatter, "saved entity {entity:?} has no SaveId")
            }
            Self::UnregisteredSavedType { type_id } => {
                write!(
                    formatter,
                    "saved type {type_id:?} is not in the type registry"
                )
            }
            Self::UnmappedEntity { entity } => {
                write!(
                    formatter,
                    "entity {entity:?} was lost while ordering the save"
                )
            }
            Self::UnloadableType { type_path } => write!(
                formatter,
                "the savestate holds type {type_path}, which is not registered with component \
                 or resource reflection"
            ),
            Self::SerializeHeader(message) => {
                write!(formatter, "could not write the savestate header: {message}")
            }
            Self::SerializeWorld(message) => {
                write!(formatter, "could not write the savestate world: {message}")
            }
            Self::MalformedFile { reason } => {
                write!(formatter, "the savestate file is malformed: {reason}")
            }
            Self::ParseHeader(message) => {
                write!(formatter, "could not read the savestate header: {message}")
            }
            Self::ParseWorld(message) => {
                write!(formatter, "could not read the savestate world: {message}")
            }
            Self::FormatVersionMismatch { saved, current } => write!(
                formatter,
                "the savestate has format version {saved} but this build uses version {current}; \
                 the save must be remade"
            ),
            Self::SchemaMismatch { saved, current } => write!(
                formatter,
                "the savestate was made with different saved types (schema fingerprint \
                 {saved:016x}, this build {current:016x}) and must be remade"
            ),
            Self::WriteWorld(message) => {
                write!(
                    formatter,
                    "could not apply the savestate to the world: {message}"
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "file error at {}: {source}", path.display())
            }
        }
    }
}

impl Error for SaveStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
