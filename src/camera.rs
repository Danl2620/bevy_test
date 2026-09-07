use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_pancam::PanCam;

use crate::state::AppState;
use crate::{helpers, GameInfoAlt};

/// Used to help identify our main camera
#[derive(Component)]
pub struct MainCamera;

/// Spawns the game camera once the level's assets are available.
///
/// Pan/zoom behaviour itself comes from `bevy_pancam::PanCamPlugin`, which also
/// handles suppressing camera input while egui wants the pointer or keyboard.
#[derive(Default)]
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Level), camera_spawn);
    }
}

fn camera_spawn(
    mut commands: Commands,
    game_info: Res<GameInfoAlt>,
    tile_maps: Res<Assets<helpers::tiled::TiledMap>>,
) {
    info!("camera_spawn");

    let mut camera_pos = Vec2::ZERO;
    let mut map_size = Vec2::ZERO;

    if let Some(map) = tile_maps.get(&game_info.tile_map) {
        map_size = Vec2::new(
            ((map.map.width - 1) * map.map.tile_width) as f32,
            ((map.map.height - 1) * map.map.tile_height) as f32,
        );

        let tilemap_size = TilemapSize {
            x: map.map.width,
            y: map.map.height,
        };
        let tile_size = TilemapTileSize {
            x: map.map.tile_width as f32,
            y: map.map.tile_height as f32,
        };
        let grid_size: TilemapGridSize = tile_size.into();
        let map_type = TilemapType::Square;
        // Must match the anchor the tile layers are spawned with in `helpers::tiled`.
        let anchor = TilemapAnchor::None;

        let low = TilePos::new(0, 0).center_in_world(
            &tilemap_size,
            &grid_size,
            &tile_size,
            &map_type,
            &anchor,
        );
        let high = TilePos::new(map.map.width - 1, map.map.height - 1).center_in_world(
            &tilemap_size,
            &grid_size,
            &tile_size,
            &map_type,
            &anchor,
        );
        camera_pos = (high - low) / 2.;
    } else {
        warn!("can't find tile map for camera setup!")
    }

    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 0.5,
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(camera_pos.x, camera_pos.y, 0.),
        PanCam {
            grab_buttons: vec![MouseButton::Left, MouseButton::Right, MouseButton::Middle],
            zoom_to_cursor: true,
            min_scale: 0.25,
            max_scale: 30.,
            min_x: 0.,
            min_y: 0.,
            max_x: map_size.x,
            max_y: map_size.y,
            ..default()
        },
        MainCamera,
    ));
}
