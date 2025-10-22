use serde::Deserialize;

pub enum Version {
    /// Use the latest of a specific Dotnet version
    ///
    /// This will compile will the latest version found on your system
    /// that matches.
    ///
    /// # Example
    /// `.NET 9` latest framework is `9.0.10` as of October 14, 2025
    /// `.NET 8` latest framework is `8.0.21` as of October 14, 2025
    /// `.NET 7` latest framework is `7.0.20` as of May 28, 2024
    Net(u8),
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
    Framework(String),
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {

        let value = String::deserialize(deserializer)?.to_ascii_lowercase();
        if let Some(version) = value.strip_prefix("net") {
            let major = version.split_once(".").unwrap().0;
            Ok(Self::Net(major.parse().unwrap()))
        } else {
            Ok(Self::Framework(value))
        }
    }
}

impl<A: AsRef<str>> PartialEq<A> for Version {
    fn eq(&self, other: &A) -> bool {
        match self {
            Self::Net(v) => format!("net{v}.0") == other.as_ref(),
            Self::Framework(v) => v == other.as_ref(),
        }
    }
}

impl Default for Version {
    fn default() -> Self {
        Version::Net(8)
    }
}

#[allow(dead_code)]
#[derive(Default, serde::Deserialize)]
pub struct Config {
    pub version: Version,
}
