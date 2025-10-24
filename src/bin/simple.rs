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
    //┌─ Lookup the Awake method that has 0 arguments
    //┆ Note: This will cache the method if found for future calls
    //└──────────────────────────────────────────────┬────────┐
    let Ok(Some(awake)) = runtime.get_method(script, "Awake", 0) else { return };
    awake.invoke(()).unwrap();
}

fn setup_scripts(world: &mut World) {
    world
        .register_component_hooks::<Script>()
        .on_add(awake);
}

fn spawn_scripts(mut commands: Commands, mut runtime: ResMut<Runtime>) {
    //┌─This is the fullname of the desired scripts class.
    //┆  The scripts class is resolved by matching a global namespace class with the same name as the file.
    //┆
    //┆  // Player.cs
    //┆  using Engine;
    //┆  class Player
    //┆  {
    //┆     public Vector3 Position;
    //┆  }
    //└─────────────────────────┐
    let script = runtime.create("Player").unwrap();
    //┌─ Set a property on the script class instance
    //┆
    //┆ This works the same with fields with `set_field_value`
    //┆
    //┆ Warning: The property must have a public setter
    //┆
    //┆┌─ Can pass any reference to a value as long as the structure matches
    //┆┆    the expected type in the C# method parameter
    //┆└──────────────────────────────────┐
    //└────┐                              │
    script.set_property_value("Position", &Vector3 { x: 1.2, y: 2.4, z: 3.6 }).unwrap();

    println!("Player {{");
    // MetaData, fields and properties, are cached when the class type is loaded and
    //    can be used to for reflection
    let metadata = runtime.get_meta_data(&script);
    for field in &metadata.fields {
        println!("    {} = {:?}", field.name, script.get_field_value::<Vector3>(&field.name));
    }
    for prop in &metadata.properties {
        println!("    {} = {:?}", prop.name, script.get_property_value::<Vector3>(&prop.name));
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
        //┌─ Lookup the Update method that has 1 arguments
        //┆
        //┆ When invoked the arguments can be passed as a single value reference if there is 1 arg
        //┆     or as a tuple of value references for multiple args.
        //┆
        //└──────────────────────────────────────────────────┬─────────┐
        if let Ok(Some(update)) = runtime.get_method(script, "Update", 1) {
            update.invoke(&dt).unwrap();
        }
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(CSharpPlugin)
        // User is given complete control on how the scripts should be called and manipulated
        //   the crate handles bootstrapping the runtime and managing script references like
        //   classes, objects, methods, etc.
        .add_systems(PreStartup, setup_scripts)
        .add_systems(Startup, spawn_scripts)
        .add_systems(Update, update)
        .run();
}
