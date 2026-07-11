#![allow(unused)]
use std::{
    marker::PhantomData,
    ops::{Deref, Index, Range},
};

use super::classes::*;
use crate::{
    core::{self, ArtMesh, ItemArray, RotForm},
    file::types::*,
};
use core::Collection;
use glam::{
    f32::vec2,
    u32::{UVec2, uvec2},
};
use paste::paste;
use private::*;

///////////// Macros

macro_rules! by_value {
    ( $field:ident, $type:ty, $f:ident ) => {
        fn $field(&self) -> $type {
            (*self.$f()).into()
        }
    };
    ( $field:ident, $type:ty ) => {
        fn $field(&self) -> $type {
            paste! {(*self.[< f_ $field >]()).into()}
        }
    };
}

macro_rules! by_ref {
    ( $field:ident, $type:ty ) => {
        fn $field(&self) -> $type {
            paste! {&(*self.[< f_ $field >]())}
        }
    };
}

macro_rules! item_ref {
    ( $field:ident, Option<$type:ty>, $f:ident ) => {
        fn $field(&self) -> Option<impl Deref<Target = $type>> {
            Some(self.$f()?.into_ref())
        }
    };
    ( $field:ident, $type:ty, $f:ident ) => {
        fn $field(&self) -> impl Deref<Target = $type> {
            self.$f().into_ref()
        }
    };
}

macro_rules! special {
    ( param_maps ) => {
        fn param_maps(
            &self,
        ) -> impl IntoIterator<Item = impl Deref<Target = ParamMapView<'model>>> {
            self.param_map_set_view()
                .refs_views()
                .into_iter()
                .map(|i| i.map_view().into_ref())
        }
    };
    ( blend_form_maps, $t:ident ) => {
        fn blend_form_maps(
            &self,
        ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>> {
            let bfm = self.blend_form_maps_view()?;
            Some(
                bfm.maps_views()
                    .into_iter()
                    .map(|v| ViewRef::new($t(v.0, self.idx))),
            )
        }
    };
    ( form ) => {
        fn form(&self, indices: &[u32]) -> Option<impl Deref<Target = Self::Form>> {
            let pms = self.param_map_set_view();
            let mut index = 0;
            let mut step = 1;
            assert!(indices.len() == pms.cnt_refs() as usize);
            for (i, pmr) in pms.refs_views().into_iter().enumerate() {
                let cnt = pmr.map_view().cnt_keypoints();
                if indices[i] > cnt {
                    return None;
                }
                index += step * indices[i];
                step *= cnt;
            }
            self.forms().index(index as usize)
        }
    };
}

macro_rules! child_collection {
    ( $field:ident, $type:ty, $f:ident ) => {
        fn $field(&self) -> impl ItemArray<'model, $type> {
            let x = self.$f();
            x
        }
    };
    ( $field:ident, $type:ty, $($f:tt)* ) => {
        fn $field(&self) -> impl ItemArray<'model, $type> {
            let x = self.$($f)*;
            x
        }
    };
}

macro_rules! declare_item {
    ( $name:ident ) => {
        impl<'model> core::Item<'model> for $name<'model> {
            type Model = ParsedModel;
            type Ref<'a>
                = ViewRef<'model, $name<'model>>
            where
                'a: 'model;

            fn uid(&self) -> u32 {
                self.idx as u32
            }
        }
    };
}

///////////// Helpers

impl<'a> MultiplyColorView<'a> {
    fn color(&self) -> core::Color {
        [*self.f_r(), *self.f_g(), *self.f_b()]
    }
}

impl<'a> ScreenColorView<'a> {
    fn color(&self) -> core::Color {
        [*self.f_r(), *self.f_g(), *self.f_b()]
    }
}

///////////// Classes

declare_item!(PartView);
impl<'model> core::Part<'model> for PartView<'model> {
    type Form = PartFormView<'model>;
    type BlendFormMap = PartBlendFormMapView<'model>;

    by_ref!(id, &str);
    special!(param_maps);
    special!(form);
    child_collection!(forms, Self::Form, forms_views);
    by_value!(visible_artmeshes, bool);
    by_value!(visible_deformers, bool);
    special!(blend_form_maps, PartBlendFormMapView);

    item_ref!(parent, Option<PartView<'model>>, parent_view);
}

declare_item!(DeformerView);
impl<'model> core::Deformer<'model> for DeformerView<'model> {
    type Warp = WarpDeformerView<'model>;
    type Rotation = RotDeformerView<'model>;
    type WarpRef<'a>
        = ViewRef<'model, WarpDeformerView<'model>>
    where
        'a: 'model;
    type RotationRef<'a>
        = ViewRef<'model, RotDeformerView<'model>>
    where
        'a: 'model;

    by_ref!(id, &str);
    special!(param_maps);
    by_value!(visible, bool);
    item_ref!(part, Option<PartView<'model>>, part_view);
    item_ref!(parent, Option<DeformerView<'model>>, parent_view);

    fn typed(&self) -> core::TypedDeformer<'model, Self> {
        let i = *self.f_i_typed();
        match self.f_deformer_type() {
            DeformerType::Warp => core::TypedDeformer::Warp(
                WarpDeformerView::get(self.model, IWarpDeformer(i))
                    .unwrap()
                    .with_parent(self)
                    .into_ref(),
            ),
            DeformerType::Rotation => core::TypedDeformer::Rotation(
                RotDeformerView::get(self.model, IRotDeformer(i))
                    .unwrap()
                    .with_parent(self)
                    .into_ref(),
            ),
        }
    }
}

impl<'model> core::WarpDeformer<'model> for WarpDeformerView<'model> {
    type Model = ParsedModel;
    type Form = WarpFormView<'model>;
    type BlendFormMap = WarpBlendFormMapView<'model>;

    special!(param_maps);
    special!(form);
    child_collection!(forms, Self::Form, forms_views);
    special!(blend_form_maps, WarpBlendFormMapView);

    fn size(&self) -> UVec2 {
        uvec2(*self.f_x_divs(), *self.f_y_divs())
    }

    by_value!(bilinear_interpolation, bool);
}

impl<'model> core::RotDeformer<'model> for RotDeformerView<'model> {
    type Model = ParsedModel;
    type Form = RotFormView<'model>;
    type BlendFormMap = RotBlendFormMapView<'model>;

    special!(param_maps);
    special!(form);
    child_collection!(forms, Self::Form, forms_views);
    special!(blend_form_maps, RotBlendFormMapView);
}

declare_item!(ArtMeshView);
impl<'model> core::ArtMesh<'model> for ArtMeshView<'model> {
    type Form = ArtMeshFormView<'model>;
    type BlendFormMap = ArtMeshBlendFormMapView<'model>;

    by_ref!(id, &str);
    by_value!(visible, bool);
    by_value!(texture, u32);
    by_value!(vertex_count, u32);

    item_ref!(part, Option<PartView<'model>>, part_view);
    item_ref!(deformer, Option<DeformerView<'model>>, deformer_view);

    fn clips(&self) -> impl IntoIterator<Item = impl Deref<Target = ArtMeshView<'model>>> {
        let cl = self.clips_views();
        cl.into_iter()
            .map(|i| i.artmesh_view().map(|v| v.into_ref()))
            .flatten()
            .collect::<Vec<_>>()
    }

    special!(param_maps);
    special!(form);
    child_collection!(forms, Self::Form, forms_views);
    special!(blend_form_maps, ArtMeshBlendFormMapView);

    fn blend_mode(&self) -> core::BlendMode {
        core::BlendMode::from_repr(*self.f_render_config() as usize & 3).unwrap()
    }
    fn culling(&self) -> bool {
        *self.f_render_config() & RENDER_DOUBLE_SIDED == 0
    }
    fn invert_mask(&self) -> bool {
        *self.f_render_config() & RENDER_INVERT_MASK != 0
    }
    fn texcoord_offset(&self) -> u32 {
        self.i_texcoord_start().0 / 2
    }
    fn index_range(&self) -> Range<u32> {
        let r = self.range_indices();
        r.start.get()..r.end.get()
    }
}

declare_item!(ParamView);
impl<'model> core::Param<'model> for ParamView<'model> {
    by_ref!(id, &str);
    by_value!(min, f32);
    by_value!(max, f32);
    by_value!(default, f32);
    by_value!(repeat, bool);
    by_value!(snap_type, ParamSnapType);
    by_value!(is_blendshape, bool, f_blendshape);

    child_collection!(param_maps, ParamMapView<'model>, maps_views);
    child_collection!(
        blend_param_maps,
        BlendParamMapView<'model>,
        blend_maps_views
    );

    fn keypoints(&self) -> Option<&[f32]> {
        let kp = self.keypoints_slice();
        if kp.is_empty() { None } else { Some(kp) }
    }
}

declare_item!(ArtMeshFormView);
impl<'model> core::ArtMeshForm<'model> for ArtMeshFormView<'model> {
    fn vertices(&self) -> &'model [core::Coord] {
        let start = self.i_start_vertex();
        let count = *self.parent().f_vertex_count();
        self.model.vertex_coord.slice(start..start.offset(count))
    }

    by_value!(opacity, f32);
    by_value!(depth, f32);

    fn multiply_color(&self) -> core::Color {
        self.multiply_color_view().color()
    }

    fn screen_color(&self) -> core::Color {
        self.screen_color_view().color()
    }
}

declare_item!(ParamMapView);
impl<'model> core::ParamMap<'model> for ParamMapView<'model> {
    fn keypoints(&self) -> &'model [f32] {
        self.keypoints_slice()
    }
}

declare_item!(BlendParamMapView);
impl<'model> core::BlendParamMap<'model> for BlendParamMapView<'model> {
    by_value!(neutral_index, u32);
}

impl<'model> core::ParamMap<'model> for BlendParamMapView<'model> {
    fn keypoints(&self) -> &'model [f32] {
        self.keypoints_slice()
    }
}

declare_item!(PartFormView);
impl<'model> core::PartForm<'model> for PartFormView<'model> {
    by_value!(depth, f32);
}

declare_item!(WarpFormView);
impl<'model> core::WarpForm<'model> for WarpFormView<'model> {
    by_value!(opacity, f32);

    fn vertices(&self) -> &'model [core::Coord] {
        let start = self.i_start_vertex();
        let count = *self.parent().f_vertex_count();
        self.model.vertex_coord.slice(start..start.offset(count))
    }

    fn multiply_color(&self) -> core::Color {
        self.multiply_color_view().color()
    }

    fn screen_color(&self) -> core::Color {
        self.screen_color_view().color()
    }
}

declare_item!(RotFormView);
impl<'model> core::RotForm<'model> for RotFormView<'model> {
    by_value!(opacity, f32);
    by_value!(angle, f32);
    by_value!(scale, f32);
    by_value!(flip_x, bool);
    by_value!(flip_y, bool);

    fn position(&self) -> core::Coord {
        [*self.f_pos_x(), *self.f_pos_y()].into()
    }

    fn multiply_color(&self) -> core::Color {
        self.multiply_color_view().color()
    }

    fn screen_color(&self) -> core::Color {
        self.screen_color_view().color()
    }
}

declare_item!(DrawItemView);
impl<'model> core::DrawItem<'model> for DrawItemView<'model> {
    fn artmesh(&self) -> Option<impl Deref<Target = ArtMeshView<'model>>> {
        if *self.f_item_type() == DrawItemType::ArtMesh {
            Some(ArtMeshView::get_ref(self.model, IArtMesh(*self.f_i_child())).unwrap())
        } else {
            None
        }
    }
}

declare_item!(GlueView);
impl<'model> core::Glue<'model> for GlueView<'model> {
    type Form = GlueFormView<'model>;
    type BlendFormMap = GlueBlendFormMapView<'model>;

    by_ref!(id, &str);
    special!(param_maps);
    special!(form);
    child_collection!(forms, Self::Form, forms_views);
    special!(blend_form_maps, GlueBlendFormMapView);

    fn artmesh_1(&self) -> impl Deref<Target = ArtMeshView<'model>> {
        self.artmesh_1_view().into_ref()
    }

    fn artmesh_2(&self) -> impl Deref<Target = ArtMeshView<'model>> {
        self.artmesh_2_view().into_ref()
    }

    fn coords(&self) -> impl Iterator<Item = [core::GlueCoord; 2]> {
        let arr = self.coords_views();

        (0..arr.count()).step_by(2).map(move |i| {
            let c1 = arr.index(i).unwrap();
            let c2 = arr.index(i + 1).unwrap();

            [
                core::GlueCoord {
                    weight: *c1.f_weight(),
                    vertex_index: *c1.f_vertex_index() as u32,
                },
                core::GlueCoord {
                    weight: *c2.f_weight(),
                    vertex_index: *c2.f_vertex_index() as u32,
                },
            ]
        })
    }
}

declare_item!(GlueFormView);
impl<'model> core::GlueForm<'model> for GlueFormView<'model> {
    by_value!(compatibility, f32);
}

macro_rules! blend_form_map_view {
    ($t:ident, $f:ident) => {
        pub struct $t<'model>(BlendFormMapView<'model>, u32);
        impl<'model> core::BlendFormMap<'model> for $t<'model> {
            type Form = <super::classes::$f as Object>::View<'model>;

            fn param_map(&self) -> impl Deref<Target = BlendParamMapView<'model>> {
                self.0.param_map_view().into_ref()
            }

            fn forms(
                &self,
            ) -> impl ItemArray<'model, <super::classes::$f as Object>::View<'model>> {
                let mut c = self.0.forms_views::<super::classes::$f>();
                c.parent = Some(self.1);
                c
            }

            fn limits(
                &self,
            ) -> impl IntoIterator<Item = impl Deref<Target = BlendWeightLimitView<'model>>> {
                self.0
                    .blendweight_limits_views()
                    .into_iter()
                    .map(|i| i.limit_view().into_ref())
            }
        }
    };
}

blend_form_map_view!(RotBlendFormMapView, RotForm);
blend_form_map_view!(WarpBlendFormMapView, WarpForm);
blend_form_map_view!(ArtMeshBlendFormMapView, ArtMeshForm);
blend_form_map_view!(PartBlendFormMapView, PartForm);
blend_form_map_view!(GlueBlendFormMapView, GlueForm);

declare_item!(BlendWeightLimitView);
impl<'model> core::BlendWeightLimit<'model> for BlendWeightLimitView<'model> {
    item_ref!(param, ParamView<'model>, param_view);

    fn points(&self) -> impl IntoIterator<Item = core::BlendWeightLimitPoint> {
        self.points_views()
            .into_iter()
            .map(|pt| core::BlendWeightLimitPoint {
                value: *pt.f_value(),
                weight: *pt.f_weight(),
            })
    }
}

//////////////////////

impl<'a, T: 'a + View<'a> + core::Item<'a, Model = ParsedModel, Ref<'a> = ViewRef<'a, T>>>
    core::Collection<'a, T> for ItemCollection<'a, T>
{
    fn get(&self, uid: u32) -> Option<T::Ref<'a>> {
        assert!(self.start == 0);
        self.index(uid as usize)
    }
}

impl<'a, T: 'a + View<'a> + core::Item<'a, Model = ParsedModel, Ref<'a> = ViewRef<'a, T>>>
    core::ItemArray<'a, T> for ItemCollection<'a, T>
{
    fn index(&self, index: usize) -> Option<T::Ref<'a>> {
        self.index(index)
    }

    fn count(&self) -> usize {
        self.count()
    }
}

impl<'a, T: 'a + View<'a>> IntoIterator for ItemCollection<'a, T> {
    type Item = ViewRef<'a, T>;
    type IntoIter = ItemIterator<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        ItemIterator::<'a, T> {
            model: self.model,
            idx: self.start,
            limit: self.limit,
            parent: self.parent,
            _p: PhantomData,
        }
    }
}

pub struct ItemIterator<'a, T: View<'a>> {
    model: &'a ParsedModel,
    idx: u32,
    limit: u32,
    parent: Option<u32>,
    _p: PhantomData<T>,
}

impl<'a, T: 'a + View<'a> + Sized> Iterator for ItemIterator<'a, T> {
    type Item = ViewRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.limit {
            return None;
        }
        let mut ret = T::get(
            self.model,
            <T::Object as RawObject>::Idx::new(self.idx as u32),
        );
        if let Some(view) = ret.as_mut() {
            if let Some(idx) = self.parent {
                view.set_parent_idx(idx);
            }
            self.idx += 1;
        }
        ret.map(|a| a.into_ref())
    }
}

impl core::Model for ParsedModel {
    type Uid = u32;
    type UidType = core::uid_type::Sequential;

    type Part<'a> = PartView<'a>;
    type Deformer<'a> = DeformerView<'a>;
    type ArtMesh<'a> = ArtMeshView<'a>;
    type Param<'a> = ParamView<'a>;
    type ParamMap<'a> = ParamMapView<'a>;
    type BlendParamMap<'a> = BlendParamMapView<'a>;
    type BlendWeightLimit<'a> = BlendWeightLimitView<'a>;
    type DrawItem<'a> = DrawItemView<'a>;
    type Glue<'a> = GlueView<'a>;

    fn canvas_properties(&self) -> core::CanvasProperties {
        core::CanvasProperties {
            scale: self.canvas.scale,
            center: vec2(self.canvas.center_x, self.canvas.center_y),
            dimensions: vec2(self.canvas.width, self.canvas.height),
        }
    }

    fn artmeshes(&self) -> impl core::Collection<'_, Self::ArtMesh<'_>> {
        ItemCollection::new(self, 0, self.art_mesh.count as u32)
    }

    fn deformers(&self) -> impl core::Collection<'_, Self::Deformer<'_>> {
        ItemCollection::new(self, 0, self.deformer.count as u32)
    }

    fn draw_items(&self) -> impl core::Collection<'_, Self::DrawItem<'_>> {
        ItemCollection::new(self, 0, self.draw_item.count as u32)
    }

    fn param_maps(&self) -> impl core::Collection<'_, Self::ParamMap<'_>> {
        ItemCollection::new(self, 0, self.param_map.count as u32)
    }

    fn blend_param_maps(&self) -> impl core::Collection<'_, Self::BlendParamMap<'_>> {
        ItemCollection::new(self, 0, self.blend_param_map.count as u32)
    }

    fn blend_weight_limits(&self) -> impl core::Collection<'_, Self::BlendWeightLimit<'_>> {
        ItemCollection::new(self, 0, self.blend_weight_limit.count as u32)
    }

    fn params(&self) -> impl core::Collection<'_, Self::Param<'_>> {
        ItemCollection::new(self, 0, self.param.count as u32)
    }

    fn glues(&self) -> impl core::Collection<'_, Self::Glue<'_>> {
        ItemCollection::new(self, 0, self.glue.count as u32)
    }

    // fn part_forms(&self) -> impl core::Collection<'_, Self::PartForm<'_>> {
    //     TestColl(PhantomData)
    // }

    fn index_buffer(&self) -> Option<&[u16]> {
        Some(&self.vertex_index.values)
    }
    fn texcoord_buffer(&self) -> Option<&[core::Coord]> {
        Some(&self.tex_coord.values)
    }
}
