//! Renders an animated sprite by loading all animation frames from a single image (a sprite sheet)
//! into a texture atlas, and changing the displayed image periodically.

use bevy::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;


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
    frames: Vec<i32>,
}

#[derive(Resource)]
struct SpriteInfoHandle(Handle<SpriteInfo>);

fn main() {
    App::new()
        .add_plugins(
            (
                DefaultPlugins.set(ImagePlugin::default_nearest()),
                RonAssetPlugin::<SpriteInfo>::new(&["sprite-info.ron"])
            )) // prevents blurry sprites
        .add_state::<AppState>()
        .add_systems(Startup, setup)
        .add_systems(Update, spawn_sprites.run_if(in_state(AppState::Loading)))
        .add_systems(Update, animate_sprite)
        .run();
}

#[derive(Component)]
struct AnimationIndices {
    first: usize,
    last: usize,
}

#[derive(Component, Deref, DerefMut)]
struct AnimationTimer(Timer);

fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(
        &AnimationIndices,
        &mut AnimationTimer,
        &mut TextureAtlasSprite,
    )>,
) {
    for (indices, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());
        if timer.just_finished() {
            sprite.index = if sprite.index == indices.last {
                indices.first
            } else {
                sprite.index + 1
            };
        }
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let sprite_info_handle = SpriteInfoHandle(asset_server.load("main.sprite-info.ron"));
    commands.insert_resource(sprite_info_handle);
}


fn spawn_sprites(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sprite_info: Res<SpriteInfoHandle>,
    mut sprite_infos: ResMut<Assets<SpriteInfo>>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut state: ResMut<NextState<AppState>>,
) {
    if let Some(info) = sprite_infos.remove(sprite_info.0.id()) {
        let texture_handle = asset_server.load(info.sheet.clone());
        //let texture_handle = asset_server.load("gabe-idle-run.png");
        let texture_atlas =
            TextureAtlas::from_grid(texture_handle, Vec2::new(24.0, 24.0), info.columns, info.rows, None, None);
        let texture_atlas_handle = texture_atlases.add(texture_atlas);
        // Use only the subset of sprites in the sheet that make up the run animation
        let animation_indices = AnimationIndices { first: 23, last: 28 };
        commands.spawn(Camera2dBundle::default());
        commands.spawn((
            SpriteSheetBundle {
                texture_atlas: texture_atlas_handle,
                sprite: TextureAtlasSprite::new(animation_indices.first),
                transform: Transform::from_scale(Vec3::splat(6.0)),
                ..default()
            },
            animation_indices,
            AnimationTimer(Timer::from_seconds(0.2, TimerMode::Repeating)),
        ));
        state.set(AppState::Level);
    }
}
