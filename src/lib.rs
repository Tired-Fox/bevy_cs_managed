use std::path::PathBuf;

mod config;
use config::Version;

mod hostfxr;

pub mod runtime;
use runtime::Runtime;

pub mod dotnet;

fn format_scripts_csproj(net: &str, framework: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{net}</TargetFramework>
    <RuntimeFrameworkVersion>{framework}</RuntimeFrameworkVersion>
    <DebugType>portable</DebugType>
    <RollForward>Disable</RollForward>
    <ImplicitUsings>disable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>
  <ItemGroup>
    <FrameworkReference Update="Microsoft.NETCore.App" RuntimeFrameworkVersion="{framework}" />
  </ItemGroup>
  <ItemGroup Condition="'$(Configuration)' == 'Debug'">
    <ProjectReference Include="..\engine\Engine.csproj" />
  </ItemGroup>
  <ItemGroup Condition="'$(Configuration)' != 'Debug'">
    <Reference Include="Engine">
      <HintPath>..\engine\bin\Release\{net}\Engine.dll</HintPath>
    </Reference>
  </ItemGroup>
</Project>"#
    )
}

fn format_engine_csproj(net: &str, framework: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{net}</TargetFramework>
    <RuntimeFrameworkVersion>{framework}</RuntimeFrameworkVersion>
    <ImplicitUsings>disable</ImplicitUsings>
    <DebugType>portable</DebugType>
    <Nullable>enable</Nullable>
    <RollForward>Disable</RollForward>
  </PropertyGroup>
  <ItemGroup>
    <FrameworkReference Update="Microsoft.NETCore.App" RuntimeFrameworkVersion="{framework}" />
  </ItemGroup>
</Project>"#
    )
}

#[derive(Default)]
pub struct CSharpScripting {
    pub version: Version,
    pub managed: Option<PathBuf>,
}

impl bevy::app::Plugin for CSharpScripting {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(Runtime::new());
        app.add_systems(bevy::prelude::Startup, setup);
    }
}

fn setup(mut runtime: bevy::prelude::ResMut<Runtime>) {
    assert!(runtime.ping(), "failed to bind and initialize C# Runtime");
    runtime.scope = Some(runtime.create_scope());

    #[cfg(debug_assertions)]
    {
        if !runtime.get_managed_path().exists() {
            std::fs::create_dir_all(runtime.get_managed_path()).unwrap();
        }

        let engine_path = runtime.get_managed_path().join("engine");
        if !engine_path.exists() {
            std::fs::create_dir_all(&engine_path).unwrap();
        }
        std::fs::write(
            engine_path.join("Engine.csproj"),
            format_engine_csproj(runtime.get_net_version(), runtime.get_framework_version()),
        )
        .unwrap();

        let scripts_path = runtime.get_managed_path().join("scripts");
        if !scripts_path.exists() {
            std::fs::create_dir_all(&engine_path).unwrap();
        }
        std::fs::write(
            scripts_path.join("Scripts.csproj"),
            format_scripts_csproj(runtime.get_net_version(), runtime.get_framework_version()),
        )
        .unwrap();

        let exe_dir = std::env::current_exe().unwrap();
        let exe_dir = exe_dir.parent().unwrap();

        let builder = dotnet::Builder::new(runtime.get_dotnet_path(), runtime.get_net_version());

        if !exe_dir.join("managed").exists() {
            std::fs::create_dir_all(exe_dir.join("managed")).unwrap();
        }

        let (name, base) = builder.build(engine_path.join("Engine.csproj")).unwrap();
        std::fs::copy(
            base.join(format!("{name}.dll")),
            exe_dir.join("managed").join(format!("{name}.dll")),
        )
        .unwrap();

        let (name, base) = builder.build(scripts_path.join("Scripts.csproj")).unwrap();
        std::fs::copy(
            base.join(format!("{name}.dll")),
            exe_dir.join("managed").join(format!("{name}.dll")),
        )
        .unwrap();

        let _engine_asm = runtime.load_from_path(runtime.scope.as_ref().unwrap(), exe_dir.join("managed").join("Engine.dll")).expect("failed to load Engine.dll");

        let scripts_asm = runtime.load_from_path(runtime.scope.as_ref().unwrap(), exe_dir.join("managed").join(format!("{name}.dll")));
        if let Some(assembly) = scripts_asm.as_ref() {
             if let Some(player) = runtime.get_class(assembly, "Player").as_ref() {
                 let instance = runtime.new_object(player).expect("failed to create a new player class");
                 if let Some(awake) = runtime.get_method(player, "Awake", 0).as_ref() {
                     runtime.invoke(awake, Some(&instance), &[]);
                 }

                 if let Some(update) = runtime.get_method(player, "Update", 1).as_ref() {
                     let dt = 0.016f32;
                     runtime.invoke(update, Some(&instance), &[(&raw const dt).cast()]);
                     runtime.invoke(update, Some(&instance), &[(&raw const dt).cast()]);
                 }
             }
        }
    }
}
