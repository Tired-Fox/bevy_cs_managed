use std::{ffi::c_void, path::{Path, PathBuf}};

use bevy::ecs::resource::Resource;

use crate::{dotnet, hostfxr::Hostfxr};

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

pub type Destroy = unsafe extern "system" fn(*const c_void) -> i32;

pub struct Managed {
    pub(crate) ping: unsafe extern "system" fn(*mut u32) -> i32,
    pub(crate) destroy: Destroy,

    pub(crate) create_scope: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) load_from_path: unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void) -> i32,
    pub(crate) unload_scope: unsafe extern "system" fn(*const c_void) -> i32,

    pub(crate) get_class: unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void) -> i32,

    pub(crate) new: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) is_assignable_from: unsafe extern "system" fn(*const c_void, *const c_void, *mut i32) -> i32,
    pub(crate) get_method: unsafe extern "system" fn(*const c_void, *const c_void, i32, *mut *const c_void) -> i32,

    pub(crate) runtime_invoke: unsafe extern "system" fn(*const c_void, *const c_void, *const *const c_void) -> i32,
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
        // Release hostfxr context
        unsafe { self.host.lib.hostfxr_close(self.host.ctx) };
    }
}

impl Runtime {
    #[allow(clippy::missing_transmute_annotations, clippy::new_without_default)]
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
                    destroy: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "Destroy",
                        "Host+DestroyDelegate, Runtime",
                    )),

                    create_scope: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "CreateScope",
                        "Host+CreateScopeDelegate, Runtime",
                    )),
                    unload_scope: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "Unload",
                        "Host+UnloadDelegate, Runtime",
                    )),
                    load_from_path: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "LoadFromPath",
                        "Host+LoadFromPathDelegate, Runtime",
                    )),

                    get_class: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "GetClass",
                        "Host+GetClassDelegate, Runtime",
                    )),

                    new: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "New",
                        "Host+NewDelegate, Runtime",
                    )),
                    is_assignable_from: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "IsAssignableFrom",
                        "Host+IsAssignableFromDelegate, Runtime",
                    )),
                    get_method: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "GetMethod",
                        "Host+GetMethodDelegate, Runtime",
                    )),

                    runtime_invoke: std::mem::transmute(host.get_function_with_delegate(
                        "Host, Runtime",
                        "RuntimeInvoke",
                        "Host+RuntimeInvokeDelegate, Runtime",
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

    pub fn create_scope(&self) -> Scope {
        let mut out: *const c_void = std::ptr::null();
        // TODO: Error handling
        unsafe { (self.managed.create_scope)(std::ptr::null(), &raw mut out) };
        Scope::new(out, self.managed.unload_scope)
    }

    pub fn load_from_path(&self, scope: &Scope, path: impl AsRef<Path>) -> Option<Assembly> {
        let mut path = path.as_ref().display().to_string();
        if !path.ends_with('\0') { path.push('\0'); }

        let mut out: *const c_void = std::ptr::null();
        // TODO: Error handling
        unsafe { (self.managed.load_from_path)(scope.as_ptr(), path.as_ptr().cast(), &raw mut out) };
        (!out.is_null()).then_some(Assembly::new(out))
    }

    pub fn get_class(&self, assembly: &Assembly, name: impl std::fmt::Display) -> Option<Class> {
        let mut name = name.to_string();
        if !name.starts_with('\0') { name.push('\0'); }
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.managed.get_class)(assembly.as_ptr(), name.as_ptr().cast(), &raw mut out) };
        (!out.is_null()).then_some(Class::new(out, self.managed.destroy))
    }

    pub fn new_object(&self, class: &Class) -> Option<Object> {
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.managed.new)(class.as_ptr(), &raw mut out) };
        (!out.is_null()).then_some(Object::new(out, self.managed.destroy))
    }

    pub fn is_assignable_from(&self, base: &Class, target: &Class) -> bool {
        // TODO: Error handling
        let mut out: i32 = 0;
        unsafe { (self.managed.is_assignable_from)(base.as_ptr(), target.as_ptr(), &raw mut out) };
        out == 1
    }

    pub fn get_method(&self, class: &Class, name: impl std::fmt::Display, args: i32) -> Option<Method> {
        let mut name = name.to_string();
        if !name.starts_with('\0') { name.push('\0'); }
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.managed.get_method)(class.as_ptr(), name.as_ptr().cast(), args, &raw mut out) };
        (!out.is_null()).then_some(Method::new(out, self.managed.destroy))
    }

    pub fn invoke(&self, method: &Method, instance: Option<&Object>, args: &[*const c_void]) {
        // TODO: Error handling
        unsafe { (self.managed.runtime_invoke)(method.as_ptr(), instance.map(|v| v.as_ptr().cast()).unwrap_or(std::ptr::null()), args.as_ptr()) };
    }
}

pub trait Wrapper {
    fn as_ptr(&self) -> *const c_void;
}

pub struct Scope {
    inner: *const c_void,
    unload: Destroy,
}
impl Scope {
    fn new(inner: *const c_void, unload: Destroy) -> Self {
        Self { inner, unload }
    }
}
impl Wrapper for Scope {
    fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        unsafe { (self.unload)(self.inner) };
    }
}

pub struct Assembly {
    inner: *const c_void,
}
impl Assembly {
    fn new(inner: *const c_void) -> Self {
        Self { inner }
    }
}
impl Wrapper for Assembly {
    fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}

pub struct Class {
    inner: *const c_void,
    destroy: Destroy,
}
impl Class {
    fn new(inner: *const c_void, destroy: Destroy) -> Self {
        Self { inner, destroy }
    }
}
impl Wrapper for Class {
    fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}
impl Drop for Class {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.inner) };
    }
}

pub struct Method {
    inner: *const c_void,
    destroy: Destroy,
}
impl Method {
    fn new(inner: *const c_void, destroy: Destroy) -> Self {
        Self { inner, destroy }
    }
}
impl Wrapper for Method {
    fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}
impl Drop for Method {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.inner) };
    }
}

pub struct Object {
    inner: *const c_void,
    destroy: Destroy,
}
impl Object {
    fn new(inner: *const c_void, destroy: Destroy) -> Self {
        Self { inner, destroy }
    }
}
impl Wrapper for Object {
    fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.inner) };
    }
}
