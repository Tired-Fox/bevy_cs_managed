use std::{
    borrow::Cow, cell::RefCell, collections::{hash_map::Entry, HashMap}, ffi::{c_void, CStr}, ops::Deref, path::{Path, PathBuf}, rc::Rc
};

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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssemblyType {
    Engine,
    Scripts,
}

/// # Saftey
/// Not safe when used outside of bevy's ecs like in an alternate thread not managed by bevy
#[allow(dead_code)]
#[derive(Resource)]
pub struct Runtime {
    pub paths: Paths,
    pub versions: Versions,

    host: Hostfxr,
    pub managed: Managed,

    pub scope: Option<Scope>,
    pub assemblies: HashMap<AssemblyType, Assembly>,

    pub fullname_to_script: HashMap<Cow<'static, str>, usize>,
    pub scripts: Vec<Type>,
}

// TODO: Add Reflect which fetches cached public fields
pub struct Type {
    pub(crate) name: Cow<'static, str>,
    pub(crate) class: Class,

    pub(crate) methods: RefCell<HashMap<(String, i32), Rc<Method>>>,
}

pub struct Invokable<'s> {
    instance: &'s Object,
    method: Rc<Method>,
    invoke: Invoke,
}
impl<'s> Invokable<'s> {
    pub fn invoke(&self, args: impl ManagedParams) {
        let params = args.into_managed_params();
        unsafe {
            (self.invoke)(
                self.method.as_ptr(),
                self.instance.as_ptr(),
                params.as_ptr(),
            )
        };
    }
}

pub trait ManagedParam {
    fn into_managed_param(self) -> *const c_void;
}
impl<A> ManagedParam for &A {
    fn into_managed_param(self) -> *const c_void {
        self as *const _ as *const c_void
    }
}
impl ManagedParam for Object {
    fn into_managed_param(self) -> *const c_void {
        self.as_ptr()
    }
}

pub trait ManagedParams {
    fn into_managed_params(self) -> Vec<*const c_void>;
}
impl ManagedParams for () {
    fn into_managed_params(self) -> Vec<*const c_void> {
        Vec::new()
    }
}
impl<A: ManagedParam> ManagedParams for A {
    fn into_managed_params(self) -> Vec<*const c_void> {
        Vec::from([self.into_managed_param()])
    }
}
impl<A: ManagedParam, B: ManagedParam> ManagedParams for (A, B) {
    fn into_managed_params(self) -> Vec<*const c_void> {
        Vec::from([
            self.0.into_managed_param(),
            self.1.into_managed_param(),
        ])
    }
}

#[derive(bevy::prelude::Component)]
pub struct Script {
    pub(crate) index: usize,
    pub(crate) instance: Object,
}
impl Deref for Script {
    type Target = Object;
    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}
impl AsRef<Object> for Script {
    fn as_ref(&self) -> &Object {
        &self.instance
    }
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
    #[allow(clippy::new_without_default)]
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
            managed: Managed::new(&host),
            host,
            paths,
            versions,
            scope: None,

            fullname_to_script: Default::default(),
            assemblies: Default::default(),
            scripts: Default::default(),
        }
    }

    /// Create a new instance of a class associated with a certain script index
    pub fn create(&mut self, name: impl AsRef<str>) -> Result<Script, String> {
        if let Some(index) = self.fullname_to_script.get(name.as_ref()).copied() {
            let script = &self.scripts[index];
            let instance = self.managed.new_object(&script.class).ok_or(format!(
                "failed to initialize script class: {}",
                script.name
            ))?;
            Ok(Script { index, instance })
        } else {
            let scripts_asm = self.assemblies.get(&AssemblyType::Scripts).unwrap();
            let class = self
                .managed
                .get_class(scripts_asm, name.as_ref())
                .ok_or(format!("unknown class: {}", name.as_ref()))?;
            self.managed.get_meta_data(&class);
            let name: Cow<'static, str> = name.as_ref().to_string().into();
            let index = self.scripts.len();
            self.fullname_to_script.insert(name.clone(), index);
            self.scripts.push(Type {
                name,
                class,
                methods: Default::default(),
            });

            let script = &self.scripts[index];
            let instance = self.managed.new_object(&script.class).ok_or(format!(
                "failed to initialize script class: {}",
                script.name
            ))?;
            Ok(Script { index, instance })
        }
    }

    pub fn get_method<'s>(
        &self,
        handle: &'s Script,
        name: impl std::fmt::Display,
        args: i32,
    ) -> Option<Invokable<'s>> {
        if let Some(script) = self.scripts.get(handle.index) {
            return match script.methods.borrow_mut().entry((name.to_string(), args)) {
                Entry::Occupied(entry) => Some(Invokable {
                    instance: &handle.instance,
                    method: entry.get().clone(),
                    invoke: self.managed.runtime_invoke,
                }),
                Entry::Vacant(entry) => {
                    let method = Rc::new(self.managed.get_method(
                        &script.class,
                        &entry.key().0,
                        args,
                    )?);
                    entry.insert(method.clone());
                    Some(Invokable {
                        instance: &handle.instance,
                        method,
                        invoke: self.managed.runtime_invoke,
                    })
                }
            };
        }
        None
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
}

pub type Destroy = unsafe extern "system" fn(*const c_void) -> i32;
pub type SetFieldValue = 
    unsafe extern "system" fn(*const c_void, *const c_void, *const c_void) -> i32;
pub type Invoke =
    unsafe extern "system" fn(*const c_void, *const c_void, *const *const c_void) -> i32;

pub struct Managed {
    pub(crate) ping: unsafe extern "system" fn(*mut u32) -> i32,
    pub(crate) destroy: Destroy,
    pub(crate) free: unsafe extern "system" fn(*const c_void) -> i32,

    pub(crate) create_scope: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) load_from_path:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void) -> i32,
    pub(crate) unload_scope: unsafe extern "system" fn(*const c_void) -> i32,

    pub(crate) get_class:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void) -> i32,

    pub(crate) new: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) is_assignable_from:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut i32) -> i32,
    pub(crate) get_method:
        unsafe extern "system" fn(*const c_void, *const c_void, i32, *mut *const c_void) -> i32,
    pub(crate) get_meta_data:
        unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) set_field_value: SetFieldValue,

    pub(crate) runtime_invoke: Invoke,
}

impl Managed {
    #[allow(clippy::missing_transmute_annotations)]
    pub fn new(host: &Hostfxr) -> Self {
        unsafe {
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
                free: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "Free",
                    "Host+FreeDelegate, Runtime",
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
                get_meta_data: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "GetMetaData",
                    "Host+GetMetaDataDelegate, Runtime",
                )),
                set_field_value: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "SetFieldValue",
                    "Host+SetFieldValueDelegate, Runtime",
                )),

                runtime_invoke: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "RuntimeInvoke",
                    "Host+RuntimeInvokeDelegate, Runtime",
                )),
            }
        }
    }

    pub fn ping(&self) -> bool {
        let mut out: u32 = 0;
        unsafe { (self.ping)(&raw mut out) };
        out == 1
    }

    pub fn free(&self, value: &CStr) {
        // TODO: Error Handling
        unsafe { (self.free)(value.as_ptr().cast()) };
    }

    pub fn create_scope(&self) -> Scope {
        let mut out: *const c_void = std::ptr::null();
        // TODO: Error handling
        unsafe { (self.create_scope)(std::ptr::null(), &raw mut out) };
        Scope::new(out, self.unload_scope)
    }

    pub fn load_from_path(&self, scope: &Scope, path: impl AsRef<Path>) -> Option<Assembly> {
        let mut path = path.as_ref().display().to_string();
        if !path.ends_with('\0') {
            path.push('\0');
        }

        let mut out: *const c_void = std::ptr::null();
        // TODO: Error handling
        unsafe { (self.load_from_path)(scope.as_ptr(), path.as_ptr().cast(), &raw mut out) };

        if out.is_null() {
            None
        } else {
            Some(Assembly::new(out))
        }
    }

    pub fn get_class(&self, assembly: &Assembly, name: impl std::fmt::Display) -> Option<Class> {
        let mut name = name.to_string();
        if !name.starts_with('\0') {
            name.push('\0');
        }
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.get_class)(assembly.as_ptr(), name.as_ptr().cast(), &raw mut out) };

        if out.is_null() {
            None
        } else {
            Some(Class::new(out, self.destroy))
        }
    }

    pub fn new_object(&self, class: &Class) -> Option<Object> {
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.new)(class.as_ptr(), &raw mut out) };

        if out.is_null() {
            None
        } else {
            Some(Object::new(out, self.set_field_value, self.destroy))
        }
    }

    pub fn is_assignable_from(&self, base: &Class, target: &Class) -> bool {
        // TODO: Error handling
        let mut out: i32 = 0;
        unsafe { (self.is_assignable_from)(base.as_ptr(), target.as_ptr(), &raw mut out) };
        out == 1
    }

    pub fn get_method(
        &self,
        class: &Class,
        name: impl std::fmt::Display,
        args: i32,
    ) -> Option<Method> {
        let mut name = name.to_string();
        if !name.starts_with('\0') {
            name.push('\0');
        }
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.get_method)(class.as_ptr(), name.as_ptr().cast(), args, &raw mut out) };

        if out.is_null() {
            None
        } else {
            Some(Method::new(out, self.destroy))
        }
    }

    pub fn get_meta_data(
        &self,
        class: &Class,
    ) {
        // TODO: Error handling
        let mut out: *const c_void = std::ptr::null();
        unsafe { (self.get_meta_data)(class.as_ptr(), &raw mut out) };

        if !out.is_null() {
            let payload = unsafe { CStr::from_ptr(out.cast()) };
            println!("{}", payload.to_string_lossy());
            unsafe { (self.free)(out) };
        }
    }

    pub fn set_field_value<A>(
        &self,
        instance: &Object,
        name: impl AsRef<str>,
        value: impl ManagedParam
    ) {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') { name.push('\0'); }

        // TODO: Error handling
        unsafe { (self.set_field_value)(instance.as_ptr(), name.as_ptr().cast(), value.into_managed_param()) };
    }

    pub fn invoke(&self, method: &Method, instance: Option<&Object>, args: &[*const c_void]) {
        // TODO: Error handling
        unsafe {
            (self.runtime_invoke)(
                method.as_ptr(),
                instance
                    .map(|v| v.as_ptr().cast())
                    .unwrap_or(std::ptr::null()),
                args.as_ptr(),
            )
        };
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
    set_field_value: SetFieldValue,
    destroy: Destroy,
}
unsafe impl Send for Object {}
unsafe impl Sync for Object {}
impl Object {
    fn new(inner: *const c_void, set_field_value: SetFieldValue, destroy: Destroy) -> Self {
        Self { inner, set_field_value, destroy }
    }

    pub fn set_field_value(
        &self,
        name: impl AsRef<str>,
        value: impl ManagedParam
    ) {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') { name.push('\0'); }

        // TODO: Error handling
        unsafe { (self.set_field_value)(self.inner, name.as_ptr().cast(), value.into_managed_param()) };
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
