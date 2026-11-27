use std::any::TypeId;

use bevy::{
    ecs::{entity_disabling::Disabled, query::Allow},
    prelude::*,
    reflect::TypeRegistry,
    state::app::StatesPlugin,
};
use rand::{Rng, SeedableRng, rngs::ChaCha8Rng};

use crate::{
    RebuiltChild, SaveStateAppExt, SaveStateError, SaveStatePlugin, Saved,
    file_format::{join_header_and_world, split_header_and_world},
    fingerprint::fingerprint_of_roots,
    load_world_from_string, save_world_to_string, unclassified_component_names,
};

/// Two layouts share every type path and differ only in the type of a nested field.
mod narrow_nested_layout {
    use bevy::prelude::Reflect;

    #[derive(Reflect)]
    #[type_path = "layout_test"]
    pub struct Inner {
        pub value: u32,
    }

    #[derive(Reflect)]
    #[type_path = "layout_test"]
    pub struct Outer {
        pub inner: Inner,
    }
}

mod wide_nested_layout {
    use bevy::prelude::Reflect;

    #[derive(Reflect)]
    #[type_path = "layout_test"]
    pub struct Inner {
        pub value: u64,
    }

    #[derive(Reflect)]
    #[type_path = "layout_test"]
    pub struct Outer {
        pub inner: Inner,
    }
}

#[derive(Component, Reflect, Default, Clone, Copy, PartialEq, Debug)]
#[reflect(Component, Default)]
struct Marker(u32);

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Target(#[entities] Entity);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource, Default)]
struct Score(u32);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource, Default)]
struct Total(u64);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource, Default)]
struct RandomStream {
    seed: u64,
    stream: u64,
    word_position: u128,
}

#[derive(Component)]
struct Visual;

#[derive(States, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Phase {
    #[default]
    Alpha,
    Beta,
}

#[derive(Resource, Default)]
struct AlphaEntered(u32);

fn saved_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(SaveStatePlugin)
        .register_saved_component::<Marker>()
        .register_saved_component::<Target>()
        .register_saved_resource::<Score>();
    app
}

fn draw_number_into_total(mut stream: ResMut<RandomStream>, mut total: ResMut<Total>) {
    let mut random_number_generator = ChaCha8Rng::seed_from_u64(stream.seed);
    random_number_generator.set_stream(stream.stream);
    random_number_generator.set_word_pos(stream.word_position);
    total.0 = total.0.wrapping_add(random_number_generator.next_u64());
    stream.word_position = random_number_generator.get_word_pos();
}

fn count_alpha_entered(mut entered: ResMut<AlphaEntered>) {
    entered.0 += 1;
}

#[test]
fn round_trip_gives_identical_text_and_keeps_references() {
    let mut app = saved_test_app();
    let world = app.world_mut();
    world.insert_resource(Score(42));
    let target = world.spawn((Saved, Marker(7))).id();
    let holder = world.spawn((Saved, Marker(1), Target(target))).id();
    world.spawn((Saved, Marker(2), ChildOf(holder)));

    let first = save_world_to_string(world).unwrap();
    load_world_from_string(world, &first).unwrap();
    let second = save_world_to_string(world).unwrap();
    assert_eq!(first, second);

    let mut targets = world.query::<&Target>();
    let target_entity = targets.single(world).unwrap().0;
    assert_eq!(world.get::<Marker>(target_entity), Some(&Marker(7)));
    assert_eq!(world.resource::<Score>().0, 42);

    let mut children = world.query::<(&Marker, &Children)>();
    let (marker, held) = children.single(world).unwrap();
    assert_eq!(*marker, Marker(1));
    assert_eq!(held.len(), 1);
}

#[test]
fn loading_restores_the_random_stream_exactly() {
    let mut app = saved_test_app();
    app.register_saved_resource::<Total>()
        .register_saved_resource::<RandomStream>()
        .insert_resource(Total(0))
        .insert_resource(RandomStream {
            seed: 99,
            stream: 3,
            word_position: 0,
        })
        .add_systems(Update, draw_number_into_total);
    let saved = save_world_to_string(app.world_mut()).unwrap();

    for _ in 0..5 {
        app.update();
    }
    let recorded_total = app.world().resource::<Total>().0;

    load_world_from_string(app.world_mut(), &saved).unwrap();
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(app.world().resource::<Total>().0, recorded_total);
}

#[test]
fn schema_mismatch_is_refused_and_leaves_the_world_unchanged() {
    let mut app = saved_test_app();
    let world = app.world_mut();
    world.spawn((Saved, Marker(5)));
    let text = save_world_to_string(world).unwrap();
    let (mut header, world_text) = split_header_and_world(&text).unwrap();
    header.schema_fingerprint = header.schema_fingerprint.wrapping_add(1);
    let tampered = join_header_and_world(&header, world_text).unwrap();

    world.spawn((Saved, Marker(6)));
    let before = save_world_to_string(world).unwrap();
    let result = load_world_from_string(world, &tampered);
    let after = save_world_to_string(world).unwrap();

    assert!(matches!(result, Err(SaveStateError::SchemaMismatch { .. })));
    assert_eq!(before, after);
}

#[test]
fn unclassified_components_are_listed_until_marked_rebuilt() {
    let mut app = saved_test_app();
    app.world_mut().spawn((Saved, Marker(1), Visual));
    let names = unclassified_component_names(app.world());
    assert!(names.iter().any(|name| name.ends_with("Visual")));
    assert!(!names.iter().any(|name| name.ends_with("Marker")));

    app.register_rebuilt_component::<Visual>();
    assert!(unclassified_component_names(app.world()).is_empty());
}

#[test]
fn disabled_saved_entities_come_back_disabled_without_duplicates() {
    let mut app = saved_test_app();
    let world = app.world_mut();
    world.spawn((Saved, Marker(3), Disabled));
    let first = save_world_to_string(world).unwrap();

    load_world_from_string(world, &first).unwrap();

    let mut markers = world.query_filtered::<(&Marker, Has<Disabled>), Allow<Disabled>>();
    let found: Vec<_> = markers.iter(world).collect();
    assert_eq!(found, vec![(&Marker(3), true)]);
    assert_eq!(first, save_world_to_string(world).unwrap());
}

#[test]
fn load_with_a_missing_next_state_fails_and_leaves_saved_entities_intact() {
    let mut app = saved_test_app();
    app.add_plugins(StatesPlugin)
        .init_state::<Phase>()
        .register_saved_state::<Phase>();
    app.update();
    app.world_mut().spawn((Saved, Marker(1)));
    let saved = save_world_to_string(app.world_mut()).unwrap();

    let world = app.world_mut();
    world.spawn((Saved, Marker(2)));
    world.remove_resource::<NextState<Phase>>();
    let result = load_world_from_string(world, &saved);

    assert!(matches!(
        result,
        Err(SaveStateError::MissingResource { .. })
    ));
    let mut markers = world.query::<&Marker>();
    assert_eq!(markers.iter(world).count(), 2);
}

#[test]
fn non_saved_children_are_dropped_and_saves_stay_identical() {
    let mut app = saved_test_app();
    let world = app.world_mut();
    let parent = world.spawn((Saved, Marker(1))).id();
    world.spawn((Saved, Marker(2), ChildOf(parent)));
    world.spawn((Visual, ChildOf(parent)));
    world.spawn((Visual, RebuiltChild, ChildOf(parent)));
    let first = save_world_to_string(world).unwrap();
    assert_eq!(first, save_world_to_string(world).unwrap());

    load_world_from_string(world, &first).unwrap();

    assert_eq!(first, save_world_to_string(world).unwrap());
    let mut parents = world.query::<(&Marker, &Children)>();
    let (marker, children) = parents.single(world).unwrap();
    assert_eq!(*marker, Marker(1));
    assert_eq!(children.len(), 1);
    assert_eq!(world.get::<Marker>(children[0]), Some(&Marker(2)));
    let mut visuals = world.query::<&Visual>();
    assert_eq!(visuals.iter(world).count(), 0);
}

#[test]
fn saved_resources_missing_from_the_file_are_removed_on_load() {
    let mut app = saved_test_app();
    let world = app.world_mut();
    let saved_without_score = save_world_to_string(world).unwrap();
    world.insert_resource(Score(5));

    load_world_from_string(world, &saved_without_score).unwrap();

    assert!(world.get_resource::<Score>().is_none());
}

#[test]
fn nested_field_changes_change_the_fingerprint() {
    let mut narrow_registry = TypeRegistry::empty();
    narrow_registry.register::<narrow_nested_layout::Outer>();
    let mut wide_registry = TypeRegistry::empty();
    wide_registry.register::<wide_nested_layout::Outer>();

    let narrow = fingerprint_of_roots(
        &narrow_registry,
        &[("component", TypeId::of::<narrow_nested_layout::Outer>())],
    )
    .unwrap();
    let wide = fingerprint_of_roots(
        &wide_registry,
        &[("component", TypeId::of::<wide_nested_layout::Outer>())],
    )
    .unwrap();
    let narrow_again = fingerprint_of_roots(
        &narrow_registry,
        &[("component", TypeId::of::<narrow_nested_layout::Outer>())],
    )
    .unwrap();

    assert_ne!(narrow, wide);
    assert_eq!(narrow, narrow_again);
}

#[test]
fn loading_a_saved_state_does_not_rerun_on_enter() {
    let mut app = saved_test_app();
    app.add_plugins(StatesPlugin)
        .init_state::<Phase>()
        .register_saved_state::<Phase>()
        .init_resource::<AlphaEntered>()
        .add_systems(OnEnter(Phase::Alpha), count_alpha_entered);
    app.update();
    let saved = save_world_to_string(app.world_mut()).unwrap();

    app.world_mut()
        .resource_mut::<NextState<Phase>>()
        .set(Phase::Beta);
    app.update();
    assert_eq!(*app.world().resource::<State<Phase>>().get(), Phase::Beta);

    load_world_from_string(app.world_mut(), &saved).unwrap();
    app.update();
    assert_eq!(*app.world().resource::<State<Phase>>().get(), Phase::Alpha);
    assert_eq!(app.world().resource::<AlphaEntered>().0, 1);
}
