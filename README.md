# bevy_save_state

Exact savestates for testing and debugging a Bevy 0.20 game, like an emulator's save and load. Save
the full game state between frames, load it later in the same build, and the game continues exactly
as before. It is not for player save slots: a save from an older build is refused with a clear
error instead of being migrated.

## Setup

```rust
app.add_plugins(SaveStatePlugin)
    .add_plugins(QuickSaveKeysPlugin::default()) // optional: F5 saves, F8 loads
    .register_saved_component::<Health>()
    .register_saved_resource::<Score>()
    .register_saved_state::<GamePhase>()
    .register_rebuilt_component::<Sprite>();
```

Spawn game entities with the `Saved` marker. It gives each entity a stable `SaveId`.

## Rules for exactness

1. Mark every game entity `Saved`. Entities without it are neither saved nor despawned on load.
2. Register every game-state type with `register_saved_component`, `register_saved_resource`, or
   `register_saved_state`. Types need `#[derive(Reflect)]` and `#[reflect(Component)]` or
   `#[reflect(Resource)]`. Entity fields need `#[entities]` so references are remapped on load.
3. Register derived or visual components (sprites, meshes, computed transforms) with
   `register_rebuilt_component`. They are never saved. Your own observers must recreate them when
   the saved components come back.
4. Game time must be a saved tick or turn resource, never Bevy's `Time`, which is not restored.
5. Randomness must be stored as stream positions, for example a ChaCha seed, stream, and word
   position, and the generator rebuilt from them each time it is used.
6. Save and load happen between frames, never from inside a system that is mid-update.

Saving logs one warning listing components on `Saved` entities that are neither saved nor marked
rebuilt (`unclassified_component_names` returns the same list). Saving still succeeds.

Loading first checks everything that could fail (types in the file, `NextState` for each saved
state), so a failure leaves the world untouched. Then it despawns every `Saved` entity, hidden
ones included, together with all of its descendants, whether or not they are `Saved`. Non-`Saved`
children such as visual children are not restored, so rebuild them with observers. It writes the
saved entities and resources, removes saved resources that are absent from the file, and writes
saved states back directly, so `OnEnter` and `OnExit` systems do not run.

`Disabled` is saved by default, so disabled entities come back disabled. Entities hidden by a
disabling component your game registers (`register_disabling_component`) are also saved and
despawned on load. Register that component with `register_saved_component` so those entities come
back hidden. Until you do, the save warning lists it as unclassified. `Children` is saved by
default because loading skips relationship hooks. Only `Saved`-to-`Saved` hierarchy is kept: a
`ChildOf` whose parent is not `Saved` is dropped and `Children` lists only `Saved` children, with
one warning naming how many links were dropped. Put the `RebuiltChild` marker on non-`Saved`
children that your observers recreate after a load (such as drawn layers): links to them are
dropped quietly, with only a debug log, so the warning shows only children that may have been
meant to be `Saved`.

Saves and loads queued from `QuickSaveKeysPlugin` keys run at the next command sync point, not at
a moment you choose.

## Using it with bevy_input_driver

```rust
app.set_driver_state_handlers(
    bevy_save_state::save_state_system,
    bevy_save_state::load_state_system,
);
```

Both are exclusive one-shot systems taking `In<PathBuf>` and returning `Result<(), String>`.

## File format

Text, two sections. The first line is the header as one line of RON:

```
(format_version:1,schema_fingerprint:1234567890)
```

Everything after the first newline is the world as Bevy's `DynamicWorld` RON. Entities are sorted
by `SaveId` and rewritten through a scratch world so entity ids are sequential. Resources and each
entity's components are sorted by type path. Saving, loading, and saving again therefore gives
identical text, even across processes and at different frames. The header holds no timestamp or
frame number for the same reason.

## Schema fingerprint

The header holds a 64-bit FNV-1a hash over every saved component, resource, and state type. For
each type it covers the type path plus field names and field type paths (enum variants included),
and it follows every field, list, map, set, and array element type to cover nested types too.
Add, remove, rename, or retype a saved field and the fingerprint changes, so old saves fail with
`SchemaMismatch` and must be remade. A changed file layout fails with `FormatVersionMismatch`.
A bad or mismatched file is rejected before the world is touched.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option, the same as Bevy.

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in this work, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
