pub mod diagnostic;

mod builder;
use std::path::PathBuf;

pub use builder::Builder;

pub fn get_path() -> Option<PathBuf> {
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
