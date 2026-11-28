use isotherm_import::vale_project::{height_layers, read_layer};
use vale_store::{
    GeometryKind, HEIGHT_UNIT, Project, ProjectFile, TILE_SIZE, TileChanges, TileKey, World,
};

#[test]
fn the_height_layers_of_a_vale_project_are_listed_and_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.gpkg");
    let mut project = Project::new(World::default());
    project.add_raster("Old".into(), 256, HEIGHT_UNIT.into(), -6000.0, 6000.0);
    project.add_raster("Rain".into(), 256, "mm".into(), 0.0, 4000.0);
    let new = project.add_raster("New".into(), 512, HEIGHT_UNIT.into(), -9000.0, 7000.0);
    project.add_layer("Towns".into(), GeometryKind::Point, None, vec![]);
    let mut file = ProjectFile::create(&path, &project).unwrap();
    let pixels = vec![40000u16; TILE_SIZE * TILE_SIZE];
    let key = TileKey {
        level: 0,
        face: 2,
        column: 1,
        row: 0,
    };
    let changes = TileChanges {
        raster: new,
        all: false,
        put: vec![(key, &pixels[..])],
        delete: vec![],
    };
    file.save_tiles(&project, &[changes]).unwrap();
    drop(file);

    let layers = height_layers(&path).unwrap();
    let names: Vec<_> = layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Old", "New"]);
    let layer = &layers[1];
    assert_eq!(layer.face_size, 512);
    assert_eq!((layer.low_m, layer.high_m), (-9000.0, 7000.0));

    let terrain = read_layer(&path, layer.id).unwrap();
    assert_eq!(terrain.heights.face_size(), 512);
    assert_eq!((terrain.low_m, terrain.high_m), (-9000.0, 7000.0));
    assert_eq!(terrain.sea_level_m, 0.0);
    assert_eq!(terrain.heights.get(2, TILE_SIZE as i64 + 5, 7), 40000);
    assert_eq!(terrain.heights.get(2, 5, 7), 0);
}
