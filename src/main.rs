use bevy::prelude::*;
use ldtk_integration::{GameLdtkPlugin, LdtkCommandExt};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GameLdtkPlugin::default())
        .add_systems(Startup, (setup_camera, bootstrap_ldtk_world))
        .run();
}

fn setup_camera(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera2d,
        Name::new("Main 2D Camera"),
    ));
}

fn bootstrap_ldtk_world(mut commands: Commands<'_, '_>) {
    commands.spawn_ldtk_world("worlds/AutoLayers_5_Advanced.ldtk");
}
