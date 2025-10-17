use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::ecs::resource::Resource;
use hostfxr_sys::{
    dlopen2::wrapper::Container, get_function_pointer_fn, hostfxr_delegate_type, hostfxr_handle,
    load_assembly_fn, wrapper::Hostfxr as HostfxrLibrary,
};

#[cfg(target_os = "windows")]
pub fn to_char_t(value: impl AsRef<str>) -> widestring::U16String {
    let mut value = value.as_ref().to_string();
    if !value.ends_with('\0') {
        value.push('\0');
    }
    widestring::U16String::from_str(&value)
}

#[cfg(not(target_os = "windows"))]
fn to_char_t(value: impl AsRef<str>) -> std::ffi::CString {
    let mut value = value.as_ref().to_string();
    if !value.ends_with('\0') {
        value.push('\0');
    }
    std::ffi::CString::from_str(&value).unwrap()
}

pub fn get_dotnet_path() -> Option<PathBuf> {
    let dotnet_path = std::env::var("DOTNET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(all(target_os = "windows", not(target_arch = "x86")))]
            let default = PathBuf::from("C:\\Program Files\\dotnet");
            #[cfg(all(target_os = "windows", target_arch = "x86"))]
            let default = PathBuf::from("C:\\Program Files (x86)\\dotnet");
            #[cfg(target_os = "linux")]
            let default = {
                let t = PathBuf::from("/usr/share/dotnet");
                if t.exists() {
                    t
                } else {
                    PathBuf::from("/usr/lib/dotnet")
                }
            };
            #[cfg(all(target_os = "macos", not(target_arch = "x86_64")))]
            let default = PathBuf::from("/usr/local/share/dotnet/x64");
            #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
            let default = PathBuf::from("/usr/local/share/dotnet");

            if default.exists() {
                default
            } else {
                dirs::home_dir().unwrap().join(".dotnet")
            }
        });

    dotnet_path.exists().then_some(dotnet_path)
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

#[derive(Clone)]
struct Hostfxr {
    lib: Arc<Container<HostfxrLibrary>>,
    ctx: hostfxr_handle,
    get_function_pointer: get_function_pointer_fn,
}
unsafe impl Send for Hostfxr {}
unsafe impl Sync for Hostfxr {}

impl Hostfxr {
    pub fn new(paths: &RuntimePaths) -> Self {
        log::debug!("[C# Runtime] initializing hostfxr");

        let hostfxr_library = unsafe {
            Container::<HostfxrLibrary>::load(&paths.hostfxr)
                .expect("failed to load hostfxr and defined path")
        };

        let mut ctx: hostfxr_handle = std::ptr::null();
        let path = to_char_t(paths.config.display().to_string());
        unsafe {
            hostfxr_library.hostfxr_initialize_for_runtime_config(
                path.as_ptr(),
                std::ptr::null(),
                &raw mut ctx,
            )
        };

        let mut load_assembly: *const () = std::ptr::null();
        let result = unsafe {
            hostfxr_library.hostfxr_get_runtime_delegate(
                ctx,
                hostfxr_delegate_type::hdt_load_assembly,
                &raw mut load_assembly,
            )
        };
        assert!(
            result == 0 && !load_assembly.is_null(),
            "failed to load 'load_assembly' from hostfxr"
        );
        let load_assembly: load_assembly_fn = unsafe { std::mem::transmute(load_assembly) };

        let mut get_function_pointer: *const () = std::ptr::null();
        let result = unsafe {
            hostfxr_library.hostfxr_get_runtime_delegate(
                ctx,
                hostfxr_delegate_type::hdt_get_function_pointer,
                &raw mut get_function_pointer,
            )
        };
        assert!(
            result == 0 && !get_function_pointer.is_null(),
            "failed to load 'load_assembly' from hostfxr"
        );
        let get_function_pointer: get_function_pointer_fn =
            unsafe { std::mem::transmute(get_function_pointer) };

        let dll = to_char_t(paths.dll.display().to_string());
        let result = unsafe { load_assembly(dll.as_ptr(), std::ptr::null(), std::ptr::null()) };
        assert_eq!(result, 0, "failed to load dll");

        Self {
            lib: Arc::new(hostfxr_library),
            ctx,
            get_function_pointer,
        }
    }

    /// # Safety
    /// Interacts with raw pointers and returns a raw c# managed function pointer
    pub unsafe fn get_function_with_delegate(
        &self,
        r#type: &str,
        method: &str,
        delegate: &str,
    ) -> *const () {
        let type_name = to_char_t(r#type);
        let method_name = to_char_t(method);
        let delegate_type_name = to_char_t(delegate);

        let mut delegate: *const () = std::ptr::null();
        let result = unsafe {
            (self.get_function_pointer)(
                type_name.as_ptr(),
                method_name.as_ptr(),
                delegate_type_name.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                (&raw mut delegate).cast(),
            )
        };
        assert_eq!(
            result, 0,
            "hostfxr failed to fetch dll function with delegate"
        );
        delegate
    }
}

impl Drop for Hostfxr {
    fn drop(&mut self) {
        unsafe { self.lib.hostfxr_close(self.ctx) };
    }
}

pub struct RuntimePaths {
    pub config: PathBuf,
    pub dll: PathBuf,
    pub dotnet: PathBuf,
    pub hostfxr: PathBuf,
}

pub struct RuntimeVersions {
    pub framework: String,
    pub net: String,
}

#[derive(Resource)]
pub struct Runtime {
    #[allow(dead_code)]
    host: Hostfxr,
    pub paths: RuntimePaths,
    pub versions: RuntimeVersions,

    ping: unsafe extern "system" fn(*mut u32) -> i32,
}

enum Level {
    Warning,
    Error,
}

struct Diagnostic {
    pub source: PathBuf,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub level: Level,
    pub code: String,
    pub message: String,
}
impl Diagnostic {
    pub fn log(&self) {
        match self.level {
            Level::Warning => log::warn!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.file.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
            Level::Error => log::error!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.file.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
        }
    }
}

impl Runtime {
    #[allow(clippy::missing_transmute_annotations)]
    pub fn new(config: &CSharpScripting) -> Self {
        let exe_parent = std::env::current_exe().unwrap();
        let exe_dir = exe_parent.parent().unwrap();

        let dotnet = get_dotnet_path().expect("dotnet not found");

        let hostfxr_path = dotnet.join("host").join("fxr");

        let mut hostfxr_versions = hostfxr_path
            .read_dir()
            .expect("host/fxr not found")
            .filter_map(Result::ok)
            .map(|v| v.file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        hostfxr_versions.sort();

        let framework_version = hostfxr_versions
            .iter()
            .find(|v| config.version == v)
            .or_else(|| match &config.version {
                Version::Latest(l) => hostfxr_versions
                    .iter()
                    .filter(|v| v.starts_with(l.rollforward()))
                    .collect::<Vec<_>>()
                    .last()
                    .copied(),
                _ => None,
            })
            .expect("failed to resolve a framework version");

        let net_version = format!("net{}.0", framework_version.split_once('.').unwrap().0);

        let versions = RuntimeVersions {
            framework: framework_version.clone(),
            net: net_version,
        };

        log::debug!("[C# Runtime] Versions:");
        log::debug!("    net: {}", versions.net);
        log::debug!("    framework: {}", versions.framework);

        let paths = RuntimePaths {
            dotnet,
            config: exe_dir.join("Runtime.runtimeconfig.json"),
            dll: exe_dir.join("Runtime.dll"),
            hostfxr: hostfxr_path.join(framework_version).join({
                #[cfg(target_os = "windows")]
                {
                    "hostfxr.dll"
                }
                #[cfg(target_os = "linux")]
                {
                    "hostfxr.so"
                }
                #[cfg(target_os = "linux")]
                {
                    "hostfxr.dylib"
                }
            }),
        };

        log::debug!("[C# Runtime] Paths:");
        log::debug!("    dotnet: {}", paths.dotnet.display());
        log::debug!("    hostfxr: {}", paths.hostfxr.display());
        log::debug!("    config: {}", paths.config.display());
        log::debug!("    dll: {}", paths.dll.display());

        // TODO: Research whether this can be done once when packaging for
        //  production (Release)
        #[cfg(all(debug_assertions, feature = "download-runtime"))]
        Self::ensure_runtime(exe_dir, &versions, &paths);

        let host = Hostfxr::new(&paths);

        Self {
            ping: unsafe {
                std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "Ping",
                    "Host+PingDelegate, Runtime",
                ))
            },

            host,
            paths,
            versions,
        }
    }

    fn ensure_runtime(exe_dir: &Path, versions: &RuntimeVersions, paths: &RuntimePaths) {
        let runtime_dir = exe_dir.parent().unwrap().join("runtime");
        let runtime_csproj = runtime_dir.join("Runtime.csproj");
        let runtimeconfig_bin = runtime_dir
            .join("bin")
            .join("Release")
            .join(&versions.net)
            .join("Runtime.runtimeconfig.json");
        let runtime_dll_bin = runtime_dir
            .join("bin")
            .join("Release")
            .join(&versions.net)
            .join("Runtime.dll");

        if !runtime_dll_bin.exists()
            || !runtimeconfig_bin.exists()
            || !runtime_csproj.exists()
            || !std::fs::read_to_string(&runtime_csproj)
                .unwrap()
                .contains(&format!(
                    "<RuntimeFrameworkVersion>{}</RuntimeFrameworkVersion>",
                    &versions.framework
                ))
        {
            // use std::process::Stdio;

            if !runtime_dir.exists() {
                std::fs::create_dir_all(&runtime_dir).unwrap();
            }
            std::fs::write(
                &runtime_csproj,
                format_runtime_csproj(&versions.net, &versions.framework),
            )
            .unwrap();

            {
                let result = reqwest::blocking::Client::new()
                    .get("https://raw.githubusercontent.com/Tired-Fox/bevy_cs_managed/refs/heads/main/Runtime.cs")
                    .send()
                    .unwrap();

                std::fs::write(runtime_dir.join("Runtime.cs"), result.text().unwrap()).unwrap();
            }

            log::debug!("[C# Runtime] compiling {}", runtime_csproj.display());

            let build_log = runtime_dir.join("build.log");
            if build_log.exists() {
                std::fs::remove_file(&build_log).unwrap();
            }

            let result = std::process::Command::new({
                #[cfg(target_os = "windows")]
                {
                    paths.dotnet.join("dotnet.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    dotnet_path.join("dotnet")
                }
            })
            .arg("build")
            .arg(&runtime_csproj)
            .args(["-c", "Release"])
            .arg(format!("/flp:v=q;logfile={}", build_log.display()))
            .arg("/clp:ErrorsOnly")
            .output()
            .unwrap();

            if !result.status.success() {
                panic!("failed to build runtime api");
            }

            if build_log.exists() {
                let diag = std::fs::read_to_string(&build_log).unwrap();
                let pattern = regex::Regex::new(
                    r"(.+)\((\d+),(\d+)\): (warning|error) ([^:]+): (.+) \[([^\]]+)\]",
                )
                .unwrap();

                diag.lines()
                    .filter_map(|v| pattern.captures(v))
                    .for_each(|v| {
                        Diagnostic {
                            source: PathBuf::from(v[7].to_string()),
                            file: PathBuf::from(v[1].to_string()),
                            line: v[2].parse::<usize>().unwrap(),
                            column: v[3].parse::<usize>().unwrap(),
                            level: match &v[4] {
                                "warning" => Level::Warning,
                                _ => Level::Error,
                            },
                            code: v[5].into(),
                            message: v[6].into(),
                        }
                        .log()
                    });
            }

            log::debug!("[c#-runtime] copying Runtime.runtimeconfig.json to output");
            std::fs::copy(&runtimeconfig_bin, &paths.config).unwrap();
            log::debug!("[C# Runtime] copying Runtime.dll to output");
            std::fs::copy(&runtime_dll_bin, &paths.dll).unwrap();
        }

        if !paths.config.exists() {
            log::debug!("[c#-runtime] copying Runtime.runtimeconfig.json to output");
            std::fs::copy(&runtimeconfig_bin, &paths.config).unwrap();
        }

        if !paths.dll.exists() {
            log::debug!("[C# Runtime] copying Runtime.dll to output");
            std::fs::copy(&runtime_dll_bin, &paths.dll).unwrap();
        }
    }

    pub fn ping(&self) -> bool {
        let mut out: u32 = 0;
        unsafe { (self.ping)(&raw mut out) };
        out == 1
    }
}

#[derive(Default)]
pub enum Dotnet {
    Net7_0,
    #[default]
    Net8_0,
    Net9_0,
}
impl Dotnet {
    pub fn latest_semver(&self) -> &'static str {
        use Dotnet::*;
        match self {
            Net7_0 => "7.0.20",
            Net8_0 => "8.0.21",
            Net9_0 => "9.0.10",
        }
    }

    pub fn rollforward(&self) -> &'static str {
        use Dotnet::*;
        match self {
            Net7_0 => "7",
            Net8_0 => "8",
            Net9_0 => "9",
        }
    }
}

pub enum Version {
    /// Use the latest of a specific Dotnet version
    ///
    /// This will compile will the latest version found on your system
    /// that matches.
    Latest(Dotnet),
    /// The full semver of the .NET runtime.
    ///
    /// # Example
    /// `9.0.10` for .NET 9 as of October 14, 2025
    /// `8.0.21` for .NET 8 as of October 14, 2025
    /// `7.0.20` for .NET 7 as of May 28, 2024
    ///
    /// > You can find the latest .NET Runtime version at:
    /// >   1. Goto https://dotnet.microsoft.com/en-us/download/dotnet,
    /// >   2. select the desired version
    /// >   3. under the `Included Runtimes` section find `.Net Runtime`
    Custom(String),
}

impl<A: AsRef<str>> PartialEq<A> for Version {
    fn eq(&self, other: &A) -> bool {
        match self {
            Self::Custom(v) => v.as_str() == other.as_ref(),
            Self::Latest(l) => l.latest_semver() == other.as_ref(),
        }
    }
}

impl Default for Version {
    fn default() -> Self {
        Version::Latest(Dotnet::default())
    }
}

#[derive(Default)]
pub struct CSharpScripting {
    pub version: Version,
}

impl bevy::app::Plugin for CSharpScripting {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(Runtime::new(self));
        app.add_systems(bevy::prelude::Startup, setup);
    }
}

fn setup(runtime: bevy::prelude::Res<Runtime>) {
    log::info!("Ping: {}", runtime.ping());
}
