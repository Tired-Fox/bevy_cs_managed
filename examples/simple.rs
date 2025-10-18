use bevy_cs_managed::CSharpScripting;

use bevy::{
    log::{Level, LogPlugin},
    prelude::*,
};

fn main() {
    App::new()
        .add_plugins(LogPlugin {
            level: Level::DEBUG,
            filter: "ureq=error,rustls=error,bevy_cs_managed=debug,bevy_ecs=trace".to_string(),
            custom_layer: |_| None,
            fmt_layer: |_| None,
        })
        .add_plugins(CSharpScripting::default())
        .run();
}
