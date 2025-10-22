use std::{path::PathBuf, sync::Once};

#[path = "src/config.rs"]
mod config;
#[path = "src/dotnet/mod.rs"]
mod dotnet;
use config::{Config, Version};
use log::{Metadata, Record};

static RUNTIME_CS: &[u8] = include_bytes!("Runtime.cs");

struct Paths {
    dotnet: PathBuf,
    hostfxr: PathBuf,
    config: PathBuf,
    profile: PathBuf,
    target: PathBuf,
    output: PathBuf,
}

fn main() {
    init_build_logger();

    println!("cargo::rerun-if-changed=target/runtime");
    println!("cargo::rerun-if-changed=managed.config.json");

    let output = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not bound"));
    let constants = output.join("constants.rs");
    let profile = output.parent().unwrap().parent().unwrap().parent().unwrap();
    let target = output.parent().unwrap();

    let dotnet_path = dotnet::get_path().expect("dotnet not found");

    let paths = Paths {
        hostfxr: dotnet_path.join("host").join("fxr"),
        dotnet: dotnet_path,
        config: std::env::current_dir().unwrap().join("managed.config.json"),
        profile: profile.to_path_buf(),
        target: target.to_path_buf(),
        output,
    };

    let config = if paths.config.exists() {
        let data = std::fs::read_to_string(&paths.config).unwrap();
        serde_json::from_str::<Config>(&data).unwrap()
    } else {
        Config::default()
    };

    let (framework, net) = get_versions(&paths, &config);

    std::fs::write(
        &constants,
        format!(
            r#"
                const FRAMEWORK: &'static str = "{framework}";
                const NET: &'static str = "{net}";
            "#
        ),
    )
    .unwrap();

    let builder = dotnet::Builder::new(&paths.dotnet, &net);

    ensure_runtime(&framework, &net, &paths, &builder);
}

fn get_versions(paths: &Paths, config: &Config) -> (String, String) {
    let mut hostfxr_versions = paths
        .hostfxr
        .read_dir()
        .expect("host/fxr not found")
        .filter_map(Result::ok)
        .map(|v| v.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    hostfxr_versions.sort();

    let framework = match &config.version {
        Version::Net(n) => hostfxr_versions
            .iter()
            .filter(|v| v.starts_with(&n.to_string()))
            .collect::<Vec<_>>()
            .last()
            .cloned(),
        Version::Framework(f) => hostfxr_versions.iter().find(|v| f == *v),
    }
    .expect("failed to resolve a framework version");

    (
        framework.to_string(),
        format!("net{}.0", framework.split_once('.').unwrap().0),
    )
}

fn format_runtime_csproj(net: &str, framework: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{net}</TargetFramework>
    <RuntimeFrameworkVersion>{framework}</RuntimeFrameworkVersion>
    <GenerateRuntimeConfigurationFiles>true</GenerateRuntimeConfigurationFiles>

    <RollForward>Disable</RollForward>
    <UseWindowsForms>false</UseWindowsForms>
    <UseWPF>false</UseWPF>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <Nullable>enable</Nullable>

    <EnableComHosting>false</EnableComHosting>
    <IsComHostedApp>false</IsComHostedApp>
    <EnableGeneratedComInterfaceSourceGenerators>false</EnableGeneratedComInterfaceSourceGenerators>
  </PropertyGroup>
  <ItemGroup>
    <FrameworkReference Update="Microsoft.NETCore.App" RuntimeFrameworkVersion="{framework}" />
  </ItemGroup>
</Project>"#
    )
}

fn ensure_runtime(framework: &str, net: &str, paths: &Paths, builder: &dotnet::Builder) {
    let runtime_dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("runtime");

    let runtime_csproj = runtime_dir.join("Runtime.csproj");
    let runtimeconfig_bin = runtime_dir
        .join("bin")
        .join("Release")
        .join(net)
        .join("Runtime.runtimeconfig.json");
    let runtime_dll_bin = runtime_dir
        .join("bin")
        .join("Release")
        .join(net)
        .join("Runtime.dll");
    let runtime_cs = runtime_dir.join("Runtime.cs");

    let runtime_out_dll = paths.profile.join("Runtime.dll");
    let runtime_out_config = paths.profile.join("Runtime.runtimeconfig.json");

    #[cfg(not(feature = "always-build-runtime"))]
    let needs_rebuild = !runtime_dll_bin.exists()
        || !runtimeconfig_bin.exists()
        || !runtime_csproj.exists()
        || !std::fs::read_to_string(&runtime_csproj)
            .unwrap()
            .contains(&format!(
                "<RuntimeFrameworkVersion>{framework}</RuntimeFrameworkVersion>",
            ));
    #[cfg(feature = "always-build-runtime")]
    let needs_rebuild = true;

    if needs_rebuild {
        if !runtime_dir.exists() {
            std::fs::create_dir(&runtime_dir).unwrap();
        }

        std::fs::write(&runtime_csproj, format_runtime_csproj(net, framework)).unwrap();

        std::fs::write(&runtime_cs, RUNTIME_CS).unwrap();

        _ = builder.build(&runtime_csproj).unwrap();

        log::debug!(
            "[copy] {} to {}",
            runtimeconfig_bin.display(),
            runtime_out_config.display()
        );
        std::fs::copy(&runtimeconfig_bin, &runtime_out_config).unwrap();
        log::debug!(
            "[copy] {} to {}",
            runtime_dll_bin.display(),
            runtime_out_dll.display()
        );
        std::fs::copy(&runtime_dll_bin, &runtime_out_dll).unwrap();
    } else {
        if !runtime_out_config.exists() {
            log::debug!(
                "[copy] {} to {}",
                runtimeconfig_bin.display(),
                runtime_out_config.display()
            );
            std::fs::copy(&runtimeconfig_bin, &runtime_out_config).unwrap();
        }

        if !runtime_out_dll.exists() {
            log::debug!(
                "[copy] {} to {}",
                runtime_dll_bin.display(),
                runtime_out_dll.display()
            );
            std::fs::copy(&runtime_dll_bin, &runtime_out_dll).unwrap();
        }
    }
}

struct BuildScriptLogger;
impl log::Log for BuildScriptLogger {
    fn enabled(&self, _: &Metadata) -> bool { true }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("cargo:warning={}", record.args());
        }
    }

    fn flush(&self) {
        // No-op for this simple example, but you might flush buffers here
    }
}

static LOGGER: BuildScriptLogger = BuildScriptLogger;
static INIT: Once = Once::new();

pub fn init_build_logger() {
    INIT.call_once(|| {
        log::set_logger(&LOGGER)
            .map(|()| log::set_max_level(log::LevelFilter::Debug)) // Set max level for filtering
            .expect("Failed to set logger");
    });
}
