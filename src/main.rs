//! Renders an animated sprite by loading all animation frames from a single image (a sprite sheet)
//! into a texture atlas, and changing the displayed image periodically.

use bevy::prelude::*;
use bevy_asset_loader::asset_collection::AssetCollection;
use bevy_asset_loader::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};

mod helpers;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum AppState {
    #[default]
    Loading,
    Level,
}

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
            PanCamPlugin::default(),
            TilemapPlugin,
            helpers::tiled::TiledMapPlugin,
        ))
        .add_state::<AppState>()
        .add_loading_state(
            LoadingState::new(AppState::Loading)
                .continue_to_state(AppState::Level)
                .with_dynamic_assets_file::<StandardDynamicAssetCollection>("main.assets.ron")
                .load_collection::<GameInfoAlt>(),
        )
        .add_systems(OnEnter(AppState::Level), spawn_level)
        .add_systems(Update, animate_sprite.run_if(in_state(AppState::Level)))
        .run();
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

    // spawn characters
    if let Some(map) = tile_maps.get(&game_info.tile_map) {
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

                    let animation_frame = AnimationFrame(0);
                    commands.spawn((
                        SpriteSheetBundle {
                            texture_atlas: game_info.creature_atlas.clone(),
                            sprite: TextureAtlasSprite::new(22),
                            transform: Transform::from_translation(Vec3::new(
                                object.x, object.y, 2.0,
                            )),
                            ..default()
                        },
                        animation_frame,
                        AnimationTimer(Timer::from_seconds(0.2, TimerMode::Repeating)),
                    ));
                }
            }
        }
    }

    commands.spawn((Camera2dBundle::default(), PanCam::default()));
    state.set(AppState::Level);
}
