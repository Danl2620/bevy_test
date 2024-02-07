//! Renders an animated sprite by loading all animation frames from a single image (a sprite sheet)
//! into a texture atlas, and changing the displayed image periodically.

use bevy::prelude::*;
use bevy_asset_loader::asset_collection::AssetCollection;
use bevy_asset_loader::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};

use bevy_inspector_egui::bevy_egui::{EguiContext, EguiPlugin};
use bevy_inspector_egui::bevy_inspector;
use bevy_inspector_egui::prelude::*;
use bevy_window::PrimaryWindow;

mod helpers;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum AppState {
    #[default]
    Loading,
    Level,
}

#[derive(Reflect, Resource, Default)]
struct WorldPosition(Vec2);

/// Used to help identify our main camera
#[derive(Component)]
struct MainCamera;

#[derive(AssetCollection, Resource)]
struct GameInfoAlt {
    #[asset(key = "atlas.creatures")]
    creature_atlas: Handle<TextureAtlas>,
    #[asset(key = "map.main")]
    tile_map: Handle<helpers::tiled::TiledMap>,
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()), // prevents blurry sprites
            bevy_inspector_egui::DefaultInspectorConfigPlugin,
            EguiPlugin,
            PanCamPlugin::default(),
            TilemapPlugin,
            helpers::tiled::TiledMapPlugin,
        ))
        .init_resource::<Configuration>()
        .init_resource::<WorldPosition>()
        .add_state::<AppState>()
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
        .add_systems(Update, inspector_ui.run_if(in_state(AppState::Level)))
        .run();
}

#[derive(Reflect, Resource, Default, InspectorOptions)]
#[reflect(Resource, InspectorOptions)]
struct Configuration {
    name: String,
    #[inspector(min = 0.0, max = 1.0)]
    option: f32,
    mouse_position: WorldPosition,
    cursor_in_map_pos: Vec2,
}

#[derive(Component)]
struct AnimationFrame(i32);

#[derive(Component, Deref, DerefMut)]
struct AnimationTimer(Timer);

fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(
        &mut AnimationFrame,
        &mut AnimationTimer,
        &mut TextureAtlasSprite,
    )>,
) {
    for (mut frame, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());
        if timer.just_finished() {
            frame.0 += 1;
            if frame.0 == 2 as i32 {
                frame.0 = 0
            }
            sprite.index = ([22, 42])[frame.0 as usize]
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
        &TilemapType,
        &TileStorage,
        &Transform,
    )>,
) {
    // get the camera info and transform
    // assuming there is exactly one main camera entity, so Query::single() is OK
    let (camera, camera_transform) = q_camera.single();

    // There is only one primary window, so we can similarly get it from the query:
    let window = q_window.single();

    // check if the cursor is inside the window and get its position
    // then, ask bevy to convert into world coordinates, and truncate to discard Z
    if let Some(world_position) = window
        .cursor_position()
        .and_then(|cursor| camera.viewport_to_world(camera_transform, cursor))
        .map(|ray| ray.origin.truncate())
    {
        config.mouse_position.0 = world_position;
    }

    // run this block _AFTER_ the cursor position is calculated above
    for (map_size, grid_size, map_type, _tile_storage, map_transform) in tilemap_q.iter() {
        // Grab the cursor position from the `Res<CursorPos>`
        let cursor_pos: Vec2 = config.mouse_position.0;
        // We need to make sure that the cursor's world position is correct relative to the map
        // due to any map transformation.
        let cursor_in_map_pos: Vec2 = {
            // Extend the cursor_pos vec3 by 0.0 and 1.0
            let cursor_pos = Vec4::from((cursor_pos, 0.0, 1.0));
            let cursor_in_map_pos = map_transform.compute_matrix().inverse() * cursor_pos;
            cursor_in_map_pos.xy()
        };

        // Once we have a world position we can transform it into a possible tile position.
        if let Some(tile_pos) =
            TilePos::from_world_pos(&cursor_in_map_pos, map_size, grid_size, map_type)
        {
            config.cursor_in_map_pos = Vec2::from(tile_pos);
        }
    }
}

fn inspector_ui(world: &mut World) {
    let mut egui_context = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .single(world)
        .clone();

    // the usual `ResourceInspector` code
    egui::Window::new("Resource Inspector").show(egui_context.get_mut(), |ui| {
        egui::ScrollArea::both().show(ui, |ui| {
            bevy_inspector_egui::bevy_inspector::ui_for_resource::<Configuration>(world, ui);
        });
    });

    // egui::Window::new("UI").show(egui_context.get_mut(), |ui| {
    //     egui::ScrollArea::both().show(ui, |ui| {
    //         //bevy_inspector::ui_for_world(world, ui);

    //         for frame in &query {
    //             // works with any `Reflect` value, including `Handle`s
    //             let mut any_reflect_value: i32 = frame.0;
    //             bevy_inspector::ui_for_value(&mut any_reflect_value, ui, world);
    //         }

    //         ui.heading("Entities");
    //         bevy_inspector::ui_for_world_entities(world, ui);
    //     })
    // });

    // egui::Window::new("UI").show(egui_context.get_mut(), |ui| {
    //     egui::ScrollArea::both().show(ui, |ui| {
    //         // equivalent to `WorldInspectorPlugin`
    //         bevy_inspector::ui_for_world(world, ui);

    //         // works with any `Reflect` value, including `Handle`s
    //         let mut any_reflect_value: i32 = 5;
    //         bevy_inspector::ui_for_value(&mut any_reflect_value, ui, world);

    //         egui::CollapsingHeader::new("Materials").show(ui, |ui| {
    //             bevy_inspector::ui_for_assets::<StandardMaterial>(world, ui);
    //         });

    //         ui.heading("Entities");
    //         bevy_inspector::ui_for_world_entities(world, ui);
    //     });
    // });
}

fn spawn_level(
    mut commands: Commands,
    game_info: Res<GameInfoAlt>,
    tile_maps: Res<Assets<helpers::tiled::TiledMap>>,
    mut state: ResMut<NextState<AppState>>,
) {
    info!("spawn_sprites");

    commands.spawn(helpers::tiled::TiledMapBundle {
        tiled_map: game_info.tile_map.clone(),
        transform: Transform::from_scale(Vec3::splat(1.0))
            .with_translation(Vec3::new(0.0, 0.0, 0.1)),
        ..Default::default()
    });

    let mut camera_pos = Vec2::default();
    let mut map_size = Vec2::default();

    // spawn characters
    if let Some(map) = tile_maps.get(&game_info.tile_map) {
        map_size = Vec2::new(
            (map.map.width * map.map.tile_width) as f32,
            (map.map.height * map.map.tile_height) as f32,
        );
        info!("spawn objects");
        let tile_layers = map
            .map
            .layers()
            .filter_map(|layer| match layer.layer_type() {
                tiled::LayerType::Objects(layer) => Some(layer),
                _ => None,
            });

        for layer in tile_layers {
            //my_renderer.render(layer);
            for object in layer.objects() {
                if object.visible && object.user_type.eq_ignore_ascii_case("spawn") {
                    info!("spawning {}\n", object.name);

                    let pos = Vec2::new(
                        object.x,
                        (map.map.height * map.map.tile_height) as f32 - object.y,
                    );

                    let animation_frame = AnimationFrame(0);
                    commands.spawn((
                        SpriteSheetBundle {
                            texture_atlas: game_info.creature_atlas.clone(),
                            sprite: TextureAtlasSprite::new(22),
                            transform: Transform::from_translation(Vec3::new(pos.x, pos.y, 2.0)),
                            ..default()
                        },
                        animation_frame,
                        AnimationTimer(Timer::from_seconds(0.2, TimerMode::Repeating)),
                    ));

                    camera_pos = pos;
                }
            }
        }
    }

    commands
        .spawn((Camera2dBundle::default(), MainCamera))
        .insert(PanCam {
            grab_buttons: vec![MouseButton::Left, MouseButton::Middle], // which buttons should drag the camera
            enabled: true, // when false, controls are disabled. See toggle example.
            zoom_to_cursor: true,
            min_x: Some(0.),
            min_y: Some(0.),
            max_x: Some(map_size.x),
            max_y: Some(map_size.y),
            min_scale: 0.25,
            max_scale: Some(30.),
        });
    state.set(AppState::Level);
}
