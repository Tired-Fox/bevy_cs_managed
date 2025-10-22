use std::{ffi::c_void, sync::Arc};

use hostfxr_sys::{
    dlopen2::wrapper::Container, get_function_pointer_fn, hostfxr_delegate_type, hostfxr_handle,
    load_assembly_fn, wrapper::Hostfxr as HostfxrLibrary,
};

use super::runtime::Paths;

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

#[derive(Clone)]
pub struct Hostfxr {
    pub lib: Arc<Container<HostfxrLibrary>>,
    pub ctx: hostfxr_handle,
    pub get_function_pointer: get_function_pointer_fn,
}
unsafe impl Send for Hostfxr {}
unsafe impl Sync for Hostfxr {}

impl Hostfxr {
    pub fn new(paths: &Paths) -> Self {
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

pub struct Scope {
    inner: *const c_void,
}

impl Scope {
    pub fn new(inner: *const c_void) -> Self {
        Self { inner }
    }

    pub fn as_ptr(&self) -> *const c_void {
        self.inner
    }
}
