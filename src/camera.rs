use crate::state::AppState;
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    math::vec2,
    prelude::*,
    render::camera::CameraProjection,
    window::PrimaryWindow,
};
use bevy_ecs_tilemap::prelude::*;
// use bevy_ecs_tilemap::tiles::TilePos;
// use bevy_ecs_tilemap::map::TilemapTileSize;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::render::camera::CameraRenderGraph;
use bevy::render::primitives::Frustum;
use bevy::render::view::VisibleEntities;
use bevy_ecs_tilemap::map::TilemapType;
use bevy_inspector_egui::*;
use crate::{GameInfoAlt, helpers};

/// Plugin that adds the necessary systems for `PanCam` components to work
#[derive(Default)]
pub struct PanCamPlugin;

/// System set to allow ordering of `PanCamPlugin`
#[derive(Debug, Clone, Copy, SystemSet, PartialEq, Eq, Hash)]
pub struct PanCamSystemSet;

/// Used to help identify our main camera
#[derive(Component)]
pub struct MainCamera;

impl Plugin for PanCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Level), camera_spawn)
            .add_systems(
                Update,
                (camera_movement, camera_zoom)
                    .in_set(PanCamSystemSet)
                    .run_if(in_state(AppState::Level)),
            )
            .register_type::<PanCam>();

        //#[cfg(feature = "bevy_egui")]
        {
            app.init_resource::<EguiWantsFocus>()
                .add_systems(PostUpdate, check_egui_wants_focus)
                .configure_sets(
                    Update,
                    PanCamSystemSet
                        .run_if(resource_equals(EguiWantsFocus(false)))
                        .run_if(in_state(AppState::Level)),
                );
        }
    }
}

#[derive(Resource, Deref, DerefMut, PartialEq, Eq, Default)]
//#[cfg(feature = "bevy_egui")]
struct EguiWantsFocus(bool);

// todo: make run condition when Bevy supports mutable resources in them
//#[cfg(feature = "bevy_egui")]
fn check_egui_wants_focus(
    mut contexts: Query<&mut bevy_egui::EguiContext>,
    mut wants_focus: ResMut<EguiWantsFocus>,
) {
    let ctx = contexts.iter_mut().next();
    let new_wants_focus = if let Some(ctx) = ctx {
        let ctx = ctx.into_inner().get_mut();
        ctx.wants_pointer_input() || ctx.wants_keyboard_input()
    } else {
        false
    };
    wants_focus.set_if_neq(EguiWantsFocus(new_wants_focus));
}

fn camera_zoom(
    mut query: Query<(&PanCam, &mut OrthographicProjection, &mut Transform)>,
    mut scroll_events: EventReader<MouseWheel>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let pixels_per_line = 100.; // Maybe make configurable?
    let scroll = scroll_events
        .read()
        .map(|ev| match ev.unit {
            MouseScrollUnit::Pixel => ev.y,
            MouseScrollUnit::Line => ev.y * pixels_per_line,
        })
        .sum::<f32>();

    if scroll == 0. {
        return;
    }

    let window = primary_window.single();
    let window_size = Vec2::new(window.width(), window.height());
    let mouse_normalized_screen_pos = window
        .cursor_position()
        .map(|cursor_pos| (cursor_pos / window_size) * 2. - Vec2::ONE)
        .map(|p| Vec2::new(p.x, -p.y));

    for (cam, mut proj, mut pos) in &mut query {
        if cam.enabled {
            let old_scale = proj.scale;
            proj.scale = (proj.scale * (1. + -scroll * 0.001)).max(cam.min_scale);

            // Apply max scale constraint
            if let Some(max_scale) = cam.max_scale {
                proj.scale = proj.scale.min(max_scale);
            }

            // If there is both a min and max boundary, that limits how far we can zoom. Make sure we don't exceed that
            let scale_constrained = BVec2::new(
                cam.min_x.is_some() && cam.max_x.is_some(),
                cam.min_y.is_some() && cam.max_y.is_some(),
            );

            if scale_constrained.x || scale_constrained.y {
                let bounds_width = if let (Some(min_x), Some(max_x)) = (cam.min_x, cam.max_x) {
                    max_x - min_x
                } else {
                    f32::INFINITY
                };

                let bounds_height = if let (Some(min_y), Some(max_y)) = (cam.min_y, cam.max_y) {
                    max_y - min_y
                } else {
                    f32::INFINITY
                };

                let bounds_size = vec2(bounds_width, bounds_height);
                let max_safe_scale = max_scale_within_bounds(bounds_size, &proj, window_size);

                if scale_constrained.x {
                    proj.scale = proj.scale.min(max_safe_scale.x);
                }

                if scale_constrained.y {
                    proj.scale = proj.scale.min(max_safe_scale.y);
                }
            }

            // Move the camera position to normalize the projection window
            if let (Some(mouse_normalized_screen_pos), true) =
                (mouse_normalized_screen_pos, cam.zoom_to_cursor)
            {
                let proj_size = proj.area.max / old_scale;
                let mouse_world_pos = pos.translation.truncate()
                    + mouse_normalized_screen_pos * proj_size * old_scale;
                pos.translation = (mouse_world_pos
                    - mouse_normalized_screen_pos * proj_size * proj.scale)
                    .extend(pos.translation.z);

                // As we zoom out, we don't want the viewport to move beyond the provided boundary. If the most recent
                // change to the camera zoom would move cause parts of the window beyond the boundary to be shown, we
                // need to change the camera position to keep the viewport within bounds. The four if statements below
                // provide this behavior for the min and max x and y boundaries.
                let proj_size = proj.area.size();

                let half_of_viewport = proj_size / 2.;

                if let Some(min_x_bound) = cam.min_x {
                    let min_safe_cam_x = min_x_bound + half_of_viewport.x;
                    pos.translation.x = pos.translation.x.max(min_safe_cam_x);
                }
                if let Some(max_x_bound) = cam.max_x {
                    let max_safe_cam_x = max_x_bound - half_of_viewport.x;
                    pos.translation.x = pos.translation.x.min(max_safe_cam_x);
                }
                if let Some(min_y_bound) = cam.min_y {
                    let min_safe_cam_y = min_y_bound + half_of_viewport.y;
                    pos.translation.y = pos.translation.y.max(min_safe_cam_y);
                }
                if let Some(max_y_bound) = cam.max_y {
                    let max_safe_cam_y = max_y_bound - half_of_viewport.y;
                    pos.translation.y = pos.translation.y.min(max_safe_cam_y);
                }
            }
        }
    }
}

/// max_scale_within_bounds is used to find the maximum safe zoom out/projection
/// scale when we have been provided with minimum and maximum x boundaries for
/// the camera.
fn max_scale_within_bounds(
    bounds_size: Vec2,
    proj: &OrthographicProjection,
    window_size: Vec2, //viewport?
) -> Vec2 {
    let mut p = proj.clone();
    p.scale = 1.;
    p.update(window_size.x, window_size.y);
    let base_world_size = p.area.size();
    bounds_size / base_world_size
}

fn camera_movement(
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mouse_buttons: Res<Input<MouseButton>>,
    mut query: Query<(&PanCam, &mut Transform, &OrthographicProjection)>,
    mut last_pos: Local<Option<Vec2>>,
) {
    let window = primary_window.single();
    let window_size = Vec2::new(window.width(), window.height());

    // Use position instead of MouseMotion, otherwise we don't get acceleration movement
    let current_pos = match window.cursor_position() {
        Some(c) => Vec2::new(c.x, -c.y),
        None => return,
    };
    let delta_device_pixels = current_pos - last_pos.unwrap_or(current_pos);

    for (cam, mut transform, projection) in &mut query {
        if cam.enabled
            && cam
                .grab_buttons
                .iter()
                .any(|btn| mouse_buttons.pressed(*btn) && !mouse_buttons.just_pressed(*btn))
        {
            let proj_size = projection.area.size();

            let world_units_per_device_pixel = proj_size / window_size;

            // The proposed new camera position
            let delta_world = delta_device_pixels * world_units_per_device_pixel;
            let mut proposed_cam_transform = transform.translation - delta_world.extend(0.);

            // Check whether the proposed camera movement would be within the provided boundaries, override it if we
            // need to do so to stay within bounds.
            if let Some(min_x_boundary) = cam.min_x {
                let min_safe_cam_x = min_x_boundary + proj_size.x / 2.;
                proposed_cam_transform.x = proposed_cam_transform.x.max(min_safe_cam_x);
            }
            if let Some(max_x_boundary) = cam.max_x {
                let max_safe_cam_x = max_x_boundary - proj_size.x / 2.;
                proposed_cam_transform.x = proposed_cam_transform.x.min(max_safe_cam_x);
            }
            if let Some(min_y_boundary) = cam.min_y {
                let min_safe_cam_y = min_y_boundary + proj_size.y / 2.;
                proposed_cam_transform.y = proposed_cam_transform.y.max(min_safe_cam_y);
            }
            if let Some(max_y_boundary) = cam.max_y {
                let max_safe_cam_y = max_y_boundary - proj_size.y / 2.;
                proposed_cam_transform.y = proposed_cam_transform.y.min(max_safe_cam_y);
            }

            transform.translation = proposed_cam_transform;
        }
    }
    *last_pos = Some(current_pos);
}

// fn camera_setup(
//     primary_window: Query<&Window, With<PrimaryWindow>>,
//     mut query: Query<(&PanCam, &mut Transform, &OrthographicProjection)>,
//     mut last_pos: Local<Option<Vec2>>,
// ) {
//     info!("setup camera");
//     let window = primary_window.single();
//     for (cam, mut transform, projection) in &mut query {
//         if cam.enabled {
//             let proj_size = projection.area.size();
//             let mut proposed_cam_transform = transform.translation;
//
//             // Check whether the proposed camera movement would be within the provided boundaries, override it if we
//             // need to do so to stay within bounds.
//             if let Some(min_x_boundary) = cam.min_x {
//                 let min_safe_cam_x = min_x_boundary + proj_size.x / 2.;
//                 proposed_cam_transform.x = proposed_cam_transform.x.max(min_safe_cam_x);
//             }
//             if let Some(max_x_boundary) = cam.max_x {
//                 let max_safe_cam_x = max_x_boundary - proj_size.x / 2.;
//                 proposed_cam_transform.x = proposed_cam_transform.x.min(max_safe_cam_x);
//             }
//             if let Some(min_y_boundary) = cam.min_y {
//                 let min_safe_cam_y = min_y_boundary + proj_size.y / 2.;
//                 proposed_cam_transform.y = proposed_cam_transform.y.max(min_safe_cam_y);
//             }
//             if let Some(max_y_boundary) = cam.max_y {
//                 let max_safe_cam_y = max_y_boundary - proj_size.y / 2.;
//                 proposed_cam_transform.y = proposed_cam_transform.y.min(max_safe_cam_y);
//             }
//
//             transform.translation = proposed_cam_transform;
//         }
//     }
//
//     // Use position instead of MouseMotion, otherwise we don't get acceleration movement
//     {
//         let current_pos = match window.cursor_position() {
//             Some(c) => Vec2::new(c.x, -c.y),
//             None => return,
//         };
//         *last_pos = Some(current_pos);
//     }
// }

/// A component that adds panning camera controls to an orthographic camera
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct PanCam {
    /// The mouse buttons that will be used to drag and pan the camera
    pub grab_buttons: Vec<MouseButton>,
    /// Whether camera currently responds to user input
    pub enabled: bool,
    /// When true, zooming the camera will center on the mouse cursor
    ///
    /// When false, the camera will stay in place, zooming towards the
    /// middle of the screen
    pub zoom_to_cursor: bool,
    /// The minimum scale for the camera
    ///
    /// The orthographic projection's scale will be clamped at this value when zooming in
    pub min_scale: f32,
    /// The maximum scale for the camera
    ///
    /// If present, the orthographic projection's scale will be clamped at
    /// this value when zooming out.
    pub max_scale: Option<f32>,
    /// The minimum x position of the camera window
    ///
    /// If present, the orthographic projection will be clamped to this boundary both
    /// when dragging the window, and zooming out.
    pub min_x: Option<f32>,
    /// The maximum x position of the camera window
    ///
    /// If present, the orthographic projection will be clamped to this boundary both
    /// when dragging the window, and zooming out.
    pub max_x: Option<f32>,
    /// The minimum y position of the camera window
    ///
    /// If present, the orthographic projection will be clamped to this boundary both
    /// when dragging the window, and zooming out.
    pub min_y: Option<f32>,
    /// The maximum y position of the camera window
    ///
    /// If present, the orthographic projection will be clamped to this boundary both
    /// when dragging the window, and zooming out.
    pub max_y: Option<f32>,
}

/// Create an orthographic projection camera with a custom `Z` position.
///
/// The camera is placed at `Z=far-0.1`, looking toward the world origin `(0,0,0)`.
/// Its orthographic projection extends from `0.0` to `-far` in camera view space,
/// corresponding to `Z=far-0.1` (closest to camera) to `Z=-0.1` (furthest away from
/// camera) in world space.
pub fn new_camera2d_with_constraints(pancam: &PanCam, pos: &Vec3) -> Camera2dBundle {

    // we want 0 to be "closest" and +far to be "farthest" in 2d, so we offset
    // the camera's translation by far and use a right handed coordinate system
    let projection = OrthographicProjection {
        scale: 0.5,
        ..default()
    };
    let transform = Transform::from_xyz(pos.x, pos.y, 1000.0 - 0.1);
    let view_projection =
        projection.get_projection_matrix() * transform.compute_matrix().inverse();
    let frustum = Frustum::from_view_projection_custom_far(
        &view_projection,
        &transform.translation,
        &transform.back(),
        projection.far(),
    );

    let proj_size = projection.area.size();
    let mut proposed_cam_transform = transform;

    // clamp to the given boundaries
    {
        let min_x_boundary = pancam.min_x.unwrap_or_else(|| f32::MIN);
        let max_x_boundary = pancam.max_x.unwrap_or_else(|| f32::MAX);
        let min_y_boundary = pancam.min_y.unwrap_or_else(|| f32::MIN);
        let max_y_boundary = pancam.max_y.unwrap_or_else(|| f32::MAX);

        let min_safe_cam_x = min_x_boundary + proj_size.x / 2.;
        let max_safe_cam_x = max_x_boundary - proj_size.x / 2.;
        proposed_cam_transform.translation.x = proposed_cam_transform.translation.x.clamp(min_safe_cam_x, max_safe_cam_x);

        let min_safe_cam_y = min_y_boundary + proj_size.y / 2.;
        let max_safe_cam_y = max_y_boundary - proj_size.y / 2.;
        proposed_cam_transform.translation.y = proposed_cam_transform.translation.y.clamp(min_safe_cam_y, max_safe_cam_y);
    }

    Camera2dBundle {
        projection,
        frustum,
        transform: proposed_cam_transform,
        ..default()
    }
}

fn camera_spawn(
    mut commands: Commands,
    game_info: Res<GameInfoAlt>,
    tile_maps: Res<Assets<helpers::tiled::TiledMap>>,
) {
    info!("camera_spawn");

    let mut camera_pos = Vec3::default();
    let mut map_size = Vec2::default();
    if let Some(map) = tile_maps.get(&game_info.tile_map) {
        map_size = Vec2::new(
            ((map.map.width - 1) * map.map.tile_width) as f32,
            ((map.map.height - 1) * map.map.tile_height) as f32,
        );

        let tile_size = TilemapTileSize { x: map.map.tile_width as f32, y: map.map.tile_height as f32 };
        let grid_size = tile_size.into();

        let map_type = TilemapType::Square;

        let low = TilePos::new(0, 0).center_in_world(&grid_size, &map_type);
        let high = TilePos::new(map.map.width - 1, map.map.height - 1).center_in_world(&grid_size, &map_type);
        let diff = high - low;

        let xform = Transform::from_xyz(diff.x / 2., diff.y / 2., 0.);
        camera_pos = xform.translation;
    }
    else {
        warn!("can't find tile map for camera setup!")
    }

    let pancam = PanCam {
        min_scale: 0.25,
        max_scale: Some(30.),
        //max_x: None,
        //max_y: None,
        min_x: Some(0.),
        min_y: Some(0.),
        max_x: Some(map_size.x),
        max_y: Some(map_size.y),
        ..default()
    };
    let cam2d = new_camera2d_with_constraints(&pancam, &camera_pos);

    // spawn the camera system
    commands.spawn((cam2d, pancam, MainCamera));
}

impl Default for PanCam {
    fn default() -> Self {
        Self {
            grab_buttons: vec![MouseButton::Left, MouseButton::Right, MouseButton::Middle],
            enabled: true,
            zoom_to_cursor: true,
            min_scale: 0.00001,
            max_scale: None,
            min_x: None,
            max_x: None,
            min_y: None,
            max_y: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::INFINITY;

    use bevy::prelude::OrthographicProjection;

    use super::*;

    /// Simple mock function to construct a square projection from a window size
    fn mock_proj(window_size: Vec2) -> OrthographicProjection {
        let mut proj = Camera2dBundle::default().projection;
        proj.update(window_size.x, window_size.y);
        proj
    }

    #[test]
    fn bounds_matching_window_width_have_max_scale_1() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(100., INFINITY), &proj, window_size).x,
            1.
        );
    }

    // boundaries are 1/2 the size of the projection window
    #[test]
    fn bounds_half_of_window_width_have_half_max_scale() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(50., INFINITY), &proj, window_size).x,
            0.5
        );
    }

    // boundaries are 2x the size of the projection window
    #[test]
    fn bounds_twice_of_window_width_have_max_scale_2() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(200., INFINITY), &proj, window_size).x,
            2.
        );
    }

    #[test]
    fn bounds_matching_window_height_have_max_scale_1() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(INFINITY, 100.), &proj, window_size).y,
            1.
        );
    }

    // boundaries are 1/2 the size of the projection window
    #[test]
    fn bounds_half_of_window_height_have_half_max_scale() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(INFINITY, 50.), &proj, window_size).y,
            0.5
        );
    }

    // boundaries are 2x the size of the projection window
    #[test]
    fn bounds_twice_of_window_height_have_max_scale_2() {
        let window_size = vec2(100., 100.);
        let proj = mock_proj(window_size);
        assert_eq!(
            max_scale_within_bounds(vec2(INFINITY, 200.), &proj, window_size).y,
            2.
        );
    }
}
