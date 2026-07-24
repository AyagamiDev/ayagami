use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Deref;
use zerocopy_derive::{FromBytes, IntoBytes};

use super::classes::ParsedModel;

use super::ParseError;

#[derive(Debug, IntoBytes, FromBytes)]
#[repr(C)]
pub(crate) struct Identifier([u8; 0x40]);

impl Default for Identifier {
    fn default() -> Self {
        use zerocopy::FromZeros;
        Self::new_zeroed()
    }
}

impl TryFrom<Identifier> for String {
    type Error = ParseError;

    fn try_from(ident: Identifier) -> Result<Self, ParseError> {
        let s = str::from_utf8(&ident.0).map_err(|a| ParseError::InvalidValue(a.to_string()))?;
        let pos = s.find('\0').unwrap_or(s.len());
        Ok(s[..pos].into())
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, IntoBytes, FromBytes)]
#[repr(transparent)]
pub struct Bool32(u32);

pub const FALSE: Bool32 = Bool32(0);
pub const TRUE: Bool32 = Bool32(1);

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, IntoBytes, FromBytes)]
pub struct U32Pair(u32, u32);

pub(crate) mod private {
    use super::super::classes::ParsedModel;
    use std::fmt::Debug;

    pub trait Object: RawObject {
        type View<'a>: View<'a>;
    }

    pub trait RawObject {
        type Idx: PrivRef + Copy;
        type OptIdx: PrivOptRef + Copy;
    }

    pub trait PrivRef: Sized + super::Reference {
        const STRIDE: u32 = 1;

        fn new(i: u32) -> Self;
        fn offset(&self, offset: u32) -> Self {
            Self::new(self.get() + offset * Self::STRIDE)
        }
    }

    pub trait PrivOptRef: Sized + super::OptReference {
        const STRIDE: u32 = 1;

        fn new(i: Option<u32>) -> Self;
        fn offset(&self, offset: u32) -> Self {
            Self::new(Some(self.get().unwrap() + offset * Self::STRIDE))
        }
    }

    pub trait View<'model>: Sized + Debug {
        type Object: super::Object;

        fn get(model: &'model ParsedModel, idx: <Self::Object as RawObject>::Idx) -> Option<Self>;
        fn get_ref(
            model: &'model ParsedModel,
            idx: <Self::Object as RawObject>::Idx,
        ) -> Option<super::ViewRef<'model, Self>>;
        fn into_ref(self) -> super::ViewRef<'model, Self>;
        fn set_parent_idx(&mut self, idx: u32);
    }

    pub trait ChildView<'a>: Sized {
        type Parent: Object;

        fn with_parent(self, parent: &<Self::Parent as Object>::View<'a>) -> Self;
        fn with_parent_idx(self, idx: u32) -> Self;
        fn parent(&self) -> <Self::Parent as Object>::View<'a>;
    }
}
pub(crate) use private::*;

pub trait Reference: Sized {
    fn get(&self) -> u32;
}

pub trait OptReference: Sized {
    fn get(&self) -> Option<u32>;
}

impl From<Bool32> for bool {
    fn from(value: Bool32) -> Self {
        match value.0 {
            0 => false,
            1 => true,
            a => panic!("Unexpected Bool32 value {0}", a),
        }
    }
}

#[derive(derive_more::Debug)]
pub struct ViewRef<'a, T: Debug>(pub(crate) T, #[debug(skip)] pub(crate) PhantomData<&'a T>);

impl<'a, T: std::fmt::Debug> ViewRef<'a, T> {
    pub(crate) fn new(v: T) -> Self {
        Self(v, PhantomData)
    }
}

impl<'a, T: Debug> Deref for ViewRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct ItemCollection<'a, T: Debug> {
    pub(crate) model: &'a ParsedModel,
    pub(crate) start: u32,
    pub(crate) limit: u32,
    pub(crate) parent: Option<u32>,
    p: PhantomData<T>,
}

impl<'a, T: 'a + View<'a>> ItemCollection<'a, T> {
    pub(crate) fn new(model: &'a ParsedModel, start: u32, limit: u32) -> Self {
        Self {
            model,
            start,
            limit,
            parent: None,
            p: PhantomData,
        }
    }

    pub(crate) fn index(&self, index: usize) -> Option<ViewRef<'a, T>> {
        let index: u32 = index.try_into().unwrap();

        if (self.start + index) >= self.limit {
            None
        } else {
            T::get(
                self.model,
                <T::Object as RawObject>::Idx::new(self.start).offset(index),
            )
            .map(|mut v| {
                if let Some(idx) = self.parent {
                    v.set_parent_idx(idx);
                }
                v.into_ref()
            })
        }
    }

    pub(crate) fn count(&self) -> usize {
        (self.limit - self.start) as usize
    }
}
