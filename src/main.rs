//! Renders an animated sprite by loading all animation frames from a single image (a sprite sheet)
//! into a texture atlas, and changing the displayed image periodically.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_asset_loader::asset_collection::AssetCollection;
use bevy_asset_loader::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_pancam::PanCamPlugin;

use bevy_inspector_egui::bevy_egui::{EguiContext, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use bevy_inspector_egui::prelude::*;

use camera::{CameraPlugin, MainCamera};
use grid::{CollisionMap, GridMotion, GridPlugin, GridPos};
use helpers::tiled::TiledMapHandle;
use state::AppState;

mod camera;
mod grid;
mod helpers;
mod state;

#[derive(Reflect, Resource, Default)]
struct WorldPosition(Vec2);

#[derive(Component)]
pub struct MainPlayer;

#[derive(AssetCollection, Resource)]
struct GameInfoAlt {
    #[asset(key = "image.creatures")]
    creature_image: Handle<Image>,
    #[asset(key = "atlas.creatures")]
    creature_layout: Handle<TextureAtlasLayout>,
    #[asset(key = "map.main")]
    tile_map: Handle<helpers::tiled::TiledMap>,
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()), // prevents blurry sprites
            bevy_inspector_egui::DefaultInspectorConfigPlugin,
            EguiPlugin::default(),
            PanCamPlugin,
            CameraPlugin,
            GridPlugin,
            TilemapPlugin,
            helpers::tiled::TiledMapPlugin,
        ))
        .init_resource::<Configuration>()
        .init_resource::<WorldPosition>()
        .register_type::<WorldPosition>()
        .register_type::<Configuration>()
        .init_state::<AppState>()
        .add_loading_state(
            LoadingState::new(AppState::Loading)
                .continue_to_state(AppState::Level)
                .with_dynamic_assets_file::<StandardDynamicAssetCollection>("main.assets.ron")
                .load_collection::<GameInfoAlt>(),
        )
        .add_systems(OnEnter(AppState::Level), spawn_level)
        .add_systems(Update, animate_sprite.run_if(in_state(AppState::Level)))
        .add_systems(
            Update,
            update_mouse_position.run_if(in_state(AppState::Level)),
        )
        // egui UI has to run in the egui pass, not `Update`.
        .add_systems(
            EguiPrimaryContextPass,
            inspector_ui.run_if(in_state(AppState::Level)),
        )
        .add_systems(Update, player_movement.run_if(in_state(AppState::Level)))
        // Not state-gated, so escape also works while the level is still loading.
        .add_systems(Update, exit_on_escape)
        .run();
}

#[derive(Reflect, Resource, InspectorOptions)]
#[reflect(Resource, InspectorOptions)]
struct Configuration {
    name: String,
    #[inspector(min = 0.0, max = 1.0)]
    option: f32,
    mouse_position: WorldPosition,
    cursor_in_map_pos: Vec2,
    /// Lets the mouse pan and zoom the camera. Off by default so that stray clicks
    /// and drags during play don't move the view.
    debug_camera: bool,
    /// Seconds an entity takes to slide from one cell to the next.
    #[inspector(min = 0.0, max = 1.0)]
    move_duration: f32,
    /// How gradually a slide departs. 0 leaves the start abrupt.
    #[inspector(min = 0.0, max = 1.0)]
    move_ease_in: f32,
    /// How gradually a slide arrives. 0 leaves the finish abrupt.
    #[inspector(min = 0.0, max = 1.0)]
    move_ease_out: f32,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            name: String::new(),
            option: 0.,
            mouse_position: WorldPosition::default(),
            cursor_in_map_pos: Vec2::ZERO,
            debug_camera: false,
            // Short enough to stay responsive when a direction is tapped repeatedly.
            move_duration: 0.12,
            // Matches CSS `ease-in-out`; see `grid::motion_curve`.
            move_ease_in: 0.42,
            move_ease_out: 0.42,
        }
    }
}

#[derive(Component)]
struct AnimationFrame(i32);

#[derive(Component, Deref, DerefMut)]
struct AnimationTimer(Timer);

fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(&mut AnimationFrame, &mut AnimationTimer, &mut Sprite)>,
) {
    for (mut frame, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());
        if timer.just_finished() {
            frame.0 += 1;
            if frame.0 == 2 as i32 {
                frame.0 = 0
            }
            if let Some(atlas) = &mut sprite.texture_atlas {
                atlas.index = ([22, 42])[frame.0 as usize]
            }
        }
    }
}

fn update_mouse_position(
    mut config: ResMut<Configuration>,
    // query to get the window (so we can read the current cursor position)
    q_window: Query<&Window, With<PrimaryWindow>>,
    // query to get camera transform
    q_camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    tilemap_q: Query<(
        &TilemapSize,
        &TilemapGridSize,
        &TilemapTileSize,
        &TilemapType,
        &TilemapAnchor,
        &Transform,
    )>,
) {
    // get the camera info and transform
    // assuming there is exactly one main camera entity, so Query::single() is OK
    let Ok((camera, camera_transform)) = q_camera.single() else {
        return;
    };

    // There is only one primary window, so we can similarly get it from the query:
    let Ok(window) = q_window.single() else {
        return;
    };

    // check if the cursor is inside the window and get its position
    // then, ask bevy to convert into world coordinates
    if let Some(world_position) = window
        .cursor_position()
        .and_then(|cursor| camera.viewport_to_world_2d(camera_transform, cursor).ok())
    {
        config.mouse_position.0 = world_position;
    }

    // run this block _AFTER_ the cursor position is calculated above
    for (map_size, grid_size, tile_size, map_type, anchor, map_transform) in tilemap_q.iter() {
        // Grab the cursor position from the `Res<CursorPos>`
        let cursor_pos: Vec2 = config.mouse_position.0;
        // We need to make sure that the cursor's world position is correct relative to the map
        // due to any map transformation.
        let cursor_in_map_pos: Vec2 = {
            // Extend the cursor_pos vec3 by 0.0 and 1.0
            let cursor_pos = Vec4::from((cursor_pos, 0.0, 1.0));
            let cursor_in_map_pos = map_transform.to_matrix().inverse() * cursor_pos;
            cursor_in_map_pos.xy()
        };

        // Once we have a world position we can transform it into a possible tile position.
        if let Some(tile_pos) = TilePos::from_world_pos(
            &cursor_in_map_pos,
            map_size,
            grid_size,
            tile_size,
            map_type,
            anchor,
        ) {
            config.cursor_in_map_pos = Vec2::new(tile_pos.x as f32, tile_pos.y as f32);
        }
    }
}

fn inspector_ui(world: &mut World) {
    let Ok(egui_context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single(world)
    else {
        return;
    };
    let mut egui_context = egui_context.clone();

    egui::Window::new("Resource Inspector").show(egui_context.get_mut(), |ui| {
        egui::ScrollArea::both().show(ui, |ui| {
            bevy_inspector_egui::bevy_inspector::ui_for_resource::<Configuration>(world, ui);
        });
    });
}

fn spawn_level(
    mut commands: Commands,
    game_info: Res<GameInfoAlt>,
    tile_maps: Res<Assets<helpers::tiled::TiledMap>>,
) {
    info!("spawn_level");

    commands.spawn(helpers::tiled::TiledMapBundle {
        tiled_map: TiledMapHandle(game_info.tile_map.clone()),
        transform: Transform::from_scale(Vec3::splat(1.0))
            .with_translation(Vec3::new(0.0, 0.0, 0.1)),
        ..Default::default()
    });

    // spawn characters
    if let Some(map) = tile_maps.get(&game_info.tile_map) {
        let collision = CollisionMap::from_map(map);

        info!("spawn objects");
        for spawn in helpers::spawn_points(map) {
            info!("spawning {}", spawn.name);

            // Snap to the containing cell, so an object left slightly off the grid in
            // the editor still lines up with the tiles.
            let cell = collision.world_to_cell(spawn.position);

            commands.spawn((
                Sprite::from_atlas_image(
                    game_info.creature_image.clone(),
                    TextureAtlas {
                        layout: game_info.creature_layout.clone(),
                        index: 22,
                    },
                ),
                Transform::from_translation(collision.cell_to_world(cell).extend(2.0)),
                GridPos(cell),
                AnimationFrame(0),
                AnimationTimer(Timer::from_seconds(0.2, TimerMode::Repeating)),
                MainPlayer,
            ));
        }

        commands.insert_resource(collision);
    } else {
        error!("no tile map, so no collision and no characters");
    }
}

fn player_movement(
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    config: Res<Configuration>,
    collision: Option<Res<CollisionMap>>,
    // `Without<GridMotion>` means a keypress during a slide is ignored rather than
    // queued, so the player can't outrun the animation.
    mut query: Query<(Entity, &mut GridPos), (With<MainPlayer>, Without<GridMotion>)>,
) {
    let Some(collision) = collision else {
        return;
    };
    let move_input = {
        let mut p = IVec2::ZERO;

        if input.just_pressed(KeyCode::Numpad1) || input.just_pressed(KeyCode::KeyZ) {
            p.x = -1;
            p.y = -1;
        }
        if input.just_pressed(KeyCode::Numpad2)
            || input.just_pressed(KeyCode::KeyX)
            || input.just_pressed(KeyCode::ArrowDown)
        {
            p.y = -1;
        }
        if input.just_pressed(KeyCode::Numpad3) || input.just_pressed(KeyCode::KeyC) {
            p.x = 1;
            p.y = -1;
        }
        if input.just_pressed(KeyCode::Numpad4)
            || input.just_pressed(KeyCode::KeyA)
            || input.just_pressed(KeyCode::ArrowLeft)
        {
            p.x = -1;
        }
        if input.just_pressed(KeyCode::Numpad6)
            || input.just_pressed(KeyCode::KeyD)
            || input.just_pressed(KeyCode::ArrowRight)
        {
            p.x = 1;
        }
        if input.just_pressed(KeyCode::Numpad7) || input.just_pressed(KeyCode::KeyQ) {
            p.x = -1;
            p.y = 1;
        }
        if input.just_pressed(KeyCode::Numpad8)
            || input.just_pressed(KeyCode::KeyW)
            || input.just_pressed(KeyCode::ArrowUp)
        {
            p.y = 1;
        }
        if input.just_pressed(KeyCode::Numpad9) || input.just_pressed(KeyCode::KeyE) {
            p.x = 1;
            p.y = 1;
        }
        p
    };

    if move_input.cmpeq(IVec2::ZERO).all() {
        return;
    }

    for (entity, mut grid_pos) in &mut query {
        grid::try_step(
            &mut commands,
            entity,
            &mut grid_pos,
            move_input,
            &collision,
            &config,
        );
    }
}

/// Quits on escape, via `AppExit` so bevy gets to shut down cleanly rather than the
/// process being torn down under it.
fn exit_on_escape(
    input: Res<ButtonInput<KeyCode>>,
    mut egui_contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !input.just_pressed(KeyCode::Escape) {
        return;
    }

    // Don't quit out from under the inspector: egui uses escape to cancel out of a
    // focused text field, and `Configuration::name` is one.
    if let Ok(mut context) = egui_contexts.single_mut() {
        if context.get_mut().egui_wants_keyboard_input() {
            return;
        }
    }

    info!("escape pressed, exiting");
    exit.write(AppExit::Success);
}
