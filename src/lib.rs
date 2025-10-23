use std::collections::HashMap;

mod config;

mod hostfxr;

pub mod runtime;
use runtime::AssemblyType;
pub use runtime::{Script, Runtime};

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

pub struct CSharpPlugin;

impl bevy::app::Plugin for CSharpPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        let exe_dir = std::env::current_exe().unwrap();
        let exe_dir = exe_dir.parent().unwrap();

        let mut runtime = Runtime::new();

        assert!(runtime.library.ping(), "failed to bind and initialize C# Runtime");
        runtime.scope = Some(runtime.library.create_scope());

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
        }

        runtime.assemblies = HashMap::from([
            (
                AssemblyType::Engine,
                runtime
                    .library
                    .load_from_path(
                        runtime.scope.as_ref().unwrap(),
                        exe_dir.join("managed").join("Engine.dll"),
                    )
                    .expect("failed to load Engine.dll"),
            ),
            (
                AssemblyType::Scripts,
                runtime
                    .library
                    .load_from_path(
                        runtime.scope.as_ref().unwrap(),
                        exe_dir.join("managed").join("Scripts.dll"),
                    )
                    .expect("failed to load Scripts.dll"),
            )
        ]);

        app.insert_resource(runtime);
    }
}
