pub(crate) mod types;
#[macro_use]
pub(crate) mod macros;
pub(crate) mod classes;
pub(crate) mod model;
pub(crate) mod parse;

pub use classes::ParsedModel;
pub use parse::ParseError;

use strum::VariantArray;
use strum_macros::FromRepr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, FromRepr)]
pub enum Version {
    V3_0 = 1,
    V3_3 = 2,
    V4_0 = 3,
    V4_2 = 4,
    V5_0 = 5,
    V5_3 = 6,
}

impl Version {
    pub(crate) const fn pass(&self) -> Pass {
        match self {
            Version::V3_0 => Pass::V3_0,
            Version::V3_3 => Pass::V3_3,
            Version::V4_0 => Pass::V4_0,
            Version::V4_2 => Pass::V4_2,
            Version::V5_0 => Pass::V5_0,
            Version::V5_3 => Pass::V5_3,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, VariantArray)]
pub(crate) enum Pass {
    Base,
    V3_0,
    V3_3,
    V4_0,
    V4_2A,
    V4_2B,
    V4_2,
    V5_0,
    V5_3,
    Internal,
}
