#![allow(unused)]

use super::parse::{Parsable, ParseError, ReadArray, SectionReader};
use super::types::*;
use super::types::*;
use super::{Pass, Version};
use crate::core;
use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use log::{debug, info, warn};
use paste::paste;
use std::marker::PhantomData;
use std::{
    io::Read,
    ops::{Deref, Range},
};
use strum::VariantArray;
use strum_macros::FromRepr;
use thiserror::Error;
use zerocopy_derive::FromBytes;

use Version::*;

// Types implicitly used by macros
type ArrayType<T> = Vec<T>;
type Model = super::classes::ParsedModel;

declare_object!(Part {
    Base {
        hdr: U32Pair,
        id: Identifier => String,
        param_map_set: &&ParamMapSet,
        forms: &&[PartForm],
        visible_artmeshes: Bool32,
        visible_deformers: Bool32,
        parent: Option<&&Part>,
    },
    Internal {
        blend_form_maps: Option<&&PartBlendFormMaps>
    }
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromRepr)]
#[repr(u32)]
pub enum DeformerType {
    Warp = 0,
    Rotation = 1,
}

enum_conversion!(DeformerType, u32);

declare_object!(Deformer {
    Base {
        hdr: U32Pair,
        id: Identifier => String,
        param_map_set: &&ParamMapSet,
        unk_flag1: Bool32,
        visible: Bool32,
        part: Option<&&Part>,
        parent: Option<&&Deformer>,
        deformer_type: u32 => DeformerType,
        i_typed: u32
    }
});

pub enum TypedDeformerView<'a> {
    Warp(WarpDeformerView<'a>),
    Rotation(RotDeformerView<'a>),
}

impl<'a> DeformerView<'a> {
    pub fn typed(&self) -> TypedDeformerView<'a> {
        let i = *self.f_i_typed();
        match self.f_deformer_type() {
            DeformerType::Warp => TypedDeformerView::Warp(
                WarpDeformerView::get(self.model, IWarpDeformer(i)).unwrap(),
            ),
            DeformerType::Rotation => TypedDeformerView::Rotation(
                RotDeformerView::get(self.model, IRotDeformer(i)).unwrap(),
            ),
        }
    }
}

declare_object!(WarpDeformer {
    Base {
        param_map_set: &&ParamMapSet,
        forms: &&[WarpForm],
        vertex_count: u32,
        y_divs: u32,
        x_divs: u32,
    },
    V3_3 {
        bilinear_interpolation: Bool32,
    },
    V4_2B {
        // Implicit pointer to first multiply & screen color
        i_color_forms: u32,
    },
    Internal {
        deformer: Option<&&Deformer>,
        blend_form_maps: Option<&&WarpBlendFormMaps>,
    }
});
declare_parent!(WarpDeformer, Deformer);

declare_object!(RotDeformer {
    Base {
        param_map_set: &&ParamMapSet,
        forms: &&[RotForm],
        angle_offset: f32,
    },
    V4_2B {
        // Implicit pointer to first multiply & screen color
        i_color_forms: u32,
    },
    Internal {
        deformer: Option<&&Deformer>,
        blend_form_maps: Option<&&RotBlendFormMaps>,
    }
});
declare_parent!(RotDeformer, Deformer);

#[derive(Copy, Clone, Debug, Ord, PartialOrd, PartialEq, Eq, Hash, FromRepr)]
pub enum BlendMode {
    Normal = 0,
    Add = 1,
    Multiply = 2,
}

pub const RENDER_INVERT_MASK: u8 = 0x8;
pub const RENDER_DOUBLE_SIDED: u8 = 0x4;

declare_object!(ArtMesh {
    Base {
        hdr: U32Pair,
        unk_a: U32Pair,
        unk_b: U32Pair,
        unk_c: U32Pair,
        id: Identifier => String,
        param_map_set: &&ParamMapSet,
        forms: &&[ArtMeshForm],
        unk_flag1: Bool32,
        visible: Bool32,
        part: Option<&&Part>,
        deformer: Option<&&Deformer>,
        texture: u32,
        render_config: u8,
        vertex_count: u32,
        texcoord_start: &TexCoord,
        indices: &[VertexIndex],
        clips: &&[ArtMeshRef],
    },
    V4_2B {
        // Implicit pointer to first multiply & screen color
        i_color_forms: u32,
    },
    Internal {
        blend_form_maps: Option<&&ArtMeshBlendFormMaps>
    }
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromRepr)]
#[repr(u32)]
pub enum ParamSnapType {
    IntegerFloor = 0,
    IntegerSnap = 1,
    Normal = 3,
}

enum_conversion!(ParamSnapType, u32);

declare_object!(Param {
    Base {
        hdr: U32Pair,
        id: Identifier => String,
        max: f32,
        min: f32,
        default: f32,
        repeat: Bool32,
        snap_type: u32 => ParamSnapType,
        maps: &&[ParamMap],
    },
    V4_2A {
        unk_zero_2: U32Pair,
        keypoints: &[Keypoint],
    },
    V4_2 {
        blendshape: Bool32,
        blend_maps: &&[BlendParamMap],
    }
});

declare_object!(PartForm {
    Base {
        depth: f32
    }
});
declare_parent!(PartForm, Part);

declare_object!(WarpForm {
    Base {
        opacity: f32,
        start_vertex: &VertexCoord,
    },
    V5_0 {
        multiply_color: &&MultiplyColor,
        screen_color: &&ScreenColor,
    }
});
declare_parent!(WarpForm, WarpDeformer);

declare_object!(RotForm {
    Base {
        opacity: f32,
        angle: f32,
        pos_x: f32,
        pos_y: f32,
        scale: f32,
        flip_x: Bool32,
        flip_y: Bool32,
    },
    V5_0 {
        multiply_color: &&MultiplyColor,
        screen_color: &&ScreenColor,
    }
});
declare_parent!(RotForm, RotDeformer);

declare_object!(ArtMeshForm {
    Base {
        opacity: f32,
        depth: f32,
        start_vertex: &VertexCoord,
    },
    V5_0 {
        multiply_color: &&MultiplyColor,
        screen_color: &&ScreenColor,
    }
});
declare_parent!(ArtMeshForm, ArtMesh);

declare_primitive!(VertexCoord(f32 => core::Coord), Base);

declare_object!(ParamMapRef {
    Base {
        map: &&ParamMap
    }
});

declare_object!(ParamMapSet {
    Base {
        refs: &&[ParamMapRef]
    }
});

declare_object!(ParamMap {
    Base {
        keypoints: &[Keypoint]
    }
});

declare_primitive!(Keypoint(f32), Base);

declare_primitive!(TexCoord(f32 => core::Coord), Base);

declare_primitive!(VertexIndex(u16), Base);

declare_object!(ArtMeshRef {
    Base {
        artmesh: Option<&&ArtMesh>
    }
});

declare_object!(DrawGroup {
    Base {
        items: &&[DrawItem],
        total_artmesh_count: u32,
        max_depth: f32,
        min_depth: f32,
    }
});

#[derive(Debug, Copy, Clone, PartialEq, Eq, FromRepr)]
#[repr(u32)]
pub enum DrawItemType {
    ArtMesh = 0,
    Part = 1,
}

enum_conversion!(DrawItemType, u32);

declare_object!(DrawItem {
    Base {
        item_type: u32 => DrawItemType,
        i_child: u32,
        draw_group: Option<&&DrawGroup>,
    }
});

declare_object!(Glue {
    V3_0 {
        hdr: U32Pair,
        id: Identifier => String,
        param_map_set: &&ParamMapSet,
        forms: &&[GlueForm],
        artmesh_1: &&ArtMesh,
        artmesh_2: &&ArtMesh,
        coords: &&[GlueCoord],
    },
    Internal {
        blend_form_maps: Option<&&GlueBlendFormMaps>
    }
});

declare_object!(GlueForm {
    V3_0 {
        compatibility: f32
    }
});
declare_parent!(GlueForm, Glue);

declare_object!(GlueCoord {
    V3_0 {
        weight: f32,
        vertex_index: u16,
    }
});

declare_object!(MultiplyColor {
    V4_2B {
        r: f32,
        g: f32,
        b: f32,
    }
});

declare_object!(ScreenColor {
    V4_2B {
        r: f32,
        g: f32,
        b: f32,
    }
});

declare_object!(BlendParamMap {
    V4_2 {
        keypoints: &[Keypoint],
        neutral_index: u32,
    }
});

declare_object!(BlendFormMap {
    V4_2 {
        param_map: &&BlendParamMap,
        // Variant
        i_forms: u32,
        cnt_forms: u32,
        blendweight_limits: &&[BlendWeightLimitRef]
    }
});

// Generics
impl<'model> BlendFormMapView<'model> {
    pub(crate) fn range_forms<T: Object>(&self) -> Range<T::Idx> {
        let i = T::Idx::new(self.fields().i_forms[self.idx as usize]);
        let cnt = self.f_cnt_forms();
        i..(i.offset(*cnt))
    }
    pub(crate) fn forms_views<T: Object + 'model>(
        &self,
    ) -> ItemCollection<'model, T::View<'model>> {
        let range = self.range_forms::<T>();
        let mut c = ItemCollection::new(self.model, range.start.get(), range.end.get());
        c.parent = Some(self.idx);
        c
    }
}

declare_object!(WarpBlendFormMaps {
    V4_2 {
        warp: &&WarpDeformer,
        maps: &&[BlendFormMap]
    }
});

declare_object!(ArtMeshBlendFormMaps {
    V4_2 {
        artmesh: &&ArtMesh,
        maps: &&[BlendFormMap]
    }
});

declare_object!(BlendWeightLimitRef {
    V4_2 {
        limit: &&BlendWeightLimit
    }
});

declare_object!(BlendWeightLimit {
    V4_2 {
        param: &&Param,
        points: &&[BlendWeightLimitPoint],
    }
});

declare_object!(BlendWeightLimitPoint {
    V4_2 {
        value: f32,
        weight: f32,
    }
});

declare_object!(PartBlendFormMaps {
    V5_0 {
        part: &&Part,
        maps: &&[BlendFormMap]
    }
});

declare_object!(RotBlendFormMaps {
    V5_0 {
        rot: &&RotDeformer,
        maps: &&[BlendFormMap]
    }
});

declare_object!(GlueBlendFormMaps {
    V5_0 {
        glue: &&Glue,
        maps: &&[BlendFormMap]
    }
});

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Canvas {
    pub scale: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
}

declare_file_objects!(ParsedModel {
    Global {
        pub(crate) canvas: Canvas,
        pub(crate) version: Option<Version>,
        pub(crate) root_draw_group: Option<IDrawGroup>,
    },
    Base {
        Part,
        Deformer,
        WarpDeformer,
        RotDeformer,
        ArtMesh,
        Param,
        PartForm,
        WarpForm,
        RotForm,
        ArtMeshForm,
        VertexCoord,
        ParamMapRef,
        ParamMapSet,
        ParamMap,
        Keypoint,
        TexCoord,
        VertexIndex,
        ArtMeshRef,
        DrawGroup,
        DrawItem,
    },
    V3_0 {
        Glue,
        GlueCoord,
        GlueForm,
    },
    V4_2B {
        MultiplyColor,
        ScreenColor,
    },
    V4_2 {
        BlendParamMap,
        BlendFormMap,
        WarpBlendFormMaps,
        ArtMeshBlendFormMaps,
        BlendWeightLimitRef,
        BlendWeightLimit,
        BlendWeightLimitPoint,
    },
    V5_0 {
        PartBlendFormMaps,
        RotBlendFormMaps,
        GlueBlendFormMaps,
    }
});

const_assert_eq!(ParsedModel::num_classes(V3_0), 23);
const_assert_eq!(ParsedModel::num_classes(V3_3), 23);
const_assert_eq!(ParsedModel::num_classes(V4_0), 23);
const_assert_eq!(ParsedModel::num_classes(V4_2), 32);
const_assert_eq!(ParsedModel::num_classes(V5_0), 35);

const_assert_eq!(ParsedModel::num_sections(V3_0), 101);
const_assert_eq!(ParsedModel::num_sections(V3_3), 102);
const_assert_eq!(ParsedModel::num_sections(V4_0), 102);
const_assert_eq!(ParsedModel::num_sections(V4_2), 137);
const_assert_eq!(ParsedModel::num_sections(V5_0), 152);
