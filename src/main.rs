use scripting::CSharpScripting;

use bevy::{
    log::{Level, LogPlugin},
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins(LogPlugin {
            level: Level::DEBUG,
            filter: "bevy_scripting=debug,bevy_ecs=trace".to_string(),
            custom_layer: |_| None,
            fmt_layer: |_| None,
        })
        .add_plugins(CSharpScripting::default())
        .run();
}
