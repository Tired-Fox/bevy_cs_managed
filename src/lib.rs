mod config;

mod hostfxr;
mod error;
pub use error::{Error, Result};

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
        let mut runtime = Runtime::new().unwrap();

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

            if !runtime.paths.exe.join("managed").exists() {
                std::fs::create_dir_all(runtime.paths.exe.join("managed")).unwrap();
            }

            let (name, base) = builder.build(engine_path.join("Engine.csproj")).unwrap();
            std::fs::copy(
                base.join(format!("{name}.dll")),
                AssemblyType::Engine.path(&runtime.paths.exe),
            )
            .unwrap();

            let (name, base) = builder.build(scripts_path.join("Scripts.csproj")).unwrap();
            std::fs::copy(
                base.join(format!("{name}.dll")),
                AssemblyType::Scripts.path(&runtime.paths.exe),
            )
            .unwrap();
        }

        runtime.load(AssemblyType::Engine).unwrap();
        runtime.load(AssemblyType::Scripts).unwrap();

        for entry in glob::glob("assets/scripts/**/*.cs").unwrap() {
            match entry {
                Ok(path) => if !path.iter().any(|c| c.to_string_lossy() == runtime.get_net_version()) {
                    runtime.register(path.file_stem().unwrap().to_string_lossy()).unwrap();
                },
                Err(e) => eprintln!("{:?}", e),
            } 
        }

        app.insert_resource(runtime);
    }
}
