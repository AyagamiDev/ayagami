use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::ops::{Deref, Range};

pub use super::file::classes::{BlendMode, DeformerType, ParamSnapType};
use glam::{f32::Vec2, u32::UVec2};

pub type Coord = Vec2;
pub type Color = [f32; 3];

macro_rules! sub_type {
    ($n:ident, $t:ident) => {
        type $n: $t<'model, Model = Self::Model>;
    };
}

macro_rules! pm {
    ($t:ident) => { impl Deref<Target = <Self::Model as Model>::$t<'model>> };
    ($t:ty) => { impl Deref<Target = $t> };
}

macro_rules! p {
    ($t:ident) => { impl Deref<Target = Self::$t> }
}

macro_rules! itm {
    ($t:ident) => { impl IntoIterator<Item = pm!($t)> };
    ($t:ty) => { impl IntoIterator<Item = pm!($t)> };
}

macro_rules! ita {
    ($t:ident) => { impl ItemArray<'model, <Self::Model as Model>::$t<'model>> };
    ($t:ty) => { impl ItemArray<'model, $t> };
}

pub trait Item<'model>: Sized
where
    Self: 'model,
{
    type Model: Model
    where
        Self: 'model;
    type Ref<'a>: Deref<Target = Self>
    where
        'a: 'model;

    fn uid(&self) -> <Self::Model as Model>::Uid;
}

pub trait Part<'model>: Item<'model>
where
    Self: 'model,
{
    sub_type!(Form, PartForm);
    type BlendFormMap: BlendFormMap<'model, Form = Self::Form>;

    fn id(&self) -> &str;
    fn param_maps(&self) -> itm!(ParamMap);
    fn forms(&self) -> ita!(Self::Form);
    fn form(&self, index: &[u32]) -> Option<p!(Form)>;
    fn visible_artmeshes(&self) -> bool;
    fn visible_deformers(&self) -> bool;
    fn parent(&self) -> Option<pm!(Part)>;
    fn blend_form_maps(
        &self,
    ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>>;
}

pub enum TypedDeformer<'model, D: Deformer<'model>>
where
    Self: 'model,
{
    Warp(D::WarpRef<'model>),
    Rotation(D::RotationRef<'model>),
}

pub trait Deformer<'model>: Item<'model>
where
    Self: 'model,
{
    type Warp: WarpDeformer<'model, Model = Self::Model>;
    type Rotation: RotDeformer<'model, Model = Self::Model>;
    type WarpRef<'a>: Deref<Target = Self::Warp>
    where
        'a: 'model;
    type RotationRef<'a>: Deref<Target = Self::Rotation>
    where
        'a: 'model;

    fn id(&self) -> &str;
    fn param_maps(&self) -> itm!(ParamMap);
    fn visible(&self) -> bool;
    fn part(&self) -> Option<pm!(Part)>;
    fn parent(&self) -> Option<pm!(Deformer)>;
    fn typed(&self) -> TypedDeformer<'model, Self>;
}

pub trait WarpDeformer<'model>
where
    Self: 'model,
{
    type Model: Model;
    sub_type!(Form, WarpForm);
    type BlendFormMap: BlendFormMap<'model, Form = Self::Form>;

    fn param_maps(&self) -> itm!(ParamMap);
    fn forms(&self) -> ita!(Self::Form);
    fn form(&self, index: &[u32]) -> Option<p!(Form)>;
    fn size(&self) -> UVec2;
    fn vertex_count(&self) -> usize {
        let s = self.size().as_usizevec2();
        (s + 1).element_product()
    }
    fn bilinear_interpolation(&self) -> bool;
    fn blend_form_maps(
        &self,
    ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>>;
}

pub trait RotDeformer<'model>
where
    Self: 'model,
{
    type Model: Model;
    sub_type!(Form, RotForm);
    type BlendFormMap: BlendFormMap<'model, Form = Self::Form>;

    fn param_maps(&self) -> itm!(ParamMap);
    fn forms(&self) -> ita!(Self::Form)
    where
        Self: 'model;
    fn form(&self, indices: &[u32]) -> Option<p!(Form)>;
    fn angle_offset(&self) -> f32;
    fn blend_form_maps(
        &self,
    ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>>;
}

pub trait ArtMesh<'model>: Item<'model>
where
    Self: 'model,
{
    sub_type!(Form, ArtMeshForm);
    type BlendFormMap: BlendFormMap<'model, Form = Self::Form>;

    fn id(&self) -> &str;
    fn param_maps(&self) -> itm!(ParamMap);
    fn forms(&self) -> ita!(Self::Form);
    fn form(&self, indices: &[u32]) -> Option<p!(Form)>;
    fn visible(&self) -> bool;
    fn part(&self) -> Option<pm!(Part)>
    where
        Self: 'model;
    fn deformer(&self) -> Option<pm!(Deformer)>;
    fn texture(&self) -> u32;
    fn blend_mode(&self) -> BlendMode;
    fn culling(&self) -> bool;
    fn invert_mask(&self) -> bool;
    fn vertex_count(&self) -> u32;
    fn texcoord_offset(&self) -> u32;
    fn index_range(&self) -> Range<u32>;
    fn clips(&self) -> itm!(ArtMesh);
    fn blend_form_maps(
        &self,
    ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>>;
}

pub trait Param<'model>: Item<'model>
where
    Self: 'model,
{
    fn id(&self) -> &str;
    fn min(&self) -> f32;
    fn max(&self) -> f32;
    fn default(&self) -> f32;
    fn repeat(&self) -> bool;
    fn snap_type(&self) -> ParamSnapType;
    fn param_maps(&self) -> ita!(ParamMap);
    fn keypoints(&self) -> Option<&[f32]>;
    fn is_blendshape(&self) -> bool;
    fn blend_param_maps(&self) -> ita!(BlendParamMap);
}

pub trait PartForm<'model>: Item<'model>
where
    Self: 'model,
{
    fn depth(&self) -> f32;
}

pub trait WarpForm<'model>: Item<'model>
where
    Self: 'model,
{
    fn opacity(&self) -> f32;
    fn vertices(&self) -> &'model [Coord];
    fn multiply_color(&self) -> Color;
    fn screen_color(&self) -> Color;
}

pub trait RotForm<'model>: Item<'model>
where
    Self: 'model,
{
    fn opacity(&self) -> f32;
    fn angle(&self) -> f32;
    fn position(&self) -> Coord;
    fn scale(&self) -> f32;
    fn flip_x(&self) -> bool;
    fn flip_y(&self) -> bool;
    fn multiply_color(&self) -> Color;
    fn screen_color(&self) -> Color;
}

pub trait ArtMeshForm<'model>: Item<'model>
where
    Self: 'model,
{
    fn opacity(&self) -> f32;
    fn depth(&self) -> f32;
    fn vertices(&self) -> &'model [Coord];
    fn multiply_color(&self) -> Color;
    fn screen_color(&self) -> Color;
}

pub trait ParamMap<'model>: Item<'model>
where
    Self: 'model,
{
    fn keypoints(&self) -> &'model [f32];
}

pub struct PartDrawItem<'model, M: Model>
where
    M: 'model,
{
    pub part: <M::Part<'model> as Item<'model>>::Ref<'model>,
    pub draw_group: <M::DrawGroup<'model> as Item<'model>>::Ref<'model>,
}

pub enum DrawItem<'model, M: Model>
where
    M: 'model,
{
    ArtMesh(<M::ArtMesh<'model> as Item<'model>>::Ref<'model>),
    Part(PartDrawItem<'model, M>),
}

impl<'model, M: Model> Debug for DrawItem<'model, M>
where
    M: 'model,
    M::Part<'model>: Debug,
    M::DrawGroup<'model>: Debug,
    M::ArtMesh<'model>: Debug,
{
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        match self {
            DrawItem::ArtMesh(am) => f.debug_tuple("ArtMesh").field(&**am).finish(),
            DrawItem::Part(p) => f
                .debug_struct("Part")
                .field("part", &*p.part)
                .field("draw_group", &*p.draw_group)
                .finish(),
        }
    }
}

pub trait DrawGroup<'model>: Item<'model>
where
    Self: 'model,
{
    fn items(&self) -> impl IntoIterator<Item = DrawItem<'model, <Self as Item<'model>>::Model>>;
}

pub struct GlueCoord {
    pub weight: f32,
    pub vertex_index: u32,
}

pub trait Glue<'model>: Item<'model>
where
    Self: 'model,
{
    sub_type!(Form, GlueForm);
    type BlendFormMap: BlendFormMap<'model, Form = Self::Form>;

    fn id(&self) -> &str;
    fn param_maps(&self) -> itm!(ParamMap);
    fn forms(&self) -> ita!(Self::Form);
    fn form(&self, indices: &[u32]) -> Option<p!(Form)>;
    fn artmesh_1(&self) -> pm!(ArtMesh);
    fn artmesh_2(&self) -> pm!(ArtMesh);
    fn coords(&self) -> impl Iterator<Item = [GlueCoord; 2]>;
    fn blend_form_maps(
        &self,
    ) -> Option<impl IntoIterator<Item = impl Deref<Target = Self::BlendFormMap>>>;
}

pub trait GlueForm<'model>: Item<'model>
where
    Self: 'model,
{
    fn compatibility(&self) -> f32;
}

pub trait BlendParamMap<'model>: ParamMap<'model>
where
    Self: 'model,
{
    fn neutral_index(&self) -> u32;
}

pub trait BlendFormMap<'model>
where
    Self: 'model,
{
    type Form: Item<'model>
    where
        Self: 'model;

    fn param_map(
        &self,
    ) -> impl Deref<Target = <<Self::Form as Item<'model>>::Model as Model>::BlendParamMap<'model>>;
    fn forms(&self) -> ita!(Self::Form);
    fn limits(
        &self,
    ) -> impl IntoIterator<
        Item = impl Deref<
            Target = <<Self::Form as Item<'model>>::Model as Model>::BlendWeightLimit<'model>,
        >,
    >;
}

#[derive(Copy, Clone, Debug)]
pub struct BlendWeightLimitPoint {
    pub value: f32,
    pub weight: f32,
}

pub trait BlendWeightLimit<'model>: Item<'model>
where
    Self: 'model,
{
    fn param(&self) -> pm!(Param);
    fn points(&self) -> impl IntoIterator<Item = BlendWeightLimitPoint>;
}

pub trait ItemArray<'model, T: Item<'model>>: IntoIterator<Item = T::Ref<'model>> {
    fn index(&self, index: usize) -> Option<T::Ref<'model>>;
    fn count(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

pub trait Collection<'model, T: Item<'model>>: ItemArray<'model, T> {
    fn get(&self, uid: <T::Model as Model>::Uid) -> Option<T::Ref<'model>>;
}

macro_rules! model_type {
    ($t:ident) => {
        type $t<'model>: $t<'model, Model = Self>
        where
            Self: 'model;
    };
}

pub struct CanvasProperties {
    pub scale: f32,
    pub center: Coord,
    pub dimensions: Coord,
}

pub trait UidStyle {
    type Select<Compact, Sparse>: UidStyle
    where
        Compact: UidStyle,
        Sparse: UidStyle;
}

pub mod uid_type {
    pub struct Sequential;
    pub struct Sparse;
}

pub trait Model {
    type Uid: Eq + Hash + Copy + Display + Debug + Into<u64> + TryInto<usize, Error: Debug>;
    type UidType: crate::collections::UidType;

    model_type!(Part);
    model_type!(Deformer);
    model_type!(ArtMesh);
    model_type!(Param);
    model_type!(ParamMap);
    model_type!(BlendParamMap);
    model_type!(BlendWeightLimit);
    model_type!(DrawGroup);
    model_type!(Glue);

    fn canvas_properties(&self) -> CanvasProperties;

    fn parts(&self) -> impl Collection<'_, Self::Part<'_>>;
    fn deformers(&self) -> impl Collection<'_, Self::Deformer<'_>>;
    fn artmeshes(&self) -> impl Collection<'_, Self::ArtMesh<'_>>;
    fn params(&self) -> impl Collection<'_, Self::Param<'_>>;
    fn param_maps(&self) -> impl Collection<'_, Self::ParamMap<'_>>;
    fn blend_param_maps(&self) -> impl Collection<'_, Self::BlendParamMap<'_>>;
    fn blend_weight_limits(&self) -> impl Collection<'_, Self::BlendWeightLimit<'_>>;
    fn draw_groups(&self) -> impl Collection<'_, Self::DrawGroup<'_>>;
    fn root_draw_group(&self) -> Option<<Self::DrawGroup<'_> as Item<'_>>::Ref<'_>>;
    fn glues(&self) -> impl Collection<'_, Self::Glue<'_>>;

    fn index_buffer(&self) -> Option<&[u16]>;
    fn texcoord_buffer(&self) -> Option<&[Coord]>;
}
