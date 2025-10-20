use std::{collections::BTreeMap, path::{Path, PathBuf}};

use super::diagnostic::{Diagnostic, Severity};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct Project {
    property_group: Vec<BTreeMap<String, String>>,
}

pub struct Builder {
    /// Path to the dotnet executable
    dotnet: PathBuf,
    net: String,
}

impl Builder {
    pub fn new(base: impl AsRef<Path>, net: impl AsRef<str>) -> Self {
        Self {
            #[cfg(target_os = "windows")]
            dotnet: base.as_ref().join("dotnet.exe"),
            #[cfg(not(target_os = "windows"))]
            dotnet: base.as_ref().join("dotnet"),
            net: net.as_ref().to_string(),
        }
    }

    pub fn build(&self, project_file: impl AsRef<Path>) -> std::io::Result<(String, PathBuf)> {
        let csproj = project_file.as_ref();
        let base = csproj.parent().unwrap();

        let data = std::fs::read_to_string(csproj)?;
        let project: Project = serde_xml_rs::from_str(&data).unwrap();

        let name = project.property_group
            .iter()
            .filter_map(|v| v.get("AssemblyName").cloned())
            .collect::<Vec<_>>()
            .first()
            .cloned()
            .unwrap_or(csproj.file_stem().unwrap().to_string_lossy().to_string());


        let now = std::time::Instant::now();
        let build_log = base.join("build.log");
        let result = std::process::Command::new(&self.dotnet)
            .arg("build")
            .arg(csproj)
            .args(["-c", "Release"])
            .arg("-flp:v=q")
            .arg(format!("-flp:logfile={}", build_log.display()))
            .output()
            .unwrap();

        log::debug!("[compile] {name} {:.3} s", now.elapsed().as_secs_f64());

        if !result.status.success() {
            panic!("dotnet failed to build '{name}'");
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

        Ok((name, base.join("bin").join("Release").join(&self.net)))
    }
}
