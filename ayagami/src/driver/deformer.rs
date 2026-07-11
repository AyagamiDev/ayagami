use glam::{
    FloatExt,
    f32::{Vec2, Vec3},
};
use zerocopy_derive::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::core::{ArtMeshForm, RotForm, WarpForm};
use zerocopy::{transmute_mut, transmute_ref};

#[derive(Debug, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct VisualVals {
    pub(crate) opacity: f32,
    pub(crate) multiply_color: Vec3,
    pub(crate) screen_color: Vec3,
}

impl VisualVals {
    pub(crate) fn saturate(mut self) -> Self {
        self.opacity = self.opacity.saturate();
        self.multiply_color = self.multiply_color.saturate();
        self.screen_color = self.screen_color.saturate();
        self
    }
}

#[derive(Debug, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct RotFormVals {
    pub(crate) visual: VisualVals,
    pub(crate) scale: f32,
    pub(crate) angle: f32,
    pub(crate) pos: Vec2,
}

impl RotFormVals {
    pub(crate) fn new<'a>(f: &impl RotForm<'a>) -> Self {
        Self {
            visual: VisualVals {
                opacity: f.opacity(),
                multiply_color: f.multiply_color().into(),
                screen_color: f.screen_color().into(),
            }
            .saturate(),
            scale: f.scale(),
            angle: f.angle(),
            pos: f.position(),
        }
    }
}

#[derive(Debug, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct WarpFormVals {
    pub(crate) visual: VisualVals,
}

impl WarpFormVals {
    pub(crate) fn new<'a>(f: &impl WarpForm<'a>) -> Self {
        Self {
            visual: VisualVals {
                opacity: f.opacity(),
                multiply_color: f.multiply_color().into(),
                screen_color: f.screen_color().into(),
            }
            .saturate(),
        }
    }
}

#[derive(Debug, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub(crate) struct ArtMeshFormVals {
    pub(crate) visual: VisualVals,
}

impl ArtMeshFormVals {
    pub(crate) fn new<'a>(f: &impl ArtMeshForm<'a>) -> Self {
        Self {
            visual: VisualVals {
                opacity: f.opacity(),
                multiply_color: f.multiply_color().into(),
                screen_color: f.screen_color().into(),
            }
            .saturate(),
        }
    }
}

pub(crate) fn blend<T>(srcs: &[T], weights: &[f32]) -> T
where
    T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable,
{
    let mut tdst: T = T::new_zeroed();
    let dst: &mut [f32] = transmute_mut!(&mut tdst);

    let srcs = &srcs[..weights.len()];
    for i in 0..dst.len() {
        dst[i] = 0.;
        for (src, weight) in srcs.iter().zip(weights) {
            let src: &[f32] = transmute_ref!(src);
            dst[i] += src[i] * weight;
        }
    }

    tdst
}

pub(crate) fn blend_arrays<T>(srcs: &[&[T]], dst: &mut [T], weights: &[f32])
where
    T: zerocopy::FromBytes + zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable,
{
    let dst: &mut [f32] = transmute_mut!(dst);

    let srcs = &srcs[..weights.len()];
    let n = dst.len();
    if n == 0 {
        return;
    }

    const STRIDE: usize = 64;
    const USE_UNSAFE: bool = true;

    let l = n & !(STRIDE - 1);
    for src in srcs.iter() {
        let src: &[f32] = transmute_ref!(*src);
        src[n - 1];
    }
    for i in (0..l).step_by(STRIDE) {
        dst[i..i + STRIDE].fill(0.);
        for (src, weight) in srcs.iter().zip(weights) {
            let src: &[f32] = transmute_ref!(*src);
            if USE_UNSAFE {
                let d = &mut dst[i..];
                let s = &src[i..];
                for j in 0..STRIDE {
                    unsafe {
                        *d.get_unchecked_mut(j) += s.get_unchecked(j) * weight;
                    }
                }
            } else {
                let d = &mut dst[i..][..STRIDE];
                let s = &src[i..][..STRIDE];
                for (a, b) in d.iter_mut().zip(s) {
                    *a += *b * weight;
                }
            }
        }
    }
    for i in l..n {
        dst[i] = 0.;
        for (src, weight) in srcs.iter().zip(weights) {
            let src: &[f32] = transmute_ref!(*src);
            dst[i] += src[i] * weight;
        }
    }
}
