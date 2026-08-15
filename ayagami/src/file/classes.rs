use super::parse::{Parsable, ParseError, ReadArray, SectionReader};
use super::types::*;
use super::{Pass, Version};
use crate::{core, pose};
use log::debug;
use paste::paste;
use std::ops::Range;
use std::sync::Arc;
use strum_macros::FromRepr;

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
        visible_artmeshes: Bool32 => bool,
        visible_deformers: Bool32 => bool,
        parent: Option<&&Part>,
    },
    V5_3A {
        offscreen_part: Option<&&OffscreenPart>,
    },
    Internal {
        blend_form_maps: Option<&&PartBlendFormMaps>
    }
});
impl_validator!(Part, |&self| {
    check!(*self.f_hdr() == U32Pair(0, 0));
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
        unk_flag1: Bool32 => bool,
        visible: Bool32 => bool,
        part: Option<&&Part>,
        parent: Option<&&Deformer>,
        deformer_type: u32 => DeformerType,
        i_typed: u32
    }
});
impl_validator!(Deformer, |&self| {
    check!(*self.f_hdr() == U32Pair(0, 0));
    match self.f_deformer_type() {
        DeformerType::Warp => check!(*self.f_i_typed() as usize <= self.model.warp_deformer.count),
        DeformerType::Rotation => {
            check!(*self.f_i_typed() as usize <= self.model.rot_deformer.count)
        }
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
        bilinear_interpolation: Bool32 => bool,
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
impl_validator!(WarpDeformer);

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
impl_validator!(RotDeformer);

#[derive(Copy, Clone, Debug, Ord, PartialOrd, PartialEq, Eq, Hash, FromRepr)]
#[repr(u8)]
pub enum BlendMode {
    Normal = 0,
    Add = 1,
    Multiply = 2,
}

pub const RENDER_INVERT_MASK: u8 = 0x8;
pub const RENDER_DOUBLE_SIDED: u8 = 0x4;

#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, PartialEq, Eq, Hash, FromRepr)]
#[repr(u8)]
pub enum ColorBlendMode {
    #[default]
    Normal = 0,
    PremultAdd = 1,
    PremultMultiply = 2,

    Add = 3,
    AddGlow = 4,
    Darken = 5,
    Multiply = 6,
    ColorBurn = 7,
    LinearBurn = 8,
    Lighten = 9,
    Screen = 10,
    ColorDodge = 11,
    Overlay = 12,
    SoftLight = 13,
    HardLight = 14,
    LinearLight = 15,
    Hue = 16,
    Color = 17,
}
enum_conversion!(ColorBlendMode, u8);

#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, PartialEq, Eq, Hash, FromRepr)]
#[repr(u8)]
pub enum AlphaBlendMode {
    #[default]
    Over = 0,
    Atop = 1,
    Out = 2,
    Conjoint = 3,
    Disjoint = 4,
}
enum_conversion!(AlphaBlendMode, u8);

#[derive(Copy, Clone, Default, Debug, Ord, PartialOrd, PartialEq, Eq, Hash)]
#[repr(C, align(4))]
pub struct BlendConfig {
    pub color: ColorBlendMode,
    pub alpha: AlphaBlendMode,
    pub(crate) pad: u16,
}

impl BlendConfig {
    pub fn is_advanced(&self) -> bool {
        match self.color {
            ColorBlendMode::Normal => self.alpha != AlphaBlendMode::Over,
            ColorBlendMode::PremultAdd => false,
            ColorBlendMode::PremultMultiply => false,
            _ => true,
        }
    }
    pub fn simple(&self) -> Option<BlendMode> {
        if self.is_advanced() {
            None
        } else {
            Some(BlendMode::from_repr(self.color as u8).unwrap())
        }
    }
}

impl TryFrom<u32> for BlendConfig {
    type Error = ParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if (value >> 16) != 0 {
            return Err(ParseError::InvalidValue(format!(
                "BlendConfig = {:#x}",
                value
            )));
        }
        let mut ret = Self {
            color: ((value & 0xff) as u8).try_into()?,
            alpha: (((value >> 8) & 0xff) as u8).try_into()?,
            pad: 0,
        };
        if !ret.is_advanced() {
            // Per docs alpha blend is ignored, so normalize it to Over
            ret.alpha = AlphaBlendMode::Over;
        }
        Ok(ret)
    }
}

declare_object!(ArtMesh {
    Base {
        hdr: U32Pair,
        unk_a: U32Pair,
        unk_b: U32Pair,
        unk_c: U32Pair,
        id: Identifier => String,
        param_map_set: &&ParamMapSet,
        forms: &&[ArtMeshForm],
        unk_flag1: Bool32 => bool,
        visible: Bool32 => bool,
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
    V5_3A {
        blend_config: u32 => BlendConfig,
    },
    Internal {
        blend_form_maps: Option<&&ArtMeshBlendFormMaps>
    }
});
impl_validator!(ArtMesh, |&self| {
    check!(*self.f_hdr() == U32Pair(0, 0));
    check!((self.f_render_config() >> 4) == 0);
    require!(BlendMode::from_repr(self.f_render_config() & 3).is_some());
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
        repeat: Bool32 => bool,
        snap_type: u32 => ParamSnapType,
        maps: &&[ParamMap],
    },
    V4_2A {
        unk_zero_2: U32Pair,
        keypoints: &[Keypoint],
    },
    V4_2 {
        blendshape: Bool32 => bool,
        blend_maps: &&[BlendParamMap],
    }
});
impl_validator!(Param, |&self| {
    check!(*self.f_hdr() == U32Pair(0, 0));
});

declare_object!(PartForm {
    Base {
        depth: f32,
    },
    V5_3 {
        offscreen: Option<&&OffscreenPartForm>,
    }
});
declare_parent!(PartForm, Part);
impl_validator!(PartForm);

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
impl_validator!(WarpForm);

declare_object!(RotForm {
    Base {
        opacity: f32,
        angle: f32,
        pos_x: f32,
        pos_y: f32,
        scale: f32,
        flip_x: Bool32 => bool,
        flip_y: Bool32 => bool,
    },
    V5_0 {
        multiply_color: &&MultiplyColor,
        screen_color: &&ScreenColor,
    }
});
declare_parent!(RotForm, RotDeformer);
impl_validator!(RotForm);

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
impl_validator!(ArtMeshForm);

declare_primitive!(VertexCoord(f32 => core::Coord), Base);

declare_object!(ParamMapRef {
    Base {
        map: &&ParamMap
    }
});
impl_validator!(ParamMapRef);

declare_object!(ParamMapSet {
    Base {
        refs: &&[ParamMapRef]
    }
});
impl_validator!(ParamMapSet);

declare_object!(ParamMap {
    Base {
        keypoints: &[Keypoint]
    }
});
impl_validator!(ParamMap);

declare_primitive!(Keypoint(f32), Base);

declare_primitive!(TexCoord(f32 => core::Coord), Base);

declare_primitive!(VertexIndex(u16), Base);

declare_object!(ArtMeshRef {
    Base {
        artmesh: Option<&&ArtMesh>
    }
});
impl_validator!(ArtMeshRef);

declare_object!(DrawGroup {
    Base {
        items: &&[DrawItem],
        total_artmesh_count: u32,
        max_depth: f32,
        min_depth: f32,
    }
});
impl_validator!(DrawGroup);

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
impl_validator!(DrawItem, |&self| {
    match self.f_item_type() {
        DrawItemType::ArtMesh => {
            check!((*self.f_i_child() as usize) < self.model.art_mesh.count);
            check!(self.i_draw_group().get().is_none());
        }
        DrawItemType::Part => {
            check!((*self.f_i_child() as usize) < self.model.part.count);
            check!(self.i_draw_group().get().is_some());
        }
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
impl_validator!(Glue, |&self| {
    check!(*self.f_hdr() == U32Pair(0, 0));
    check!(self.cnt_coords().is_multiple_of(2));
});

declare_object!(GlueForm {
    V3_0 {
        compatibility: f32
    }
});
declare_parent!(GlueForm, Glue);
impl_validator!(GlueForm);

declare_object!(GlueCoord {
    V3_0 {
        weight: f32,
        vertex_index: u16,
    }
});
impl_validator!(GlueCoord);

declare_object!(MultiplyColor {
    V4_2B {
        r: f32,
        g: f32,
        b: f32,
    }
});
impl_validator!(MultiplyColor);

declare_object!(ScreenColor {
    V4_2B {
        r: f32,
        g: f32,
        b: f32,
    }
});
impl_validator!(ScreenColor, |&self| {
    check!(*self.f_r() >= 0. && *self.f_r() <= 1.);
    check!(*self.f_g() >= 0. && *self.f_g() <= 1.);
    check!(*self.f_b() >= 0. && *self.f_b() <= 1.);
});

declare_object!(BlendParamMap {
    V4_2 {
        keypoints: &[Keypoint],
        neutral_index: u32,
    }
});
impl_validator!(BlendParamMap);

declare_object!(BlendFormMap {
    V4_2 {
        param_map: &&BlendParamMap,
        // Variant
        i_forms: u32,
        cnt_forms: u32,
        blendweight_limits: &&[BlendWeightLimitRef]
    }
});
impl_validator!(BlendFormMap);

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
impl_validator!(WarpBlendFormMaps);

declare_object!(ArtMeshBlendFormMaps {
    V4_2 {
        artmesh: &&ArtMesh,
        maps: &&[BlendFormMap]
    }
});
impl_validator!(ArtMeshBlendFormMaps);

declare_object!(BlendWeightLimitRef {
    V4_2 {
        limit: &&BlendWeightLimit
    }
});
impl_validator!(BlendWeightLimitRef);

declare_object!(BlendWeightLimit {
    V4_2 {
        param: &&Param,
        points: &&[BlendWeightLimitPoint],
    }
});
impl_validator!(BlendWeightLimit);

declare_object!(BlendWeightLimitPoint {
    V4_2 {
        value: f32,
        weight: f32,
    }
});
impl_validator!(BlendWeightLimitPoint);

declare_object!(PartBlendFormMaps {
    V5_0 {
        part: &&Part,
        maps: &&[BlendFormMap]
    }
});
impl_validator!(PartBlendFormMaps);

declare_object!(RotBlendFormMaps {
    V5_0 {
        rot: &&RotDeformer,
        maps: &&[BlendFormMap]
    }
});
impl_validator!(RotBlendFormMaps);

declare_object!(GlueBlendFormMaps {
    V5_0 {
        glue: &&Glue,
        maps: &&[BlendFormMap]
    }
});
impl_validator!(GlueBlendFormMaps);

declare_object!(OffscreenPart {
    V5_3A {
        unk_zeros: u32,
        part: &&Part,
        render_config: u8,
        blend_config: u32 => BlendConfig,
        clips: &&[ArtMeshRef],
    },
    Internal {
        blend_form_maps: Option<&&OffscreenPartBlendFormMaps>
    }
});
impl_validator!(OffscreenPart, |&self| {
    require!(self.part_view().i_offscreen_part().0 == self.idx as i32);
    check!(*self.f_unk_zeros() == 0);
    check!((self.f_render_config() >> 4) == 0);
    require!(BlendMode::from_repr(self.f_render_config() & 3).is_some());
    if self.f_blend_config().color as u8 <= ColorBlendMode::PremultMultiply as u8 {
        check!(self.f_render_config() & 3 == self.f_blend_config().color as u8);
    } else {
        check!(self.f_render_config() & 3 == BlendMode::Normal as u8);
    }
});

declare_object!(OffscreenPartForm {
    V5_3 {
        opacity: f32,
        multiply_color: &&MultiplyColor,
        screen_color: &&ScreenColor,
    }
});
declare_parent!(OffscreenPartForm, PartForm);
impl_validator!(OffscreenPartForm);

declare_object!(OffscreenPartBlendFormMaps {
    V5_3 {
        offscreen_part: &&OffscreenPart,
        maps: &&[BlendFormMap]
    }
});
impl_validator!(OffscreenPartBlendFormMaps, |&self| {}, |&self| {
    let offscreen_part = self.offscreen_part_view();
    let part = offscreen_part.part_view();
    require!(part.blend_form_maps_view().is_some());
    let part_blend_form_maps = part.blend_form_maps_view().unwrap();
    check!(self.cnt_maps() == part_blend_form_maps.cnt_maps());
    for (map, partmap) in self
        .maps_views()
        .into_iter()
        .zip(part_blend_form_maps.maps_views())
    {
        check!(map.i_param_map() == partmap.i_param_map());
        check!(map.f_cnt_forms() == partmap.f_cnt_forms());
        check!(map.range_blendweight_limits() == partmap.range_blendweight_limits());
        let part_forms = partmap.forms_views::<PartForm>();
        for (form, partform) in map
            .forms_views::<OffscreenPartForm>()
            .into_iter()
            .zip(part_forms)
        {
            check!(partform.i_offscreen().get() == Some(form.idx));
        }
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
        pub(crate) pose_map: Option<Arc<pose::PoseMap>>,
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
    },
    V5_3A {
        OffscreenPart,
    },
    V5_3 {
        OffscreenPartForm,
        OffscreenPartBlendFormMaps,
    }
});

const_assert_eq!(ParsedModel::num_classes(V3_0), 23);
const_assert_eq!(ParsedModel::num_classes(V3_3), 23);
const_assert_eq!(ParsedModel::num_classes(V4_0), 23);
const_assert_eq!(ParsedModel::num_classes(V4_2), 32);
const_assert_eq!(ParsedModel::num_classes(V5_0), 35);
const_assert_eq!(ParsedModel::num_classes(V5_3), 38);

const_assert_eq!(ParsedModel::num_sections(V3_0), 101);
const_assert_eq!(ParsedModel::num_sections(V3_3), 102);
const_assert_eq!(ParsedModel::num_sections(V4_0), 102);
const_assert_eq!(ParsedModel::num_sections(V4_2), 137);
const_assert_eq!(ParsedModel::num_sections(V5_0), 152);
const_assert_eq!(ParsedModel::num_sections(V5_3), 167);
