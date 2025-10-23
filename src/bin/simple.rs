use bevy_cs_managed::{CSharpPlugin, Runtime, Script};

use bevy::{ecs::{lifecycle::HookContext, world::DeferredWorld}, prelude::*};
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct Vector3 {
    x: f32,
    y: f32,
    z: f32,
}
impl std::fmt::Debug for Vector3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

pub fn awake(world: DeferredWorld, context: HookContext) {
    let entity = world.entity(context.entity);
    let script = entity.get::<Script>().unwrap();

    let Some(runtime) = world.get_resource::<Runtime>() else { return };
    let Some(awake) = runtime.get_method(script, "Awake", 0) else { return };
    awake.invoke(());
}

fn setup_scripts(world: &mut World) {
    world
        .register_component_hooks::<Script>()
        .on_add(awake);
}

fn spawn_scripts(mut commands: Commands, mut runtime: ResMut<Runtime>) {
    let script = runtime.create("Player").unwrap();
    script.set_property_value("Position", &Vector3 { x: 1.2, y: 2.4, z: 3.6 });

    // Can inspect the fields on a script
    println!("Player {{");
    let metadata = runtime.get_meta_data(&script);
    for field in &metadata.fields {
        println!("    {} = {:?}", field.name, script.get_field_value::<Vector3>("Position"));
    }
    for prop in &metadata.properties {
        println!("    {} = {:?}", prop.name, script.get_property_value::<Vector3>("Position"));
    }
    println!("}}");

    commands.spawn(script);
}

fn update(
    query: Query<&Script>,
    delta: Res<Time>,
    runtime: Res<Runtime>,
) {
    let dt = delta.delta_secs();

    for script in &query {
        if let Some(update) = runtime.get_method(script, "Update", 1) {
            update.invoke(&dt);
        }
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(CSharpPlugin)
        .add_systems(PreStartup, setup_scripts)
        .add_systems(Startup, spawn_scripts)
        .add_systems(Update, update)
        .run();
}
