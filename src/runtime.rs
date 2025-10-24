use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{hash_map::Entry, HashMap},
    ffi::{c_void, CStr},
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc
};

use bevy::ecs::resource::Resource;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use crate::{dotnet, hostfxr::Hostfxr, Error, Result};

include!(concat!(std::env!("OUT_DIR"), "/constants.rs"));

pub struct Paths {
    pub exe: PathBuf,
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
impl std::fmt::Display for AssemblyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine => write!(f, "Engine"),
            Self::Scripts => write!(f, "Scripts"),
        }
    }
}
impl AssemblyType {
    pub fn path(&self, base: impl AsRef<Path>) -> PathBuf {
        base.as_ref().join("managed").join(format!("{self}.dll"))
    }
}

// TODO: Add Reflect which fetches cached public fields
pub struct Type {
    pub(crate) name: Cow<'static, str>,
    pub(crate) class: Class,

    pub(crate) methods: RefCell<HashMap<(String, i32), Rc<Method>>>,
    pub(crate) metadata: MetaData,
}

pub struct Invokable<'s> {
    instance: &'s Object,
    method: Rc<Method>,
    invoke: Invoke,
}
impl<'s> Invokable<'s> {
    pub fn invoke(&self, args: impl ManagedParams) -> Result<()> {
        let params = args.into_managed_params();
        let mut err: i32 = -1;
        unsafe {
            (self.invoke)(
                self.method.as_ptr(),
                self.instance.as_ptr(),
                params.as_ptr(),
                &raw mut err,
            )
        };
        if err > 0 { return Err(Error::from(err)); }
        Ok(())
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
        Vec::from([self.0.into_managed_param(), self.1.into_managed_param()])
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

/// # Saftey
/// Not safe when used outside of bevy's ecs like in an alternate thread not managed by bevy
#[allow(dead_code)]
#[derive(Resource)]
pub struct Runtime {
    pub paths: Paths,
    pub versions: Versions,

    host: Hostfxr,
    pub library: RuntimeLibrary,

    pub scope: Option<Scope>,
    pub assemblies: HashMap<AssemblyType, Assembly>,

    pub fullname_to_script: HashMap<Cow<'static, str>, usize>,
    pub scripts: Vec<Rc<Type>>,
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
    pub fn new() -> Result<Self> {
        let exe_parent = std::env::current_exe().unwrap();
        let exe_dir = exe_parent.parent().unwrap();

        let dotnet = dotnet::get_path().ok_or(Error::PathNotFound)?;
        let hostfxr_path = dotnet.join("host").join("fxr");

        let versions = Versions {
            framework: FRAMEWORK.to_string(),
            net: NET.to_string(),
        };

        log::debug!("Versions:");
        log::debug!("    net: {}", versions.net);
        log::debug!("    framework: {}", versions.framework);

        let paths = Paths {
            exe: exe_dir.to_path_buf(),
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
        Ok(Self {
            library: RuntimeLibrary::new(&host),
            host,
            paths,
            versions,
            scope: None,

            fullname_to_script: Default::default(),
            assemblies: Default::default(),
            scripts: Default::default(),
        })
    }

    /// Create a new instance of a class associated with a certain script index
    pub fn create(&self, name: impl AsRef<str>) -> Result<Script> {
        if let Some(index) = self.fullname_to_script.get(name.as_ref()).copied() {
            let script = &self.scripts[index];
            let instance = self.library.new_object(&script.class)?.ok_or(Error::UnknownManaged)?;
            Ok(Script { index, instance })
        } else {
            Err(Error::ClassNotRegistered)
        }
    }

    pub fn register(&mut self, name: impl AsRef<str>) -> Result<()> {
        let scripts_asm = self.assemblies.get(&AssemblyType::Scripts).ok_or(Error::AssemblyNotLoaded)?;

        let class = self
            .library
            .get_class(scripts_asm, name.as_ref())?
            .ok_or(Error::ClassNotFound)?;

        let metadata = self.library.get_meta_data(&class)?;
        let name: Cow<'static, str> = name.as_ref().to_string().into();
        let index = self.scripts.len();

        self.fullname_to_script.insert(name.clone(), index);
        self.scripts.push(Rc::new(Type {
            name,
            class,
            methods: Default::default(),
            metadata,
        }));

        Ok(())
    }

    pub fn load(&mut self, assembly: AssemblyType) -> Result<()> {
        // TODO: Make the load more dynamic to include more assemblies
        if let Some(scope) = self.scope.as_ref() {
            let asm = self.library.load_from_path(scope, assembly.path(&self.paths.exe))?.ok_or(Error::PathNotFound)?;
            self.assemblies.insert(assembly, asm);
        }
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.scripts.truncate(0);
        self.fullname_to_script = HashMap::new();
        self.assemblies.clear();

        if let Some(scope) = self.scope.replace(self.library.create_scope()) {
            let mut err: i32 = -1;
            unsafe { (self.library.unload_scope)(scope.as_ptr(), &raw mut err) };
            if err > 0 { return Err(Error::from(err)); }
        }

        Ok(())
    }

    pub fn get_method<'s>(
        &self,
        handle: &'s Script,
        name: impl std::fmt::Display,
        args: i32,
    ) -> Result<Option<Invokable<'s>>> {
        if let Some(script) = self.scripts.get(handle.index) {
            return match script.methods.borrow_mut().entry((name.to_string(), args)) {
                Entry::Occupied(entry) => Ok(Some(Invokable {
                    instance: &handle.instance,
                    method: entry.get().clone(),
                    invoke: self.library.runtime_invoke,
                })),
                Entry::Vacant(entry) => {
                    let method = Rc::new(match self.library.get_method(
                        &script.class,
                        &entry.key().0,
                        args,
                    )? {
                        Some(m) => m,
                        None => return Ok(None)
                    });
                    entry.insert(method.clone());
                    Ok(Some(Invokable {
                        instance: &handle.instance,
                        method,
                        invoke: self.library.runtime_invoke,
                    }))
                }
            };
        }
        Ok(None)
    }

    pub fn get_meta_data(&mut self, handle: &Script) -> &MetaData {
        let script = self.scripts.get(handle.index).unwrap();
        &script.metadata
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
pub type Unload = unsafe extern "system" fn(*const c_void, *mut i32) -> i32;
pub type SetFieldValue =
    unsafe extern "system" fn(*const c_void, *const c_void, *const c_void, *mut i32) -> i32;
pub type GetFieldValue =
    unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void, *mut i32) -> i32;
pub type Invoke =
    unsafe extern "system" fn(*const c_void, *const c_void, *const *const c_void, *mut i32) -> i32;

pub struct RuntimeLibrary {
    pub(crate) ping: unsafe extern "system" fn(*mut u32) -> i32,
    pub(crate) destroy: Destroy,
    pub(crate) free: unsafe extern "system" fn(*const c_void) -> i32,

    pub(crate) create_scope: unsafe extern "system" fn(*const c_void, *mut *const c_void) -> i32,
    pub(crate) load_from_path:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void, *mut i32) -> i32,
    pub(crate) unload_scope: Unload,

    pub(crate) get_class:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut *const c_void, *mut i32) -> i32,

    pub(crate) new: unsafe extern "system" fn(*const c_void, *mut *const c_void, *mut i32) -> i32,
    pub(crate) is_assignable_from:
        unsafe extern "system" fn(*const c_void, *const c_void, *mut i32, *mut i32) -> i32,
    pub(crate) get_method:
        unsafe extern "system" fn(*const c_void, *const c_void, i32, *mut *const c_void, *mut i32) -> i32,
    pub(crate) get_meta_data: unsafe extern "system" fn(*const c_void, *mut *const c_void, *mut i32) -> i32,
    pub(crate) set_field_value: SetFieldValue,
    pub(crate) get_field_value: GetFieldValue,
    pub(crate) set_property_value: SetFieldValue,
    pub(crate) get_property_value: GetFieldValue,

    pub(crate) runtime_invoke: Invoke,
}

impl RuntimeLibrary {
    #[allow(clippy::missing_transmute_annotations)]
    pub fn new(host: &Hostfxr) -> Self {
        unsafe {
            RuntimeLibrary {
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
                get_field_value: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "GetFieldValue",
                    "Host+GetFieldValueDelegate, Runtime",
                )),
                set_property_value: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "SetPropertyValue",
                    "Host+SetPropertyValueDelegate, Runtime",
                )),
                get_property_value: std::mem::transmute(host.get_function_with_delegate(
                    "Host, Runtime",
                    "GetPropertyValue",
                    "Host+GetPropertyValueDelegate, Runtime",
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
        unsafe { (self.free)(value.as_ptr().cast()) };
    }

    pub fn create_scope(&self) -> Scope {
        unsafe {
            let mut out: *const c_void = std::ptr::null();
            (self.create_scope)(std::ptr::null(), &raw mut out);
            Scope::new(out, self.unload_scope)
        }
    }

    pub fn load_from_path(&self, scope: &Scope, path: impl AsRef<Path>) -> Result<Option<Assembly>> {
        let mut path = path.as_ref().display().to_string();
        if !path.ends_with('\0') {
            path.push('\0');
        }

        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.load_from_path)(scope.as_ptr(), path.as_ptr().cast(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        Ok(if out.is_null() {
            None
        } else {
            Some(Assembly::new(out))
        })
    }

    pub fn get_class(&self, assembly: &Assembly, name: impl std::fmt::Display) -> Result<Option<Class>> {
        let mut name = name.to_string();
        if !name.starts_with('\0') {
            name.push('\0');
        }
        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.get_class)(assembly.as_ptr(), name.as_ptr().cast(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        Ok(if out.is_null() {
            None
        } else {
            Some(Class::new(out, self.destroy))
        })
    }

    pub fn new_object(&self, class: &Class) -> Result<Option<Object>> {
        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.new)(class.as_ptr(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        Ok(if out.is_null() {
            None
        } else {
            Some(Object {
                inner: out,
                get_field_value: self.get_field_value,
                set_field_value: self.set_field_value,
                get_property_value: self.get_property_value,
                set_property_value: self.set_property_value,
                destroy: self.destroy,
                free: self.free,
            })
        })
    }

    pub fn is_assignable_from(&self, base: &Class, target: &Class) -> Result<bool> {
        let mut out: i32 = 0;
        let mut err: i32 = -1;
        unsafe { (self.is_assignable_from)(base.as_ptr(), target.as_ptr(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }
        Ok(out == 1)
    }

    pub fn get_method(
        &self,
        class: &Class,
        name: impl std::fmt::Display,
        args: i32,
    ) -> Result<Option<Method>> {
        let mut name = name.to_string();
        if !name.starts_with('\0') {
            name.push('\0');
        }
        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.get_method)(class.as_ptr(), name.as_ptr().cast(), args, &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        Ok(if out.is_null() {
            None
        } else {
            Some(Method::new(out, self.destroy))
        })
    }

    pub fn get_meta_data(&self, class: &Class) -> Result<MetaData> {
        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.get_meta_data)(class.as_ptr(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        if !out.is_null() {
            let payload = unsafe { CStr::from_ptr(out.cast()) };
            let pref = payload.to_string_lossy();
            let value = serde_json::from_str(&pref)?;
            unsafe { (self.free)(out) };

            return Ok(value);
        }

        Ok(Default::default())
    }

    pub fn set_field_value(
        &self,
        instance: &Object,
        name: impl AsRef<str>,
        value: impl ManagedParam,
    ) -> Result<()> {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') {
            name.push('\0');
        }

        let mut err: i32 = -1;
        unsafe {
            (self.set_field_value)(
                instance.as_ptr(),
                name.as_ptr().cast(),
                value.into_managed_param(),
                &raw mut err,
            )
        };
        if err > 0 { return Err(Error::from(err)); }
        Ok(())
    }

    pub fn invoke(&self, method: &Method, instance: Option<&Object>, args: &[*const c_void]) -> Result<()> {
        let mut err: i32 = -1;
        unsafe {
            (self.runtime_invoke)(
                method.as_ptr(),
                instance
                    .map(|v| v.as_ptr().cast())
                    .unwrap_or(std::ptr::null()),
                args.as_ptr(),
                &raw mut err,
            )
        };
        if err > 0 { return Err(Error::from(err)); }
        Ok(())
    }
}

pub trait Wrapper {
    fn as_ptr(&self) -> *const c_void;
}

pub struct Scope {
    inner: *const c_void,
    unload: Unload,
}
impl Scope {
    fn new(inner: *const c_void, unload: Unload) -> Self {
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
        let mut err: i32 = -1;
        unsafe { (self.unload)(self.inner, &raw mut err) };
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
    get_field_value: GetFieldValue,
    set_field_value: SetFieldValue,
    get_property_value: GetFieldValue,
    set_property_value: SetFieldValue,
    destroy: Destroy,
    free: Destroy,
}
unsafe impl Send for Object {}
unsafe impl Sync for Object {}
impl Object {
    pub fn set_field_value(&self, name: impl AsRef<str>, value: impl ManagedParam) -> Result<()> {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') {
            name.push('\0');
        }

        let mut err: i32 = -1;
        unsafe {
            (self.set_field_value)(self.inner, name.as_ptr().cast(), value.into_managed_param(), &raw mut err)
        };
        if err > 0 { return Err(Error::from(err)); }
        Ok(())
    }

    pub fn get_field_value<A: DeserializeOwned>(&self, name: impl AsRef<str>) -> Result<Option<A>> {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') {
            name.push('\0');
        }

        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.get_field_value)(self.inner, name.as_ptr().cast(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        if out.is_null() {
            return Ok(None);
        }

        let payload = unsafe { CStr::from_ptr(out.cast()) };
        let payload_ref = payload.to_string_lossy();
        let value = serde_json::from_str(&payload_ref)?;

        unsafe { (self.free)(out) };

        Ok(Some(value))
    }

    pub fn set_property_value(&self, name: impl AsRef<str>, value: impl ManagedParam) -> Result<()> {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') {
            name.push('\0');
        }

        let mut err: i32 = -1;
        unsafe {
            (self.set_property_value)(self.inner, name.as_ptr().cast(), value.into_managed_param(), &raw mut err)
        };
        if err > 0 { return Err(Error::from(err)); }
        Ok(())
    }

    pub fn get_property_value<A: DeserializeOwned>(&self, name: impl AsRef<str>) -> Result<Option<A>> {
        let mut name = name.as_ref().to_string();
        if !name.ends_with('\0') {
            name.push('\0');
        }

        let mut out: *const c_void = std::ptr::null();
        let mut err: i32 = -1;
        unsafe { (self.get_property_value)(self.inner, name.as_ptr().cast(), &raw mut out, &raw mut err) };
        if err > 0 { return Err(Error::from(err)); }

        if out.is_null() {
            return Ok(None);
        }

        let payload = unsafe { CStr::from_ptr(out.cast()) };
        let payload_ref = payload.to_string_lossy();
        let value = serde_json::from_str::<A>(&payload_ref)?;

        unsafe { (self.free)(out) };

        Ok(Some(value))
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

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct MetaData {
    pub fields: Vec<Field>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct Field {
    pub name: String,
    pub is_static: bool,
    pub custom_attributes: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct Property {
    pub name: String,
    pub is_static: bool,
    pub custom_attributes: Vec<Value>,
    pub can_read: bool,
    pub can_write: bool,
}
