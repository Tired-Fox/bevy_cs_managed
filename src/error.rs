#[derive(Debug)]
pub enum Error {
    ClassNotFound,
    MethodNotFound,
    FieldNotFound,
    PropertyNotFound,
    ReadonlyField,
    MissingGetter,
    MissingSetter,
    MissingRequiredArgument,
    PathNotFound,
    AssemblyNotLoaded,
    ClassNotRegistered,
    UnknownManaged,
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl From<i32> for Error {
    fn from(value: i32) -> Self {
        match value {
            1 => Error::ClassNotFound,
            2 => Error::MethodNotFound,
            3 => Error::FieldNotFound,
            4 => Error::PropertyNotFound,
            5 => Error::ReadonlyField,
            6 => Error::MissingGetter,
            7 => Error::MissingSetter,
            8 => Error::MissingRequiredArgument,
            9 => Error::PathNotFound,
            10 => Error::AssemblyNotLoaded,
            11 => Error::ClassNotRegistered,
            _ => Error::UnknownManaged,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssemblyNotLoaded => write!(f, "attempt to use assembly that was NOT loaded"),
            Self::ClassNotRegistered => write!(f, "script class is not registered with the runtime"),
            Self::PathNotFound => write!(f, "path not found"),
            Self::ClassNotFound => write!(f, "class not found"),
            Self::MethodNotFound => write!(f, "method not found"),
            Self::FieldNotFound => write!(f, "field not found"),
            Self::PropertyNotFound => write!(f, "property not found"),
            Self::ReadonlyField => write!(f, "field is readonly"),
            Self::MissingGetter => write!(f, "property is missing a getter"),
            Self::MissingSetter => write!(f, "property is missing a setter"),
            Self::MissingRequiredArgument => write!(f, "missing required argument: was `null`"),
            Self::UnknownManaged => write!(f, "an unknown managed c# error occured"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
