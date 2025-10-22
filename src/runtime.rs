use std::{ffi::c_void, path::{PathBuf, Path}};

use bevy::ecs::resource::Resource;

use crate::{dotnet, hostfxr::{Hostfxr, Scope}};

include!(concat!(std::env!("OUT_DIR"), "/constants.rs"));

pub struct Paths {
    pub config: PathBuf,
    pub dll: PathBuf,
    pub dotnet: PathBuf,
    pub hostfxr: PathBuf,
    pub managed: PathBuf,
}

pub struct Versions {
    pub framework: String,
    pub net: String,
}

pub struct Managed {
    pub(crate) ping: unsafe extern "system" fn(*mut u32) -> i32,
    pub(crate) create_scope: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) unload_scope: unsafe extern "system" fn(*const c_void) -> i32,
}

/// # Saftey
/// Not safe when used outside of bevy's ecs like in an alternate thread not managed by bevy
#[allow(dead_code)]
#[derive(Resource)]
pub struct Runtime {
    pub paths: Paths,
    pub versions: Versions,

    host: Hostfxr,
    managed: Managed,

    pub scope: Option<Scope>,
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
    pub fn new() -> Self {
        let exe_parent = std::env::current_exe().unwrap();
        let exe_dir = exe_parent.parent().unwrap();

        let dotnet = dotnet::get_path().expect("dotnet not found");
        let hostfxr_path = dotnet.join("host").join("fxr");

        let versions = Versions {
            framework: FRAMEWORK.to_string(),
            net: NET.to_string(),
        };

        log::debug!("Versions:");
        log::debug!("    net: {}", versions.net);
        log::debug!("    framework: {}", versions.framework);

        let paths = Paths {
            dotnet,
            config: exe_dir.join("Runtime.runtimeconfig.json"),
            dll: exe_dir.join("Runtime.dll"),
            hostfxr: hostfxr_path.join(FRAMEWORK).join({
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
            managed: PathBuf::from("assets"),
        };

        log::debug!("Paths:");
        log::debug!("    dotnet: {}", paths.dotnet.display());
        log::debug!("    hostfxr: {}", paths.hostfxr.display());
        log::debug!("    config: {}", paths.config.display());
        log::debug!("    dll: {}", paths.dll.display());
        log::debug!("    managed: {}", paths.managed.display());

        let host = Hostfxr::new(&paths);

        log::debug!("[bind] Runtime.dll methods");
        Self {
            managed: unsafe {
                Managed {
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
            host,
            paths,
            versions,
            scope: None,
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
        Scope::new(out)
    }

    pub fn unload_scope(&self, scope: &Scope) {
        unsafe { (self.managed.unload_scope)(scope.as_ptr()) };
    }
}
