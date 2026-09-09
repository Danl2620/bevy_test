pub mod tiled;

use bevy::prelude::*;

/// The object layer that spawn points are authored on. Objects elsewhere in the map are
/// ignored, so an object layer can be added for something else without it accidentally
/// spawning entities.
const SPAWNER_LAYER: &str = "Spawner Layer";

/// A visible object in the map's spawner layer whose Tiled *type* is `spawn`.
pub struct SpawnPoint {
    pub name: String,
    pub position: Vec2,
}

/// Collects the map's spawn points in world coordinates.
///
/// Tiled measures object y downward from the top of the map while bevy measures it
/// upward, so the y axis is flipped here to line up with how `helpers::tiled` places
/// the tile layers.
pub fn spawn_points(map: &tiled::TiledMap) -> Vec<SpawnPoint> {
    let map_height_px = (map.map.height * map.map.tile_height) as f32;
    let mut points = Vec::new();

    for layer in map.map.layers() {
        if layer.name != SPAWNER_LAYER {
            continue;
        }
        let ::tiled::LayerType::Objects(object_layer) = layer.layer_type() else {
            warn!("'{SPAWNER_LAYER}' is not an object layer, so it has no spawn points");
            continue;
        };
        for object in object_layer.objects() {
            if object.visible && object.user_type.eq_ignore_ascii_case("spawn") {
                points.push(SpawnPoint {
                    name: object.name.clone(),
                    position: Vec2::new(object.x, map_height_px - object.y),
                });
            }
        }
    }

    points
}
