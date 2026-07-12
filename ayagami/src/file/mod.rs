pub mod types;
#[macro_use]
pub(crate) mod macros;
pub mod classes;
pub mod model;
pub(crate) mod parse;

pub use classes::ParsedModel;
pub use parse::ParseError;

use strum::VariantArray;
use strum_macros::FromRepr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, FromRepr, derive_more::Display)]
pub enum Version {
    #[display("3.0")]
    V3_0 = 1,
    #[display("3.3")]
    V3_3 = 2,
    #[display("4.0")]
    V4_0 = 3,
    #[display("4.2")]
    V4_2 = 4,
    #[display("5.0")]
    V5_0 = 5,
    #[display("5.3")]
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
