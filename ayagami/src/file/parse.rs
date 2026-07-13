#![allow(unused)]

use super::classes::*;
use super::types::*;
use super::{Pass, Version};
use crate::core;
use crate::core::Model;
use crate::file::Pass::V3_0;
use ParseError::*;
use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use log::{debug, info, warn};
use paste::paste;
use std::io::Read;
use std::ops::Deref;
#[macro_use]
use strum_macros::{FromRepr};
use strum::VariantArray;
use thiserror::Error;
use zerocopy::transmute_mut;
use zerocopy_derive::{FromBytes, FromZeros, IntoBytes};

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Invalid magic value {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("Unknown/unsupported version {0}")]
    UnknownVersion(u32),
    #[error("Invalid reference: {0}")]
    InvalidReference(String),
    #[error("Invalid value: {0}")]
    InvalidValue(String),
    #[error("Invalid offset: {0}")]
    InvalidOffset(String),
    #[error("Invalid padding: Non-zero {1:#x} at offset {0:#x}")]
    InvalidPadding(usize, u8),
    #[error("Unaligned item count: {0} count {1} not a multiple of {2}")]
    UnalignedItemCount(&'static str, usize, usize),
    #[error("Duplicate reference: {0}")]
    DuplicateReference(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub(crate) trait Parsable {
    fn parse(&mut self, pass: Pass, data: &mut SectionReader) -> Result<(), ParseError>;

    fn parse_prim<T: ReadArray>(
        field: &mut Vec<T>,
        name: &str,
        count: usize,
        data: &mut SectionReader,
    ) -> Result<(), ParseError> {
        debug!(
            "[{0}: {1:#x}] Load [{3}] {2}",
            data.section, data.offsets[data.section], name, count
        );
        assert!(field.is_empty());
        field.resize_with(count, Default::default);
        T::read_array(field, data)
    }

    fn parse_ref<T: Reference + ReadArray>(
        field: &mut Vec<T>,
        name: &str,
        count: usize,
        data: &mut SectionReader,
    ) -> Result<(), ParseError> {
        Self::parse_prim(field, name, count, data)?;
        Ok(())
    }

    fn parse_opt_ref<T: OptReference + ReadArray>(
        field: &mut Vec<T>,
        name: &str,
        count: usize,
        data: &mut SectionReader,
    ) -> Result<(), ParseError> {
        Self::parse_prim(field, name, count, data)?;
        Ok(())
    }

    fn parse_arrayref<T: Reference + ReadArray>(
        index_field: &mut Vec<T>,
        count_field: &mut Vec<u32>,
        index_name: &str,
        count_name: &str,
        count: usize,
        data: &mut SectionReader,
    ) -> Result<(), ParseError> {
        Self::parse_prim(index_field, index_name, count, data)?;
        Self::parse_prim(count_field, count_name, count, data)?;
        Ok(())
    }

    fn parse_convert<F: ReadArray, T>(
        field: &mut Vec<T>,
        name: &str,
        count: usize,
        data: &mut SectionReader,
    ) -> Result<(), ParseError>
    where
        T: TryFrom<F, Error = ParseError>,
    {
        let mut vec: Vec<F> = Vec::new();
        Self::parse_prim(&mut vec, name, count, data)?;
        *field = vec
            .into_iter()
            .map(|a| <F as TryInto<T>>::try_into(a))
            .collect::<Result<_, _>>()?;
        Ok(())
    }
}

pub(crate) struct SectionReader<'a> {
    pub rdr: &'a mut dyn Read,
    pub p: usize,
    pub section: usize,
    pub offsets: Vec<u32>,
}

impl<'a> SectionReader<'a> {
    pub(crate) fn read_exact(&mut self, d: &mut [u8]) -> Result<(), ParseError> {
        self.rdr.read_exact(d)?;
        self.p += d.len();
        Ok(())
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, ParseError> {
        let ret = self.rdr.read_u32::<LittleEndian>()?;
        self.p += 4;
        Ok(ret)
    }

    pub(crate) fn read_f32(&mut self) -> Result<f32, ParseError> {
        let ret = self.rdr.read_f32::<LittleEndian>()?;
        self.p += 4;
        Ok(ret)
    }

    pub(crate) fn read_u32_into(&mut self, d: &mut [u32]) -> Result<(), ParseError> {
        let ret = self.rdr.read_u32_into::<LittleEndian>(d)?;
        self.p += 4 * d.len();
        Ok(ret)
    }

    pub(crate) fn next_section(&mut self) -> Result<(), ParseError> {
        self.advance_to(self.offsets[self.section])?;
        self.section += 1;
        Ok(())
    }

    pub(crate) fn advance_to(&mut self, to: u32) -> Result<(), ParseError> {
        const MAX_SKIP: usize = 0x1000;
        let mut buf = [0; MAX_SKIP];

        let to = to as usize;
        if to < self.p {
            Err(InvalidOffset(format!(
                "Tried to seek from {0:#x} to {1:#x}",
                self.p, to
            )))
        } else if to == self.p {
            Ok(())
        } else {
            let skip = to - self.p;
            // Should only skip padding, not large blocks
            if skip > MAX_SKIP {
                Err(InvalidOffset(format!(
                    "Tried to seek from {0:#x} to {1:#x} ({2:#x} bytes > {3:#x})",
                    self.p, to, skip, MAX_SKIP
                )))
            } else {
                self.rdr.read_exact(&mut buf[..skip])?;
                for (i, b) in buf[..skip].iter().enumerate() {
                    if *b != 0 {
                        return Err(InvalidPadding(self.p + i, *b));
                    }
                }
                self.p = to;
                Ok(())
            }
        }
    }
}

pub(crate) trait ReadArray: Sized + Default {
    fn read_array(dest: &mut [Self], data: &mut SectionReader) -> Result<(), ParseError>;
}

primitive_reader!(u8, Read::read_exact);
primitive_reader!(u16, ReadBytesExt::read_u16_into::<LittleEndian>);
primitive_reader!(u32, ReadBytesExt::read_u32_into::<LittleEndian>);
primitive_reader!(i32, ReadBytesExt::read_i32_into::<LittleEndian>);
primitive_reader!(f32, ReadBytesExt::read_f32_into::<LittleEndian>);

transparent_reader!(Bool32, u32);
transparent_reader!(U32Pair, u32);
transparent_reader!(Identifier, u8);
transparent_reader!(core::Coord, f32);

const FILE_MAGIC: &[u8; 4] = b"MOC3";

impl ParsedModel {
    pub fn load(rdr: &mut impl Read) -> Result<ParsedModel, ParseError> {
        use ParseError::*;

        fn advance(rdr: &mut impl Read, p: &mut usize, to: u32) -> Result<(), ParseError> {
            const MAX_SKIP: usize = 0x1000;
            let mut buf = [0; MAX_SKIP];

            let to = to as usize;
            if to < *p {
                Err(InvalidOffset(format!(
                    "Tried to seek from {0:#x} to {1:#x}",
                    *p, to
                )))
            } else if to == *p {
                Ok(())
            } else {
                let skip = to - *p;
                // Should only skip padding, not large blocks
                if skip > MAX_SKIP {
                    Err(InvalidOffset(format!(
                        "Tried to seek from {0:#x} to {1:#x} ({2:#x} bytes > {3:#x})",
                        *p, to, skip, MAX_SKIP
                    )))
                } else {
                    rdr.read_exact(&mut buf[..skip])?;
                    for (i, b) in buf[..skip].iter().enumerate() {
                        if *b != 0 {
                            return Err(InvalidPadding(*p + i, *b));
                        }
                    }
                    *p = to;
                    Ok(())
                }
            }
        }

        let mut rdr = SectionReader {
            rdr,
            p: 0,
            section: 0,
            offsets: Vec::new(),
        };

        let mut magic = [0; 4];
        rdr.read_exact(&mut magic)?;

        debug!("File magic: {0:?}", &magic);

        if magic != *FILE_MAGIC {
            return Err(InvalidMagic(magic));
        }

        let ver = rdr.read_u32()?;
        debug!("File version: {0:?}", ver);

        let ver = Version::from_repr(ver as usize).ok_or(UnknownVersion(ver))?;

        // At 0x40: Section offsets
        rdr.advance_to(0x40)?;

        let nsects = Self::num_sections(ver);
        let mut offsets = Vec::new();
        offsets.resize(nsects, 0);
        rdr.read_u32_into(&mut offsets)?;
        rdr.offsets = offsets;

        // First section: Object counts
        rdr.next_section();
        let mut counts: Vec<u32> = Vec::new();
        counts.resize(Self::num_classes(ver), 0);
        rdr.read_u32_into(&mut counts)?;

        // Second section: Canvas properties
        rdr.next_section();
        let canvas = Canvas {
            scale: rdr.read_f32()?,
            center_x: rdr.read_f32()?,
            center_y: rdr.read_f32()?,
            width: rdr.read_f32()?,
            height: rdr.read_f32()?,
        };
        debug!("Canvas properties: {0:?}", &canvas);

        let mut m = ParsedModel {
            canvas,
            version: Some(ver),
            ..Default::default()
        };

        m.load_counts(ver.pass(), &counts);
        for pass in Pass::VARIANTS {
            if *pass > ver.pass() {
                break;
            }
            debug!("Loading fields for {:?}...", pass);
            m.parse_objects(*pass, &mut rdr)?;
        }

        m.upgrade()?;
        m.find_refs()?;

        Ok(m)
    }

    fn default_colors(&mut self, count: u32) -> i32 {
        assert!(self.multiply_color.count == self.screen_color.count);

        let ret = self.multiply_color.count as i32;

        let new_count = self.multiply_color.count + count as usize;

        self.multiply_color.r.resize(new_count, 1.0);
        self.multiply_color.g.resize(new_count, 1.0);
        self.multiply_color.b.resize(new_count, 1.0);
        self.screen_color.r.resize(new_count, 0.0);
        self.screen_color.g.resize(new_count, 0.0);
        self.screen_color.b.resize(new_count, 0.0);

        ret
    }

    fn upgrade(&mut self) -> Result<(), ParseError> {
        let ver = self.version.unwrap();

        fn flat_vec<T: Clone>(count: usize, val: T) -> Vec<T> {
            let mut v = Vec::new();
            v.resize(count, val);
            v
        }

        // Internal stuff
        self.part.i_blend_form_maps = flat_vec(self.part.count, None.into());
        self.art_mesh.i_blend_form_maps = flat_vec(self.art_mesh.count, None.into());
        self.rot_deformer.i_blend_form_maps = flat_vec(self.rot_deformer.count, None.into());
        self.warp_deformer.i_blend_form_maps = flat_vec(self.warp_deformer.count, None.into());
        self.glue.i_blend_form_maps = flat_vec(self.glue.count, None.into());

        self.rot_deformer.i_deformer = flat_vec(self.rot_deformer.count, None.into());
        self.warp_deformer.i_deformer = flat_vec(self.warp_deformer.count, None.into());

        if ver < Version::V3_3 {
            debug!("Upgrading to V3.3");
            self.warp_deformer.bilinear_interpolation = flat_vec(self.warp_deformer.count, FALSE);
        }
        // V4_0: Added bit in render_config, no need to upgrade
        if ver < Version::V4_2 {
            debug!("Upgrading to V4.2");
            self.param.unk_zero_2 = flat_vec(self.param.count, Default::default());
            self.param.i_keypoints = flat_vec(self.param.count, IKeypoint(0));
            self.param.cnt_keypoints = flat_vec(self.param.count, 0);
            self.param.blendshape = flat_vec(self.param.count, FALSE);
            self.param.i_blend_maps = flat_vec(self.param.count, IBlendParamMap(0));
            self.param.cnt_blend_maps = flat_vec(self.param.count, 0);

            debug!("Upgrading colors from < V4.2 to V5.0");
            self.art_mesh_form.i_multiply_color =
                flat_vec(self.art_mesh_form.count, IMultiplyColor(0));
            self.art_mesh_form.i_screen_color = flat_vec(self.art_mesh_form.count, IScreenColor(0));
            self.warp_form.i_multiply_color = flat_vec(self.warp_form.count, IMultiplyColor(0));
            self.warp_form.i_screen_color = flat_vec(self.warp_form.count, IScreenColor(0));
            self.rot_form.i_multiply_color = flat_vec(self.rot_form.count, IMultiplyColor(0));
            self.rot_form.i_screen_color = flat_vec(self.rot_form.count, IScreenColor(0));
            self.multiply_color.count = 1;
            self.multiply_color.r = vec![1.0];
            self.multiply_color.g = vec![1.0];
            self.multiply_color.b = vec![1.0];
            self.screen_color.count = 1;
            self.screen_color.r = vec![0.0];
            self.screen_color.g = vec![0.0];
            self.screen_color.b = vec![0.0];
        } else if ver < Version::V5_0 {
            debug!("Upgrading colors from V4.2 to V5.0");
            self.art_mesh_form
                .i_multiply_color
                .resize(self.art_mesh_form.count, IMultiplyColor(0));
            self.art_mesh_form
                .i_screen_color
                .resize(self.art_mesh_form.count, IScreenColor(0));
            self.warp_form
                .i_multiply_color
                .resize(self.warp_form.count, IMultiplyColor(0));
            self.warp_form
                .i_screen_color
                .resize(self.warp_form.count, IScreenColor(0));
            self.rot_form
                .i_multiply_color
                .resize(self.rot_form.count, IMultiplyColor(0));
            self.rot_form
                .i_screen_color
                .resize(self.rot_form.count, IScreenColor(0));

            for i in 0..self.art_mesh.count {
                let v = ArtMeshView::get(self, IArtMesh::new(i as u32)).unwrap();
                let mut color = *v.f_i_color_forms();
                let r = v.range_forms();
                for j in r.start.0..r.end.0 {
                    self.art_mesh_form.i_multiply_color[j as usize] = IMultiplyColor(color);
                    self.art_mesh_form.i_screen_color[j as usize] = IScreenColor(color);
                    color += 1;
                }
            }
            for i in 0..self.rot_deformer.count {
                let v = RotDeformerView::get(self, IRotDeformer::new(i as u32)).unwrap();
                let mut color = *v.f_i_color_forms();
                let r = v.range_forms();
                for j in r.start.0..r.end.0 {
                    self.rot_form.i_multiply_color[j as usize] = IMultiplyColor(color);
                    self.rot_form.i_screen_color[j as usize] = IScreenColor(color);
                    color += 1;
                }
            }
            for i in 0..self.warp_deformer.count {
                let v = WarpDeformerView::get(self, IWarpDeformer::new(i as u32)).unwrap();
                let mut color = *v.f_i_color_forms();
                let r = v.range_forms();
                for j in r.start.0..r.end.0 {
                    self.warp_form.i_multiply_color[j as usize] = IMultiplyColor(color);
                    self.warp_form.i_screen_color[j as usize] = IScreenColor(color);
                    color += 1;
                }
            }
        }

        Ok(())
    }

    fn find_refs(&mut self) -> Result<(), ParseError> {
        for i in 0..self.deformer.count {
            let v = DeformerView::get(self, IDeformer::new(i as u32)).unwrap();
            let idx = v.idx();
            match v.typed() {
                TypedDeformerView::Warp(t) => {
                    let tidx = t.idx as usize;
                    self.warp_deformer.i_deformer[tidx] = Some(idx).into();
                }
                TypedDeformerView::Rotation(t) => {
                    let tidx = t.idx as usize;
                    self.rot_deformer.i_deformer[tidx] = Some(idx).into();
                }
            }
        }

        macro_rules! blend_form_map {
            ($obj:ident, $field:ident) => {
                paste! {
                    for i in 0..self.[<$obj:snake _blend_form_maps>].count {
                        let bfm =
                            [<$obj BlendFormMapsView>]::get(self, [<I $obj BlendFormMaps>]::new(i as u32)).unwrap();
                        let j = bfm.[<i_ $obj:lower>]().0 as usize;
                        let idx = bfm.idx();
                        let item = &mut self.$field.i_blend_form_maps[j];
                        if let Some(p) = item.get() {
                            return Err(DuplicateReference(format!(
                                "{0} referenced by two blend form maps: {1:?} and {2:?}",
                                stringify!($obj),
                                p,
                                idx
                            )));
                        }
                        *item = Some(idx).into();
                    }
                }
            };
        }

        blend_form_map!(ArtMesh, art_mesh);
        blend_form_map!(Rot, rot_deformer);
        blend_form_map!(Warp, warp_deformer);
        blend_form_map!(Part, part);
        blend_form_map!(Glue, glue);

        // TODO: There's probably a better way...
        if let Some(dg) = self
            .draw_groups()
            .into_iter()
            .max_by_key(|g| *g.f_total_artmesh_count())
        {
            self.root_draw_group = Some(dg.idx())
        }

        Ok(())
    }

    pub fn version(&self) -> Option<Version> {
        self.version
    }
}
