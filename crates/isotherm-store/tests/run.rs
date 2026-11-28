use isotherm_sim::{MODEL_VERSION, Model, STEPS_PER_MONTH};
use isotherm_store::{RunFile, WorldFile};
use isotherm_world::{Planet, World};
use rusqlite::Connection;

fn world() -> World {
    World::flat("Run", Planet::earth(), 8)
}

/// A model in the second month, with the first month filled.
fn model(world: &World) -> Model {
    let mut model = Model::new(world, 4);
    model.run(u64::from(STEPS_PER_MONTH) + 7);
    model
}

#[test]
fn a_run_round_trips_with_its_partial_month() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    let world = world();
    let mut file = WorldFile::create(&path, &world).unwrap();
    assert_eq!(file.load_run().unwrap(), None);
    let state = model(&world).snapshot();
    assert_eq!(state.means.partial().count, 7);
    assert!(state.means.fields()[0].filled[0]);
    file.save_run(&state).unwrap();
    drop(file);
    let (file, _) = WorldFile::open(&path).unwrap();
    assert_eq!(file.load_run().unwrap(), Some(state));
}

#[test]
fn a_run_file_writes_the_run_beside_an_open_world_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    let world = world();
    let file = WorldFile::create(&path, &world).unwrap();
    let state = model(&world).snapshot();
    RunFile::open(&path).unwrap().save_run(&state).unwrap();
    assert_eq!(file.load_run().unwrap(), Some(state));
}

#[test]
fn a_change_to_the_world_clears_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    let world = world();
    let mut file = WorldFile::create(&path, &world).unwrap();
    let state = model(&world).snapshot();

    file.save_run(&state).unwrap();
    let mut planet = Planet::earth();
    planet.atmosphere.co2_ppm = 560.0;
    file.save_planet(&planet).unwrap();
    assert_eq!(file.load_run().unwrap(), None);

    file.save_run(&state).unwrap();
    file.set_sea_level(0.0, 100.0).unwrap();
    assert_eq!(file.load_run().unwrap(), None);

    file.save_run(&state).unwrap();
    file.undo().unwrap();
    assert_eq!(file.load_run().unwrap(), None);

    file.save_run(&state).unwrap();
    file.redo().unwrap();
    assert_eq!(file.load_run().unwrap(), None);

    file.save_run(&state).unwrap();
    file.replace_terrain(&world.terrain).unwrap();
    assert_eq!(file.load_run().unwrap(), None);

    file.save_run(&state).unwrap();
    file.clear_run().unwrap();
    assert_eq!(file.load_run().unwrap(), None);
}

#[test]
fn a_run_of_another_model_version_does_not_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    let world = world();
    let mut file = WorldFile::create(&path, &world).unwrap();
    file.save_run(&model(&world).snapshot()).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE isotherm_run SET model_version = ?1",
            [MODEL_VERSION + 1],
        )
        .unwrap();
    assert_eq!(file.load_run().unwrap(), None);
}

#[test]
fn the_run_tables_are_in_the_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    WorldFile::create(&path, &world()).unwrap();
    let conn = Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM gpkg_contents WHERE data_type = 'isotherm'
               AND table_name IN ('isotherm_run', 'isotherm_run_grid', 'isotherm_field')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
}
