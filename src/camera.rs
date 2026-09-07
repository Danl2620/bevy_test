use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_pancam::{DirectionKeys, PanCam};

use crate::state::AppState;
use crate::{helpers, Configuration, GameInfoAlt};

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
        app.add_systems(OnEnter(AppState::Level), camera_spawn)
            .add_systems(Update, sync_debug_camera.run_if(in_state(AppState::Level)));
    }
}

fn camera_spawn(
    mut commands: Commands,
    game_info: Res<GameInfoAlt>,
    tile_maps: Res<Assets<helpers::tiled::TiledMap>>,
) {
    info!("camera_spawn");

    let map = tile_maps.get(&game_info.tile_map);
    if map.is_none() {
        warn!("can't find tile map for camera setup!");
    }

    // World-space rectangle the tiles actually cover. `TilemapAnchor::None` puts the
    // *centre* of tile (0,0) on the origin, so the artwork reaches half a tile further
    // out than the outermost tile centres in each direction.
    let map_area = map.map(|map| {
        let tile = Vec2::new(map.map.tile_width as f32, map.map.tile_height as f32);
        let far_centre = Vec2::new(
            (map.map.width - 1) as f32 * tile.x,
            (map.map.height - 1) as f32 * tile.y,
        );
        Rect::from_corners(-tile / 2., far_centre + tile / 2.)
    });

    // Open on the player's spawn point, falling back to the middle of the map if the
    // level doesn't define one.
    let camera_pos = match map.map(helpers::spawn_points) {
        Some(spawns) if !spawns.is_empty() => {
            info!("centering camera on '{}'", spawns[0].name);
            spawns[0].position
        }
        _ => {
            if map.is_some() {
                warn!("no spawn point in map; centering camera on the map instead");
            }
            map_area.map(|area| area.center()).unwrap_or(Vec2::ZERO)
        }
    };

    // Give the debug camera a full map of slack on every side: enough to push the level
    // right out to the edge of the view, not enough to lose it entirely.
    let bounds = map_area
        .map(|area| Rect::from_corners(area.min - area.size(), area.max + area.size()))
        .unwrap_or(Rect::from_corners(Vec2::NEG_INFINITY, Vec2::INFINITY));

    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 0.5,
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(camera_pos.x, camera_pos.y, 0.),
        PanCam {
            grab_buttons: vec![MouseButton::Left, MouseButton::Right, MouseButton::Middle],
            // The keyboard drives the player, so keep it away from the camera.
            move_keys: DirectionKeys::NONE,
            zoom_to_cursor: true,
            min_scale: 0.25,
            // Filled in by `sync_debug_camera` from the live window size.
            max_scale: f32::INFINITY,
            // Enabled by the `debug_camera` toggle only; see `sync_debug_camera`.
            enabled: false,
            min_x: bounds.min.x,
            min_y: bounds.min.y,
            max_x: bounds.max.x,
            max_y: bounds.max.y,
            ..default()
        },
        MainCamera,
    ));
}

/// Mirrors the `debug_camera` toggle in the inspector onto the camera, so mouse
/// panning and zooming stay inert during normal play, and keeps the zoom limit in
/// step with the window size.
fn sync_debug_camera(
    config: Res<Configuration>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut query: Query<(&mut PanCam, &mut Projection)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let window_size = window.size();
    if window_size.cmple(Vec2::ZERO).any() {
        return;
    }

    for (mut pan_cam, mut projection) in &mut query {
        // Each field is checked before assigning so this doesn't flag the component
        // as changed on every frame.
        if pan_cam.enabled != config.debug_camera {
            pan_cam.enabled = config.debug_camera;
        }

        // `PanCam` builds its safe zone by shrinking the bounds by the visible
        // half-extents, and `Aabb2d::shrink` trips a `debug_assert!` if that inverts.
        // So the view must never be allowed to grow larger than the bounds -- which
        // depends on the window, and the window can be resized at any time. At maximum
        // zoom-out the safe zone collapses to a point and the camera simply centres.
        let bounds_size = Vec2::new(pan_cam.max_x - pan_cam.min_x, pan_cam.max_y - pan_cam.min_y);
        let max_scale = (bounds_size / window_size).min_element();
        if pan_cam.max_scale != max_scale {
            pan_cam.max_scale = max_scale;
        }

        // A window that just grew can leave the current zoom above the new limit.
        if let Projection::Orthographic(ortho) = &mut *projection {
            if ortho.scale > max_scale {
                ortho.scale = max_scale;
            }
        }
    }
}
