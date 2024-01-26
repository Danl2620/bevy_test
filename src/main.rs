//! Renders an animated sprite by loading all animation frames from a single image (a sprite sheet)
//! into a texture atlas, and changing the displayed image periodically.

use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;
use bevy_pancam::{PanCam, PanCamPlugin};

mod helpers;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum AppState {
    #[default]
    Loading,
    Level,
}


#[derive(serde::Deserialize,bevy::asset::Asset,bevy::reflect::TypePath)]
struct SpriteInfo {
    sheet: String,
    columns: usize,
    rows: usize,
    frames: Vec<usize>,
}

#[derive(Resource)]
struct SpriteInfoHandle(Handle<SpriteInfo>);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest())) // prevents blurry sprites
        .add_plugins(RonAssetPlugin::<SpriteInfo>::new(&["sprite-info.ron"]))
        .add_plugins(PanCamPlugin::default())
        .add_plugins(TilemapPlugin)
        .add_plugins(helpers::tiled::TiledMapPlugin)
        .add_state::<AppState>()
        .add_systems(Startup, setup.run_if(in_state(AppState::Loading)))
        .add_systems(Update, spawn_sprites.run_if(in_state(AppState::Loading)))
        .add_systems(Update, animate_sprite.run_if(in_state(AppState::Level)))
        .run();
}

#[derive(Component)]
struct AnimationFrame(i32);

#[derive(Component, Deref, DerefMut)]
struct AnimationTimer(Timer);

fn animate_sprite(
    time: Res<Time>,
    sprite_info: Res<SpriteInfoHandle>,
    sprite_infos: Res<Assets<SpriteInfo>>,
    mut query: Query<(
        &mut AnimationFrame,
        &mut AnimationTimer,
        &mut TextureAtlasSprite,
    )>,
) {
    if let Some(info) = sprite_infos.get(sprite_info.0.id()) {
        for (mut frame, mut timer, mut sprite) in &mut query {
            timer.tick(time.delta());
            if timer.just_finished() {
                frame.0 += 1;
                if frame.0 == info.frames.len() as i32 {
                    frame.0 = 0
                }
                sprite.index = info.frames[frame.0 as usize]
            }
        }
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let sprite_info_handle = SpriteInfoHandle(asset_server.load("main.sprite-info.ron"));
    commands.insert_resource(sprite_info_handle);

    let map_handle: Handle<helpers::tiled::TiledMap> = asset_server.load("maps/TMX/oryx_16-bit_fantasy_test.tmx");

    commands.spawn(helpers::tiled::TiledMapBundle {
        tiled_map: map_handle,
        ..Default::default()
    });

    commands.spawn((
        Camera2dBundle::default(),
        PanCam::default()));
}


fn spawn_sprites(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sprite_info: Res<SpriteInfoHandle>,
    sprite_infos: Res<Assets<SpriteInfo>>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut state: ResMut<NextState<AppState>>,
) {
    if let Some(info) = sprite_infos.get(sprite_info.0.id()) {
        let texture_handle = asset_server.load(info.sheet.clone());
        //let texture_handle = asset_server.load("gabe-idle-run.png");
        let texture_atlas =
            TextureAtlas::from_grid(texture_handle, Vec2::new(24.0, 24.0), info.columns, info.rows, None, None);
        let texture_atlas_handle = texture_atlases.add(texture_atlas);
        // Use only the subset of sprites in the sheet that make up the run animation
        //let animation_indices = AnimationIndices { first: 23, last: 28 };
        let animation_frame = AnimationFrame(0);
        commands.spawn((
            SpriteSheetBundle {
                texture_atlas: texture_atlas_handle,
                sprite: TextureAtlasSprite::new(info.frames[0]),
                transform: Transform::from_scale(Vec3::splat(6.0)),
                ..default()
            },
            animation_frame,
            AnimationTimer(Timer::from_seconds(0.2, TimerMode::Repeating)),
        ));
        state.set(AppState::Level);
    }
}
