use std::{
    ffi::c_void, io::{BufRead, BufReader, Write}, path::{Path, PathBuf}, sync::Arc
};

use bevy::ecs::resource::Resource;
use hostfxr_sys::{
    dlopen2::wrapper::Container, get_function_pointer_fn, hostfxr_delegate_type, hostfxr_handle,
    load_assembly_fn, wrapper::Hostfxr as HostfxrLibrary,
};

#[cfg(not(feature="distribute"))]
static RUNTIME_CS: &[u8] = include_bytes!("../Runtime.cs");
#[cfg(not(feature="distribute"))]
static BUILDER_CS: &[u8] = include_bytes!("../Builder.cs");

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

fn format_scripts_csproj(net: &str, framework: &str) -> String {
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
  <ItemGroup Condition="'$(Configuration)' == 'Debug'">
    <!-- Reference the dynamically built Engine.dll -->
    <ProjectReference Include="..\engine\Engine.csproj" />
  </ItemGroup>
  <ItemGroup Condition="'$(Configuration)' != 'Debug'">
    <!-- Reference the dynamically built Engine.dll -->
    <ProjectReference Include="..\engine\.bin\Engine.dll" />
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

fn format_builder_csproj(net: &str, framework: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>{net}</TargetFramework>
    <RuntimeFrameworkVersion>{framework}</RuntimeFrameworkVersion>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>

  <ItemGroup>
	<PackageReference Include="Microsoft.CodeAnalysis.CSharp.Workspaces" Version="4.11.0" />
    <PackageReference Include="Microsoft.CodeAnalysis.Workspaces.MSBuild" Version="4.11.0" />
    <PackageReference Include="Microsoft.Build.Locator" Version="1.6.10" />
  </ItemGroup>
</Project>"#
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
        log::debug!("[init] hostfxr");

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

        log::debug!("[load] Runtime.dll");
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

#[derive(serde::Deserialize)]
enum Severity {
    Warning,
    Error,
}

#[derive(serde::Deserialize)]
#[serde(rename_all="PascalCase")]
struct Diagnostic {
    pub filename: PathBuf,
    pub line: usize,
    pub column: usize,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}
impl Diagnostic {
    pub fn log(&self) {
        match self.severity {
            Severity::Warning => log::warn!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
            Severity::Error => log::error!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.file_name().unwrap().to_string_lossy(),
                self.line,
                self.column,
                self.message
            ),
        }
    }

    #[allow(dead_code)]
    pub fn log_with_base(&self, base: &Path) {
        let base = dunce::canonicalize(base).unwrap();
        match self.severity {
            Severity::Warning => log::warn!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.strip_prefix(base).unwrap_or(&self.filename).display(),
                self.line,
                self.column,
                self.message
            ),
            Severity::Error => log::error!(
                "[msbuild {}] {} {},{}: {}",
                self.code,
                self.filename.strip_prefix(base).unwrap_or(&self.filename).display(),
                self.line,
                self.column,
                self.message
            ),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all="PascalCase")]
struct BuildResponse {
    elapsed_ms: usize,
    diagnostics: Vec<Diagnostic>,
}

struct DotnetBuilder {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}
impl DotnetBuilder {
    pub fn new(dotnet: PathBuf, builder_dll: &Path) -> std::io::Result<Self> {
        let mut child = std::process::Command::new(dotnet)
            .arg("exec")
            .arg(builder_dll)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());

        Ok(Self { child, stdin, stdout })
    }

    pub fn build(&mut self, project_file: &Path, out_file: &Path) -> std::io::Result<BuildResponse> {
        writeln!(
            self.stdin,
            "{{\"ProjectFile\": \"{}\", \"OutFile\": \"{}\"}}",
            project_file.display().to_string().replace("\\", "\\\\"),
            out_file.display().to_string().replace("\\", "\\\\"),
        )?;
        self.stdin.flush()?;

        let mut response = String::new();
        self.stdout.read_line(&mut response)?;
        serde_json::from_str(&response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }
}
impl Drop for DotnetBuilder {
    fn drop(&mut self) {
        _ = self.child.kill();
    }
}

pub struct Scope {
    inner: *const c_void,
}

pub struct RuntimePaths {
    pub config: PathBuf,
    pub dll: PathBuf,
    pub dotnet: PathBuf,
    pub hostfxr: PathBuf,
    pub managed: PathBuf,
}

pub struct RuntimeVersions {
    pub framework: String,
    pub net: String,
}

pub struct RuntimeManaged {
    pub(crate) ping: unsafe extern "system" fn(*mut u32) -> i32,
    pub(crate) create_scope: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) unload_scope: unsafe extern "system" fn(*const c_void) -> i32,
}

/// # Saftey
/// Not safe when used outside of bevy's ecs like in an alternate thread not managed by bevy
#[allow(dead_code)]
#[derive(Resource)]
pub struct Runtime {
    paths: RuntimePaths,
    versions: RuntimeVersions,

    host: Hostfxr,
    managed: RuntimeManaged,
    #[cfg(not(feature="distribute"))]
    builder: DotnetBuilder,

    scope: Option<Scope>,
}

// Bevy garuntees that one system at a time is using the resource.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(scope) = self.scope.as_ref() {
            self.unload_scope(scope);
        }

        // Release hostfxr context
        unsafe { self.host.lib.hostfxr_close(self.host.ctx) };
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

        log::debug!("Versions:");
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
            managed: config.managed.clone().unwrap_or(PathBuf::from("assets")),
        };

        log::debug!("Paths:");
        log::debug!("    dotnet: {}", paths.dotnet.display());
        log::debug!("    hostfxr: {}", paths.hostfxr.display());
        log::debug!("    config: {}", paths.config.display());
        log::debug!("    dll: {}", paths.dll.display());
        log::debug!("    managed: {}", paths.managed.display());

        #[cfg(not(feature="distribute"))]
        let builder_path = Self::ensure_builder(&versions, &paths);

        #[cfg(not(feature="distribute"))]
        Self::ensure_runtime(&versions, &paths);

        let host = Hostfxr::new(&paths);

        log::debug!("[bind] Runtime.dll methods");
        Self {
            managed: unsafe {
                RuntimeManaged {
                    ping: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "Ping",
                        "Host+PingDelegate, Runtime",
                    )),
                    create_scope: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "CreateScope",
                        "Host+CreateScopeDelegate, Runtime",
                    )),
                    unload_scope: std::mem::transmute(host.get_function_with_delegate(
                        "Scope, Runtime",
                        "Unload",
                        "Scope+UnloadDelegate, Runtime",
                    )),
                }
            },
            #[cfg(not(feature="distribute"))]
            builder: DotnetBuilder::new(
                {
                    #[cfg(target_os = "windows")]
                    {
                        paths.dotnet.join("dotnet.exe")
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        paths.dotnet.join("dotnet")
                    }
                },
                // TODO: download and resolve dll
                &builder_path
            ).unwrap(),
            host,
            paths,
            versions,
            scope: None,
        }
    }

    #[cfg(not(feature="distribute"))]
    fn ensure_builder(versions: &RuntimeVersions, paths: &RuntimePaths) -> PathBuf {
        let builder_dir = std::env::current_dir().unwrap().join("target").join("builder");
        let builder_cs = builder_dir.join("Builder.cs");
        let builder_csproj = builder_dir.join("Builder.csproj");
        let builder_dll = builder_dir.join(".bin").join("Builder.dll");

        if !builder_dll.exists() {
            if !builder_dir.exists() {
                std::fs::create_dir(&builder_dir).unwrap();
            }

            std::fs::write(
                &builder_csproj,
                format_builder_csproj(&versions.net, &versions.framework),
            )
                .unwrap();

            std::fs::write(
                &builder_cs,
                BUILDER_CS,
            )
                .unwrap();

            let now = std::time::Instant::now();
            let build_log = builder_dir.join("build.log");
            let result = std::process::Command::new({
                #[cfg(target_os = "windows")]
                {
                    paths.dotnet.join("dotnet.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    paths.dotnet.join("dotnet")
                }
            })
                .arg("build")
                .arg(&builder_csproj)
                .args(["-c", "Release"])
                .arg(format!("/flp:v=q;logfile={}", build_log.display()))
                .arg("-o")
                .arg(builder_dir.join(".bin"))
                .arg(r#"/p:BaseIntermediateOutputPath=".obj/""#)
                .output()
                .unwrap();

            log::debug!("[compile] {} {:.3} s", builder_csproj.display(), now.elapsed().as_secs_f64());

            if !result.status.success() {
                panic!("failed to build dotnet builder");
            }

            if build_log.exists() {
                let diag = std::fs::read_to_string(&build_log).unwrap();
                let pattern = regex::Regex::new(
                    r"(.+)\((\d+),(\d+)\): (warning|error) (CS\d+): (.+) \[[^\]]+\]",
                )
                .unwrap();

                diag.lines()
                    .filter_map(|v| pattern.captures(v))
                    .for_each(|v| {
                        Diagnostic {
                            filename: PathBuf::from(v[1].to_string()),
                            line: v[2].parse::<usize>().unwrap(),
                            column: v[3].parse::<usize>().unwrap(),
                            severity: match &v[4] {
                                "warning" => Severity::Warning,
                                _ => Severity::Error,
                            },
                            code: v[5].into(),
                            message: v[6].into(),
                        }
                        .log()
                    });
            }
        }

        builder_dll
    }

    #[cfg(not(feature="distribute"))]
    fn ensure_runtime(versions: &RuntimeVersions, paths: &RuntimePaths) {
        let runtime_dir = std::env::current_dir().unwrap().join("target").join("runtime");
        let runtime_csproj = runtime_dir.join("Runtime.csproj");
        
        let runtimeconfig_bin = runtime_dir
            .join(".bin")
            .join("Runtime.runtimeconfig.json");
        let runtime_dll_bin = runtime_dir
            .join(".bin")
            .join("Runtime.dll");
        let runtime_cs = runtime_dir.join("Runtime.cs");

        #[cfg(not(feature = "always-build-runtime"))]
        let needs_rebuild = !runtime_dll_bin.exists()
            || !runtimeconfig_bin.exists()
            || !runtime_csproj.exists()
            || !std::fs::read_to_string(&runtime_csproj)
                .unwrap()
                .contains(&format!(
                    "<RuntimeFrameworkVersion>{}</RuntimeFrameworkVersion>",
                    &versions.framework
                ));
        #[cfg(feature = "always-build-runtime")]
        let needs_rebuild = true;

        if needs_rebuild {
            if !runtime_dir.exists() {
                std::fs::create_dir(&runtime_dir).unwrap();
            }

            std::fs::write(
                &runtime_csproj,
                format_runtime_csproj(&versions.net, &versions.framework),
            )
                .unwrap();

            std::fs::write(
                &runtime_cs,
                RUNTIME_CS,
            )
                .unwrap();

            let now = std::time::Instant::now();
            let build_log = runtime_dir.join("build.log");
            let result = std::process::Command::new({
                #[cfg(target_os = "windows")]
                {
                    paths.dotnet.join("dotnet.exe")
                }
                #[cfg(not(target_os = "windows"))]
                {
                    paths.dotnet.join("dotnet")
                }
            })
                .arg("build")
                .arg(&runtime_csproj)
                .args(["-c", "Release"])
                .arg(format!("/flp:v=q;logfile={}", build_log.display()))
                .arg("-o")
                .arg(runtime_dir.join(".bin"))
                .arg(r#"/p:BaseIntermediateOutputPath=".obj/""#)
                .output()
                .unwrap();

            log::debug!("[compile] {} {:.3} s", runtime_csproj.display(), now.elapsed().as_secs_f64());

            if !result.status.success() {
                panic!("failed to build runtime api");
            }

            if build_log.exists() {
                let diag = std::fs::read_to_string(&build_log).unwrap();
                let pattern = regex::Regex::new(
                    r"(.+)\((\d+),(\d+)\): (warning|error) (CS\d+): (.+) \[[^\]]+\]",
                )
                .unwrap();

                diag.lines()
                    .filter_map(|v| pattern.captures(v))
                    .for_each(|v| {
                        Diagnostic {
                            filename: PathBuf::from(v[1].to_string()),
                            line: v[2].parse::<usize>().unwrap(),
                            column: v[3].parse::<usize>().unwrap(),
                            severity: match &v[4] {
                                "warning" => Severity::Warning,
                                _ => Severity::Error,
                            },
                            code: v[5].into(),
                            message: v[6].into(),
                        }
                        .log()
                    });
            }

            log::debug!("[copy] {} to {}", runtimeconfig_bin.display(), paths.config.display());
            std::fs::copy(&runtimeconfig_bin, &paths.config).unwrap();
            log::debug!("[copy] {} to {}", runtime_dll_bin.display(), paths.dll.display());
            std::fs::copy(&runtime_dll_bin, &paths.dll).unwrap();
        } else {
            if !paths.config.exists() {
                log::debug!("[copy] {} to {}", runtimeconfig_bin.display(), paths.config.display());
                std::fs::copy(&runtimeconfig_bin, &paths.config).unwrap();
            }

            if !paths.dll.exists() {
                log::debug!("[copy] {} to {}", runtime_dll_bin.display(), paths.dll.display());
                std::fs::copy(&runtime_dll_bin, &paths.dll).unwrap();
            }
        }
    }

    pub fn get_config_path(&self) -> &Path {
        &self.paths.config
    }

    pub fn get_dll_path(&self) -> &Path {
        &self.paths.dll
    }

    pub fn get_dotnet_path(&self) -> &Path {
        &self.paths.dotnet
    }

    pub fn get_hostfxr_path(&self) -> &Path {
        &self.paths.hostfxr
    }

    pub fn get_managed_path(&self) -> &Path {
        &self.paths.managed
    }

    pub fn get_framework_version(&self) -> &str {
        &self.versions.framework
    }

    pub fn get_net_version(&self) -> &str {
        &self.versions.net
    }

    #[cfg(not(feature="distribute"))]
    pub fn build_engine(&mut self, target_dir: &Path) {
        let engine_path = self.get_managed_path().join("engine");
        let engine_csproj = engine_path.join("Engine.csproj");

        if !target_dir.join("managed").exists() {
            std::fs::create_dir_all(target_dir.join("managed")).unwrap();
        }

        let result = self.builder.build(&engine_csproj, &target_dir.join("managed").join("Engine.dll")).unwrap();
        log::debug!("[compile] Engine API in {:.3} s", result.elapsed_ms as f64 / 1000.0);
        for d in result.diagnostics {
            d.log_with_base(&engine_path);
        }
    }

    #[cfg(not(feature="distribute"))]
    pub fn build_scripts(&mut self, target_dir: &Path) {
        let scripts_path = self.get_managed_path().join("scripts");
        let scripts_csproj = scripts_path.join("Scripts.csproj");

        if !target_dir.join("managed").exists() {
            std::fs::create_dir_all(target_dir.join("managed")).unwrap();
        }

        let result = self.builder.build(&scripts_csproj, &target_dir.join("managed").join("Scripts.dll")).unwrap();
        log::debug!("[compile] Scripts in {:.3} s", result.elapsed_ms as f64 / 1000.0);
        for d in result.diagnostics {
            d.log_with_base(&scripts_path);
        }
    }

    pub fn ping(&self) -> bool {
        let mut out: u32 = 0;
        unsafe { (self.managed.ping)(&raw mut out) };
        out == 1
    }

    pub fn destroy(&self) -> *const c_void {
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.managed.create_scope)(std::ptr::null(), &raw mut out) };
        out
    }

    pub fn create_scope(&self) -> Scope {
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.managed.create_scope)(std::ptr::null(), &raw mut out) };
        Scope { inner: out }
    }

    pub fn unload_scope(&self, scope: &Scope) {
        unsafe { (self.managed.unload_scope)(scope.inner) };
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
    pub managed: Option<PathBuf>,
}

impl bevy::app::Plugin for CSharpScripting {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(Runtime::new(self));
        app.add_systems(bevy::prelude::Startup, setup);
    }
}

fn setup(mut runtime: bevy::prelude::ResMut<Runtime>) {
    assert!(runtime.ping(), "failed to bind and initialize C# Runtime");
    runtime.scope = Some(runtime.create_scope());

    #[cfg(not(feature="distribute"))]
    {
        if !runtime.get_managed_path().exists() {
            std::fs::create_dir_all(runtime.get_managed_path()).unwrap();
        }

        let engine_path = runtime.get_managed_path().join("engine");
        if !engine_path.exists() {
            std::fs::create_dir_all(&engine_path).unwrap();
        }
        std::fs::write(engine_path.join("Engine.csproj"), format_engine_csproj(runtime.get_net_version(), runtime.get_framework_version())).unwrap();

        let scripts_path = runtime.get_managed_path().join("scripts");
        if !scripts_path.exists() {
            std::fs::create_dir_all(&engine_path).unwrap();
        }
        std::fs::write(scripts_path.join("Scripts.csproj"), format_scripts_csproj(runtime.get_net_version(), runtime.get_framework_version())).unwrap();

        let exe_dir = std::env::current_exe().unwrap();
        let exe_dir = exe_dir.parent().unwrap();

        runtime.build_engine(exe_dir);
        runtime.build_scripts(exe_dir);
    }
}
