//! Movement on the tile grid.
//!
//! Entities occupy whole cells. `GridPos` is the authoritative position and `Transform`
//! is derived from it, so an entity is never left between cells: a move commits to the
//! destination immediately and the slide is presentation only.

use bevy::asset::saver::{AssetSaver, SavedAsset};
use bevy::asset::{AsyncWriteExt, ErasedLoadedAsset, LoadedAsset};
use bevy::math::cubic_splines::CubicSegment;
use bevy::prelude::*;
use bevy::tasks::block_on;
use bevy_common_assets::ron::{RonAssetPlugin, RonAssetSaver};
use serde::{Deserialize, Serialize};

use crate::helpers::tiled::TiledMap;
use crate::state::AppState;
use crate::{Configuration, GameInfoAlt};

/// The Tiled tile class ("Class" in the tileset editor, `type=` in the TSX) that marks a
/// cell as impassable.
///
/// Per-tile collision shapes are deliberately *not* consulted. They can describe things
/// other than walls, so treating any shape as solid would make unrelated authoring
/// silently blocking.
const WALL_CLASS: &str = "wall";

/// Initial values for grid movement, loaded from `assets/main.movement.ron`.
///
/// These only seed `Configuration` when the level starts; the inspector owns the live
/// values from then on, so retuning in-game doesn't write back to the file.
#[derive(Asset, TypePath, Debug, Clone, Deserialize, Serialize)]
pub struct MovementSettings {
    /// Seconds an entity takes to slide from one cell to the next.
    pub duration: f32,
    /// How gradually a slide departs. 0 leaves the start abrupt.
    pub ease_in: f32,
    /// How gradually a slide arrives. 0 leaves the finish abrupt.
    pub ease_out: f32,
}

impl Default for MovementSettings {
    fn default() -> Self {
        Self {
            // Short enough to stay responsive when a direction is tapped repeatedly.
            duration: 0.12,
            // Matches CSS `ease-in-out`; see `motion_curve`.
            ease_in: 0.42,
            ease_out: 0.42,
        }
    }
}

/// Copies the loaded movement settings into `Configuration` as the level starts.
fn apply_movement_settings(
    game_info: Res<GameInfoAlt>,
    settings: Res<Assets<MovementSettings>>,
    mut config: ResMut<Configuration>,
) {
    let Some(settings) = settings.get(&game_info.movement) else {
        warn!("no movement settings asset; keeping the built-in defaults");
        return;
    };

    info!(?settings, "applying movement settings");
    config.move_duration = settings.duration;
    config.move_ease_in = settings.ease_in;
    config.move_ease_out = settings.ease_out;
}

/// Asks for the live values in `Configuration` to be written back over the movement
/// settings file.
#[derive(Message, Default)]
pub struct SaveMovementSettings;

/// Prepended to the file on every save.
///
/// `RonAssetSaver` emits only the serialized value, so anything explanatory has to be
/// written alongside it -- and because the whole file is rewritten, notes added here by
/// hand are lost on the next save.
const FILE_HEADER: &str = "\
// Initial values for grid movement.
//
// These seed `Configuration` when the level starts. Saving from the inspector rewrites
// this file, so hand-written notes here will not survive a round trip.
//
//   duration   seconds an entity takes to slide from one cell to the next
//   ease_in    delays the departure of that slide, 0..=1
//   ease_out   softens its arrival, 0..=1
//
// ease_in / ease_out pairs, as CSS `cubic-bezier` equivalents:
//
//   0.0  / 0.0    linear
//   0.42 / 0.42   ease-in-out
//   0.42 / 0.0    ease-in
//   0.0  / 0.42   ease-out
";

/// Writes `Configuration`'s movement values back over the settings file.
///
/// This runs synchronously. The payload is a few dozen bytes, and blocking briefly in a
/// debug tool is easier to reason about than a detached task whose failure would surface
/// somewhere unrelated -- or not at all.
fn save_movement_settings(
    mut requests: MessageReader<SaveMovementSettings>,
    asset_server: Res<AssetServer>,
    game_info: Res<GameInfoAlt>,
    config: Res<Configuration>,
) {
    // Coalesced, so several clicks in one frame still write once.
    if requests.read().count() == 0 {
        return;
    }

    // Resolved from the handle rather than hardcoded, so this follows the path if the
    // `settings.movement` key in `main.assets.ron` is ever repointed.
    let Some(asset_path) = asset_server.get_path(&game_info.movement) else {
        error!("movement settings have no source path; not saving");
        return;
    };
    let source_id = asset_path.source().clone_owned();
    let path = asset_path.path().to_path_buf();

    let source = match asset_server.get_source(&source_id) {
        Ok(source) => source,
        Err(err) => {
            error!("no asset source to save movement settings to: {err}");
            return;
        }
    };

    let settings = MovementSettings {
        duration: config.move_duration,
        ease_in: config.move_ease_in,
        ease_out: config.move_ease_out,
    };
    let loaded: ErasedLoadedAsset = LoadedAsset::from(settings.clone()).into();
    let Some(saved) = SavedAsset::<MovementSettings>::from_loaded(&loaded) else {
        error!("could not wrap movement settings for saving");
        return;
    };

    let result: Result<(), Box<dyn core::error::Error>> = block_on(async {
        let mut writer = source.writer()?.write(&path).await?;
        writer.write_all(FILE_HEADER.as_bytes()).await?;
        RonAssetSaver::<MovementSettings>::default()
            .save(&mut *writer, saved, &(), asset_path.clone())
            .await?;
        // `to_string` leaves no trailing newline.
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    });

    match result {
        Ok(()) => info!(?settings, "saved movement settings to {}", path.display()),
        Err(err) => error!("could not save movement settings: {err}"),
    }
}

/// Which cell an entity occupies.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPos(pub IVec2);

/// An in-progress slide between two cells. Only present while the entity is moving, so
/// `Without<GridMotion>` is how a system asks for "entities that are free to act".
#[derive(Component)]
pub struct GridMotion {
    from: Vec2,
    to: Vec2,
    elapsed: f32,
    duration: f32,
}

/// The solid cells of the level, baked once at load.
///
/// Colliders are not spawned as entities: at 28x40 this would be a thousand of them, and
/// a lookup answers the only question grid movement ever asks.
#[derive(Resource)]
pub struct CollisionMap {
    size: UVec2,
    tile_size: Vec2,
    solid: Vec<bool>,
}

impl CollisionMap {
    /// Bakes every tile layer in the map into a single solidity grid.
    ///
    /// Layers are OR-ed together, so a wall painted on any layer blocks.
    pub fn from_map(map: &TiledMap) -> Self {
        let size = UVec2::new(map.map.width, map.map.height);
        let tile_size = Vec2::new(map.map.tile_width as f32, map.map.tile_height as f32);
        let mut solid = vec![false; (size.x * size.y) as usize];
        let mut wall_count = 0;

        for layer in map.map.layers() {
            let ::tiled::LayerType::Tiles(tile_layer) = layer.layer_type() else {
                continue;
            };

            for row in 0..size.y {
                for x in 0..size.x {
                    let is_wall = tile_layer
                        .get_tile(x as i32, row as i32)
                        .and_then(|layer_tile| layer_tile.get_tile())
                        .and_then(|tile| tile.user_type.clone())
                        .is_some_and(|class| class.eq_ignore_ascii_case(WALL_CLASS));

                    if is_wall {
                        // Tiled numbers rows downward from the top of the map, bevy
                        // upward from the bottom; `helpers::tiled` flips them the same
                        // way when it places the tiles.
                        let y = size.y - 1 - row;
                        let cell = &mut solid[(y * size.x + x) as usize];
                        if !*cell {
                            *cell = true;
                            wall_count += 1;
                        }
                    }
                }
            }
        }

        if wall_count == 0 {
            warn!("no tiles with class '{WALL_CLASS}' in the map; nothing will block movement");
        } else {
            info!("baked {wall_count} solid cells");
        }

        Self {
            size,
            tile_size,
            solid,
        }
    }

    /// Whether `cell` blocks movement. Anything off the edge of the map counts as solid,
    /// so entities can't walk out of the level.
    pub fn is_solid(&self, cell: IVec2) -> bool {
        if cell.x < 0 || cell.y < 0 || cell.x >= self.size.x as i32 || cell.y >= self.size.y as i32
        {
            return true;
        }
        self.solid[(cell.y as u32 * self.size.x + cell.x as u32) as usize]
    }

    /// The world-space centre of a cell.
    ///
    /// `helpers::tiled` anchors the tilemap with `TilemapAnchor::None`, which puts the
    /// *centre* of cell (0,0) on the origin -- so this is a plain scale, with no
    /// half-tile correction.
    pub fn cell_to_world(&self, cell: IVec2) -> Vec2 {
        cell.as_vec2() * self.tile_size
    }

    /// The cell containing a world-space point.
    pub fn world_to_cell(&self, pos: Vec2) -> IVec2 {
        (pos / self.tile_size).round().as_ivec2()
    }
}

/// Starts a slide one cell in `direction`, unless the destination is solid.
///
/// The move commits to `GridPos` up front, so a second call before the slide finishes
/// steps on from the destination rather than from where the sprite happens to be.
/// Returns whether the step was taken.
pub fn try_step(
    commands: &mut Commands,
    entity: Entity,
    grid_pos: &mut GridPos,
    direction: IVec2,
    collision: &CollisionMap,
    config: &Configuration,
) -> bool {
    let target = grid_pos.0 + direction;
    if collision.is_solid(target) {
        return false;
    }

    // Sliding from the cell rather than from the current translation keeps rounding
    // error from accumulating over a long run of moves.
    let from = collision.cell_to_world(grid_pos.0);
    let to = collision.cell_to_world(target);
    grid_pos.0 = target;

    commands.entity(entity).insert(GridMotion {
        from,
        to,
        elapsed: 0.,
        duration: config.move_duration.max(0.),
    });

    true
}

/// The shaping curve for a slide, built from the two inspector knobs.
///
/// This is CSS `cubic-bezier` with the handles constrained to the time axis: `ease_in`
/// drags the start handle forward in time (a slower departure) and `ease_out` drags the
/// end handle back (a slower arrival). 0/0 is linear, and 0.42/0.42 reproduces CSS
/// `ease-in-out` exactly.
fn motion_curve(config: &Configuration) -> CubicSegment<Vec2> {
    CubicSegment::new_bezier_easing(
        Vec2::new(config.move_ease_in.clamp(0., 1.), 0.),
        Vec2::new(1. - config.move_ease_out.clamp(0., 1.), 1.),
    )
}

/// Drives in-progress slides, and drops `GridMotion` once an entity has arrived.
fn advance_grid_motion(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<Configuration>,
    mut query: Query<(Entity, &mut GridMotion, &mut Transform)>,
) {
    if query.is_empty() {
        return;
    }
    let curve = motion_curve(&config);

    for (entity, mut motion, mut transform) in &mut query {
        motion.elapsed += time.delta_secs();

        // A zero duration means "snap", and also keeps the division below safe.
        let t = if motion.duration > 0. {
            (motion.elapsed / motion.duration).clamp(0., 1.)
        } else {
            1.
        };

        let position = motion.from.lerp(motion.to, curve.ease(t));
        transform.translation.x = position.x;
        transform.translation.y = position.y;

        if t >= 1. {
            commands.entity(entity).remove::<GridMotion>();
        }
    }
}

/// Registers grid movement. The `CollisionMap` itself is inserted by `spawn_level`,
/// since it comes from the loaded map.
pub struct GridPlugin;

impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        // A distinctive extension rather than a bare `ron`, so this loader can't claim
        // files meant for something else. Bevy matches the longest extension first, so
        // `main.assets.ron` still reaches the asset-loader collection.
        app.add_plugins(RonAssetPlugin::<MovementSettings>::new(&["movement.ron"]))
            .add_message::<SaveMovementSettings>()
            .add_systems(OnEnter(AppState::Level), apply_movement_settings)
            .add_systems(
                Update,
                (advance_grid_motion, save_movement_settings)
                    .run_if(in_state(AppState::Level)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ease_in: f32, ease_out: f32) -> Configuration {
        Configuration {
            move_ease_in: ease_in,
            move_ease_out: ease_out,
            ..default()
        }
    }

    fn collision_map(size: UVec2, solid_cells: &[IVec2]) -> CollisionMap {
        let mut solid = vec![false; (size.x * size.y) as usize];
        for cell in solid_cells {
            solid[(cell.y as u32 * size.x + cell.x as u32) as usize] = true;
        }
        CollisionMap {
            size,
            tile_size: Vec2::splat(24.),
            solid,
        }
    }

    #[test]
    fn zero_easing_is_linear() {
        let curve = motion_curve(&config(0., 0.));
        for t in [0.25, 0.5, 0.75] {
            assert!((curve.ease(t) - t).abs() < 1e-3, "ease({t}) = {}", curve.ease(t));
        }
    }

    #[test]
    fn easing_is_symmetric_when_both_knobs_match() {
        let curve = motion_curve(&config(0.42, 0.42));
        assert!((curve.ease(0.5) - 0.5).abs() < 1e-3);
        // Eased-in-out means the midpoint is reached at the midpoint, but the first
        // quarter of the time covers less than a quarter of the distance.
        assert!(curve.ease(0.25) < 0.25);
        assert!(curve.ease(0.75) > 0.75);
    }

    #[test]
    fn easing_endpoints_are_exact() {
        for (i, o) in [(0., 0.), (0.42, 0.42), (1., 1.), (1., 0.)] {
            let curve = motion_curve(&config(i, o));
            assert_eq!(curve.ease(0.), 0.);
            assert_eq!(curve.ease(1.), 1.);
        }
    }

    #[test]
    fn cells_map_to_tile_centres_and_back() {
        let map = collision_map(UVec2::new(28, 40), &[]);
        assert_eq!(map.cell_to_world(IVec2::ZERO), Vec2::ZERO);
        assert_eq!(map.cell_to_world(IVec2::new(3, 36)), Vec2::new(72., 864.));
        assert_eq!(map.world_to_cell(Vec2::new(72., 864.)), IVec2::new(3, 36));
        // Anywhere inside the cell resolves to that cell.
        assert_eq!(map.world_to_cell(Vec2::new(83., 875.)), IVec2::new(3, 36));
    }

    #[test]
    fn off_map_is_solid() {
        let map = collision_map(UVec2::new(28, 40), &[]);
        assert!(map.is_solid(IVec2::new(-1, 0)));
        assert!(map.is_solid(IVec2::new(0, -1)));
        assert!(map.is_solid(IVec2::new(28, 0)));
        assert!(map.is_solid(IVec2::new(0, 40)));
        assert!(!map.is_solid(IVec2::new(27, 39)));
    }

    #[test]
    fn solid_cells_are_addressed_by_column_and_row() {
        // Both this cell and its transpose are inside a 28x40 map, so an x/y mix-up
        // shows up as a wrong-cell hit rather than an out-of-bounds one.
        let map = collision_map(UVec2::new(28, 40), &[IVec2::new(3, 20)]);
        assert!(map.is_solid(IVec2::new(3, 20)));
        assert!(!map.is_solid(IVec2::new(20, 3)));
        assert!(!map.is_solid(IVec2::new(4, 20)));
    }
}
