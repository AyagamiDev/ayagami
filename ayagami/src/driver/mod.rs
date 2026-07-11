mod deformer;
use crate::collections::UidCollection;
use crate::core::*;
use deformer::*;
use derive_more;
use log::{debug, info, trace, warn};
use std::{
    cell::Cell,
    collections::HashMap,
    ops::{Deref, Index},
};
use thiserror::Error;

use glam::{
    FloatExt, USizeVec2,
    f32::{Affine2, Mat2, Vec2, Vec3},
    vec2,
};

type ItemStateMap<T, V> =
    UidCollection<<T as Model>::UidType, Vec<V>, HashMap<<T as Model>::Uid, V>>;
struct ItemState<T: Model, V: Default>(ItemStateMap<T, V>);

impl<T: Model, V: Default> ItemState<T, V> {
    fn new() -> Self {
        ItemState(ItemStateMap::<T, V>::new(|| Vec::new(), || HashMap::new()))
    }
    fn get(&self, k: T::Uid) -> Option<&V> {
        self.0
            .visit(|vec| vec.get(k.try_into().unwrap()), |map| map.get(&k))
    }
    fn get_mut(&mut self, k: T::Uid) -> &mut V {
        self.0.visit_mut(
            |vec| &mut vec[k.try_into().unwrap()],
            |map| map.get_mut(&k).unwrap(),
        )
    }
    fn lookup(&mut self, k: T::Uid) -> &mut V {
        self.0.visit_mut(
            |vec| {
                let k = k.try_into().unwrap();
                if k >= vec.len() {
                    vec.resize_with(k + 1, Default::default);
                }
                &mut vec[k]
            },
            |map| map.entry(k).or_insert(Default::default()),
        )
    }
    fn insert(&mut self, k: T::Uid, v: V) {
        self.0.put_mut(
            |vec, v| {
                let k = k.try_into().unwrap();
                if k >= vec.len() {
                    vec.resize_with(k + 1, Default::default);
                }
                vec[k] = v;
            },
            |map, v| {
                map.insert(k, v);
            },
            v,
        )
    }
    fn clear(&mut self) {
        self.0.visit_mut(|vec| vec.clear(), |map| map.clear())
    }
    fn contains_key(&self, k: T::Uid) -> bool {
        self.0.visit(
            |vec| k.try_into().unwrap() < vec.len(),
            |map| map.contains_key(&k),
        )
    }
}

impl<T: Model, V: Default> Default for ItemState<T, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Model, V: Default> Index<T::Uid> for ItemState<T, V> {
    type Output = V;

    fn index(&self, k: T::Uid) -> &Self::Output {
        self.0
            .visit(|vec| &vec[k.try_into().unwrap()], |map| &map[&k])
    }
}

#[derive(Debug, Clone, Default)]
pub struct Visual {
    pub visible: bool,
    pub opacity: f32,
    pub multiply_color: Vec3,
    pub screen_color: Vec3,
}

impl From<VisualVals> for Visual {
    fn from(v: VisualVals) -> Self {
        let v = v.saturate();
        Self {
            visible: true,
            opacity: v.opacity,
            multiply_color: v.multiply_color,
            screen_color: v.screen_color,
        }
    }
}

#[derive(Default, derive_more::Debug)]
struct ArtMeshState {
    updated: bool,
    visual: Visual,
    #[debug("vertices: [{}..{}, {} verts]",
        vertices.iter().copied().reduce(Vec2::min).unwrap(),
        vertices.iter().copied().reduce(Vec2::max).unwrap(),
        vertices.len()
    )]
    vertices: Vec<Coord>,
    #[debug("glued_vertices: [{:?}..{:?}, {:?} verts]",
        glued_vertices.as_ref().map(|a| a.iter().copied().reduce(Vec2::min).unwrap()),
        glued_vertices.as_ref().map(|a| a.iter().copied().reduce(Vec2::max).unwrap()),
        glued_vertices.as_ref().map(|a| a.len()),
    )]
    glued_vertices: Option<Vec<Coord>>,
}

#[derive(Default, derive_more::Debug, Clone)]
struct WarpState {
    visual: Visual,
    size: USizeVec2,
    bilinear: bool,
    #[debug("vertices: [{}..{}, {} verts]",
        vertices.iter().copied().reduce(Vec2::min).unwrap(),
        vertices.iter().copied().reduce(Vec2::max).unwrap(),
        vertices.len()
    )]
    vertices: Vec<Coord>,
    scale: f32,
    affine: Cell<Option<Affine2>>,
}

#[derive(Default, Debug, Clone)]
struct RotState {
    visual: Visual,
    affine: Affine2,
    scale: f32,
}

#[derive(Debug)]
enum DeformerSubState {
    Warp(WarpState),
    Rot(RotState),
}

#[derive(Default, Debug)]
struct DeformerState {
    clean: bool,
    updated: bool,
    sub: Option<DeformerSubState>,
}

impl Visual {
    fn apply(&self, visual: &mut Visual) {
        visual.opacity *= self.opacity;
        visual.visible = visual.visible && self.visible;
        visual.multiply_color *= self.multiply_color;
        visual.screen_color = (visual.screen_color + self.screen_color).saturate();
    }
}

impl RotState {
    fn apply(&self, coords: &mut [Coord], visual: &mut Visual) {
        trace!("Apply Rot: {:?} to {} vertices", self.affine, coords.len());
        coords
            .iter_mut()
            .for_each(|c| *c = self.affine.transform_point2(*c));
        self.visual.apply(visual);
    }
}

impl WarpState {
    fn get_affine(&self) -> Affine2 {
        if let Some(aff) = self.affine.get() {
            return aff;
        }
        // Four corners
        let p00 = self.point(USizeVec2::ZERO);
        let p01 = self.point(self.size * USizeVec2::X);
        let p10 = self.point(self.size * USizeVec2::Y);
        let p11 = self.point(self.size);
        // Compute "average" affine transform
        let pc = (p00 + p01 + p10 + p11) / 4.0;
        let dx = (p01 - p00 + p11 - p10) / 2.0;
        let dy = (p10 - p00 + p11 - p01) / 2.0;
        let aff = Affine2 {
            matrix2: Mat2::from_cols(dx, dy),
            translation: pc,
        };
        // Center of the deformer is input coord (0.5, 0.5)
        let aff = aff * Affine2::from_translation(Vec2::splat(-0.5));
        self.affine.set(Some(aff));
        aff
    }
    fn point(&self, pt: USizeVec2) -> Vec2 {
        assert!(pt.x <= self.size.x);
        assert!(pt.y <= self.size.y);
        let row = self.size.x + 1;
        self.vertices[row * pt.y + pt.x]
    }
    fn extrap_point(&self, pt: USizeVec2) -> Vec2 {
        if pt.min_element() >= 1 && pt.x <= (self.size.x + 1) && pt.y <= (self.size.y + 1) {
            return self.point(pt - 1);
        }

        let ix = if pt.x == 0 {
            -2.
        } else if pt.x > (self.size.x + 1) {
            3.
        } else {
            (pt.x - 1) as f32 / self.size.x as f32
        };

        let iy = if pt.y == 0 {
            -2.
        } else if pt.y > (self.size.y + 1) {
            3.
        } else {
            (pt.y - 1) as f32 / self.size.y as f32
        };

        self.get_affine().transform_point2(vec2(ix, iy))
    }
    fn apply(&self, coords: &mut [Coord], visual: &mut Visual) {
        trace!(
            "Apply Warp {}x{} ({}): {} vertices",
            self.size.x,
            self.size.y,
            self.vertices.len(),
            coords.len()
        );

        let fsize = self.size.as_vec2();

        coords.iter_mut().for_each(|c| {
            if c.min_element() < 0. || c.max_element() > 1. {
                let aff = self.get_affine();
                if c.min_element() <= -2.0 || c.max_element() >= 3.0 {
                    // Out of expanded warp area, apply affine transform and return
                    *c = aff.transform_point2(*c);
                    return;
                }
                let mut rpos = *c * fsize;
                if rpos.x < 0. {
                    rpos.x = c.x / 2.;
                } else if rpos.x > fsize.x {
                    rpos.x = (c.x - 1.) / 2. + fsize.x;
                }
                if rpos.y < 0. {
                    rpos.y = c.y / 2.;
                } else if rpos.y >= fsize.y {
                    rpos.y = (c.y - 1.) / 2. + fsize.y;
                }
                let rpos = rpos + 1.;
                let ipos = rpos.as_usizevec2().clamp(USizeVec2::ZERO, self.size + 1);
                let fpos = rpos - ipos.as_vec2();
                assert!(fpos.min_element() >= 0. && fpos.max_element() <= 1.);
                let p00 = self.extrap_point(ipos);
                let p01 = self.extrap_point(ipos + USizeVec2::X);
                let p10 = self.extrap_point(ipos + USizeVec2::Y);
                let p11 = self.extrap_point(ipos + USizeVec2::ONE);
                if (fpos.x + fpos.y) < 1. {
                    *c = (1. - (fpos.x + fpos.y)) * p00 + fpos.x * p01 + fpos.y * p10;
                } else {
                    let fpos = vec2(1., 1.) - fpos;
                    *c = (1. - (fpos.x + fpos.y)) * p11 + fpos.x * p10 + fpos.y * p01;
                }
            } else {
                let rpos = *c * fsize;
                let ipos = rpos.as_usizevec2().clamp(USizeVec2::ZERO, self.size - 1);
                let fpos = rpos - ipos.as_vec2();
                //println!("rpos={:?} ipos={:?}, fpos={:?}", rpos, ipos, fpos);
                let p00 = self.point(ipos);
                let p01 = self.point(ipos + USizeVec2::X);
                let p10 = self.point(ipos + USizeVec2::Y);
                let p11 = self.point(ipos + USizeVec2::ONE);
                if self.bilinear {
                    let p0 = p00.lerp(p01, fpos.x);
                    let p1 = p10.lerp(p11, fpos.x);
                    *c = p0.lerp(p1, fpos.y);
                } else {
                    if (fpos.x + fpos.y) < 1. {
                        *c = (1. - (fpos.x + fpos.y)) * p00 + fpos.x * p01 + fpos.y * p10;
                    } else {
                        let fpos = vec2(1., 1.) - fpos;
                        *c = (1. - (fpos.x + fpos.y)) * p11 + fpos.x * p10 + fpos.y * p01;
                    }
                }
            }
        });
        self.visual.apply(visual);
    }
}

impl DeformerState {
    fn apply(&self, coords: &mut [Coord], visual: &mut Visual) {
        let sub = self.sub.as_ref().unwrap();

        match sub {
            DeformerSubState::Rot(r) => r.apply(coords, visual),
            DeformerSubState::Warp(w) => w.apply(coords, visual),
        }
    }
    fn scale(&self) -> f32 {
        let sub = self.sub.as_ref().unwrap();

        match sub {
            DeformerSubState::Rot(r) => r.scale,
            DeformerSubState::Warp(w) => w.scale,
        }
    }
}

#[derive(Default, Debug)]
struct ParamState {
    exists: bool,
    clean: bool,
    value: f32,
}

#[derive(Default, Debug)]
struct ParamMapState {
    clean: bool,
    value: Option<(usize, f32)>,
}

#[derive(Default, Debug)]
struct BlendLimitState {
    updated: bool,
    weight: f32,
}

pub struct Driver<T: Model> {
    param_uids: HashMap<String, T::Uid>,
    param: ItemState<T, ParamState>,
    param_map: ItemState<T, ParamMapState>,
    blend_param_map: ItemState<T, ParamMapState>,
    blend_limit: ItemState<T, BlendLimitState>,
    deformer: ItemState<T, DeformerState>,
    artmesh: ItemState<T, ArtMeshState>,
    perftest_mode: bool,
}

#[derive(Default)]
pub struct DrivenArtMesh<'a> {
    pub updated: bool,
    pub visual: Visual,
    pub vertices: &'a [Coord],
}

#[derive(Error, Debug)]
pub enum ParamError {
    #[error("Parameter {0} does not exist")]
    ParameterNotFound(String),
}

impl<T: Model> Driver<T> {
    pub fn new(model: &T) -> Self {
        let mut param = ItemState::new();
        let mut param_uids = HashMap::new();

        for p in model.params() {
            param_uids.insert(p.id().to_string(), p.uid());
            param.insert(
                p.uid(),
                ParamState {
                    exists: true,
                    clean: false,
                    value: p.default(),
                },
            );
        }

        Self {
            param_uids,
            param,
            param_map: Default::default(),
            blend_param_map: Default::default(),
            blend_limit: Default::default(),
            deformer: Default::default(),
            artmesh: Default::default(),
            perftest_mode: false,
        }
    }

    pub fn set_param_by_id(&mut self, id: &str, value: f32) -> Result<(), ParamError> {
        let uid = self
            .param_uids
            .get(id)
            .ok_or_else(|| ParamError::ParameterNotFound(id.to_string()))?;

        self.set_param(*uid, value)?;

        Ok(())
    }

    pub fn set_param(&mut self, uid: T::Uid, value: f32) -> Result<(), ParamError> {
        if !self.param.contains_key(uid) {
            return Err(ParamError::ParameterNotFound(format!("#{}", uid)));
        }

        let st = self.param.get_mut(uid);
        if !st.exists {
            return Err(ParamError::ParameterNotFound(format!("#{}", uid)));
        }
        st.clean = st.clean && st.value == value;
        st.value = value;

        Ok(())
    }

    fn get_form_set<'a, F>(
        &self,
        model: &'a T,
        maps: impl IntoIterator<Item = impl Deref<Target = T::ParamMap<'a>>>,
        forms: impl ItemArray<'a, F>,
        blends: Option<impl IntoIterator<Item = impl Deref<Target: BlendFormMap<'a, Form = F>>>>,
    ) -> Option<(Vec<<F as Item<'a>>::Ref<'a>>, Vec<f32>)>
    where
        F: Item<'a, Model = T>,
    {
        let mut states = Vec::new();

        for map in maps.into_iter() {
            let st = &self.param_map[map.uid()];
            states.push((map.keypoints().len(), st.value?));
        }
        assert!(states.len() < 32);

        let mut form_list = Vec::new();
        let mut weights = Vec::new();

        let form_count = 1u32 << states.len();

        for i in 0..form_count {
            let mut stride = 1;
            let mut index = 0;
            let mut weight = 1.0;
            for (j, (count, value)) in states.iter().enumerate() {
                if i & (1 << j) != 0 {
                    index += stride * (value.0 + 1);
                    weight *= value.1;
                } else {
                    index += stride * value.0;
                    weight *= 1.0 - value.1;
                }
                stride *= count;
            }
            form_list.push(forms.index(index).unwrap());
            weights.push(weight);
        }

        if let Some(blends) = blends {
            for blend in blends.into_iter() {
                let mut weight: f32 = 1.0;
                for limit in blend.limits().into_iter() {
                    weight = weight.min(self.blend_limit[limit.uid()].weight);
                }
                let neutral = blend.param_map().neutral_index() as usize;
                let st = &self.blend_param_map[blend.param_map().uid()];
                form_list.push(blend.forms().index(st.value?.0).unwrap());
                form_list.push(blend.forms().index(st.value?.0 + 1).unwrap());
                weights.push(if st.value?.0 == neutral {
                    0.
                } else {
                    weight * (1. - st.value?.1)
                });
                weights.push(if (st.value?.0 + 1) == neutral {
                    0.
                } else {
                    weight * st.value?.1
                });
            }
        }

        Some((form_list, weights))
    }

    fn calc_rot<'model>(
        &self,
        model: &'model T,
        deformer: &T::Deformer<'model>,
        rot: <T::Deformer<'model> as Deformer<'model>>::RotationRef<'model>,
    ) -> RotState {
        let Some((forms, weights)) =
            self.get_form_set(model, rot.param_maps(), rot.forms(), rot.blend_form_maps())
        else {
            // Out of range, return default (invisible) state
            return Default::default();
        };

        let values: Vec<RotFormVals> = forms.into_iter().map(|f| RotFormVals::new(&*f)).collect();

        let form = blend(&values, &weights);

        trace!(
            "  ++ Scale={:?} Angle={:?} Pos={:?} (blended {} forms)",
            form.scale,
            form.angle,
            form.pos,
            weights.len(),
        );

        let mut st = RotState {
            affine: Affine2::from_scale_angle_translation(
                Vec2::splat(form.scale),
                form.angle.to_radians(),
                form.pos,
            ),
            visual: form.visual.into(),
            scale: form.scale,
        };
        st.visual.visible = deformer.visible();

        if let Some(parent) = deformer.parent() {
            let uid = parent.uid();
            drop(parent);
            let pst = &self.deformer[uid];

            st.scale *= pst.scale();
            pst.apply(&mut [], &mut st.visual);
            match pst.sub.as_ref().unwrap() {
                DeformerSubState::Rot(r) => {
                    st.affine = r.affine * st.affine;
                }
                DeformerSubState::Warp(w) => {
                    let mut p = vec![
                        st.affine.translation,
                        st.affine.translation + vec2(0., -0.1),
                    ];
                    pst.apply(&mut p, &mut st.visual);
                    let angle = if p[1] != p[0] {
                        (p[1] - p[0]).perp().to_angle()
                    } else {
                        0.
                    };
                    st.affine.matrix2 =
                        Mat2::from_scale_angle(Vec2::splat(w.scale), angle) * st.affine.matrix2;
                    st.affine.translation = p[0];
                }
            }
        }

        st
    }

    fn calc_warp<'model>(
        &self,
        model: &'model T,
        deformer: &T::Deformer<'model>,
        warp: <T::Deformer<'model> as Deformer<'model>>::WarpRef<'model>,
    ) -> WarpState {
        let Some((forms, weights)) = self.get_form_set(
            model,
            warp.param_maps(),
            warp.forms(),
            warp.blend_form_maps(),
        ) else {
            // Out of range, return default (invisible) state
            return Default::default();
        };

        let values: Vec<WarpFormVals> = forms.iter().map(|f| WarpFormVals::new(&**f)).collect();

        let arrays: Vec<&[Coord]> = forms.into_iter().map(|f| f.vertices()).collect();

        let form = blend(&values, &weights);
        let mut vertices = Vec::new();
        vertices.resize(warp.vertex_count(), Vec2::ZERO);

        blend_arrays(&arrays, &mut vertices, &weights);

        let mut st = WarpState {
            vertices,
            size: warp.size().as_usizevec2(),
            bilinear: warp.bilinear_interpolation(),
            scale: 1.0,
            visual: form.visual.into(),
            affine: None.into(),
        };
        st.visual.visible = deformer.visible();

        if let Some(parent) = deformer.parent() {
            let uid = parent.uid();
            drop(parent);
            let pst = &self.deformer[uid];
            st.scale *= pst.scale();
            pst.apply(&mut st.vertices, &mut st.visual);
        }

        st
    }

    fn calc_deformer<'a>(&mut self, model: &'a T, deformer: &T::Deformer<'a>) -> bool {
        let st = self.deformer.lookup(deformer.uid());

        if st.clean {
            return st.updated;
        }
        st.clean = true;

        let mut changed = !st.sub.is_some();

        for pm in deformer.param_maps().into_iter() {
            let pm_state = &self.param_map[pm.uid()];
            if !pm_state.clean {
                changed = true;
                break;
            }
        }

        fn check_bfm<'model, B: BlendFormMap<'model>>(
            this: &Driver<<B::Form as Item<'model>>::Model>,
            bfm: &B,
        ) -> bool {
            if !this.blend_param_map[bfm.param_map().uid()].clean {
                return true;
            }
            for l in bfm.limits() {
                if this.blend_limit[l.uid()].updated {
                    return true;
                }
            }
            return false;
        }

        match deformer.typed() {
            TypedDeformer::Warp(w) => {
                for bfm in w.blend_form_maps().into_iter().flatten() {
                    if check_bfm(self, &*bfm) {
                        changed = true;
                        break;
                    }
                }
            }
            TypedDeformer::Rotation(r) => {
                for bfm in r.blend_form_maps().into_iter().flatten() {
                    if check_bfm(self, &*bfm) {
                        changed = true;
                        break;
                    }
                }
            }
        };

        if let Some(parent) = deformer.parent() {
            if self.calc_deformer(model, &*parent) {
                changed = true;
            }
        }

        if !changed {
            return false;
        }

        let st = self.deformer.get_mut(deformer.uid());

        trace!(
            ">> Update defomer #{} {} {}/{}",
            deformer.uid(),
            deformer.id(),
            st.clean,
            st.updated
        );

        let new_state = match deformer.typed() {
            TypedDeformer::Warp(w) => DeformerSubState::Warp(self.calc_warp(model, deformer, w)),
            TypedDeformer::Rotation(r) => DeformerSubState::Rot(self.calc_rot(model, deformer, r)),
        };

        trace!(
            "<< Updated defomer #{} {}: {:?}",
            deformer.uid(),
            deformer.id(),
            new_state
        );

        let st = self.deformer.get_mut(deformer.uid());
        st.clean = true;
        st.updated = true;
        st.sub = Some(new_state);

        return true;
    }

    pub fn deformer_tree_changed(&mut self) {
        self.deformer.clear();
        self.artmesh.clear();
    }

    pub fn part_tree_changed(&mut self) {
        self.deformer.clear();
        self.artmesh.clear();
    }

    pub fn drive(&mut self, model: &T) {
        for param in model.params() {
            let pstate = self.param.lookup(param.uid());
            if self.perftest_mode {
                pstate.clean = false;
            }
            if !pstate.clean {
                let value = pstate.value.min(param.max()).max(param.min());
                pstate.value = value;
                trace!(
                    ">> Updating param #{} {}: {:?}",
                    param.uid(),
                    param.id(),
                    pstate
                );
                let calc_param_map = |tname, uid, keypoints: &[f32], mstate: &mut ParamMapState| {
                    mstate.clean = false;
                    let kp = keypoints;
                    mstate.value = None;
                    if kp.len() < 2 {
                        warn!(
                            "{} #{} (param #{} {}) has <2 keypoints: {:?}",
                            tname,
                            uid,
                            param.uid(),
                            param.id(),
                            kp,
                        );
                    } else if value < *kp.first().unwrap() || value > *kp.last().unwrap() {
                        debug!(
                            "  Value {} is out of range for {} #{} ({:?})",
                            value, tname, uid, kp
                        );
                    } else {
                        for (i, (a, b)) in kp.iter().zip(kp[1..].iter()).enumerate() {
                            if value == *b {
                                mstate.value = Some((i, 1.0));
                            } else if value >= *a && value < *b {
                                mstate.value = Some((i, f32::inverse_lerp(*a, *b, value)));
                            }
                        }
                    }
                    trace!(
                        "  {} #{}: {} -> {:?} ({:?})",
                        tname, uid, value, mstate.value, kp
                    );
                };

                for map in param.param_maps() {
                    let mstate = self.param_map.lookup(map.uid());
                    calc_param_map("ParamMap", map.uid(), map.keypoints(), mstate);
                }
                for map in param.blend_param_maps() {
                    let mstate = self.blend_param_map.lookup(map.uid());
                    calc_param_map("BlendParamMap", map.uid(), map.keypoints(), mstate);
                }
                trace!(
                    "<< Updated param #{} {}: {:?}",
                    param.uid(),
                    param.id(),
                    pstate
                );
            } else {
                for map in param.param_maps() {
                    self.param_map.get_mut(map.uid()).clean = true;
                }
                for map in param.blend_param_maps() {
                    self.blend_param_map.get_mut(map.uid()).clean = true;
                }
            }
        }

        for l in model.blend_weight_limits() {
            let param = &self.param[l.param().uid()];
            let st = self.blend_limit.lookup(l.uid());
            if !param.clean {
                let old = st.weight;
                st.weight = l.points().into_iter().last().unwrap().weight;

                for (a, b) in l.points().into_iter().zip(l.points().into_iter().skip(1)) {
                    if param.value < a.value {
                        st.weight = a.weight;
                        break;
                    } else if param.value >= a.value && param.value < b.value {
                        let t = f32::inverse_lerp(a.value, b.value, param.value);
                        st.weight = a.weight.lerp(b.weight, t);
                        break;
                    }
                }
                st.updated = old != st.weight;
            } else {
                st.updated = false;
            }
        }

        for d in model.deformers() {
            let st = self.deformer.lookup(d.uid());
            st.clean = false;
            st.updated = false;
        }

        if self.perftest_mode {
            for deformer in model.deformers() {
                self.calc_deformer(model, &deformer);
            }
        }

        for artmesh in model.artmeshes() {
            let mut changed = !self.artmesh.contains_key(artmesh.uid());
            if !changed {
                self.artmesh.lookup(artmesh.uid()).updated = false;
            }

            if let Some(deformer) = artmesh.deformer() {
                let deformer_changed = self.calc_deformer(model, &*deformer);
                changed = changed || deformer_changed;
            }

            if !changed {
                for pm in artmesh.param_maps() {
                    let pm_state = &self.param_map[pm.uid()];
                    if !pm_state.clean {
                        changed = true;
                        break;
                    }
                }
            }
            if !changed {
                'bfm: for bfm in artmesh.blend_form_maps().into_iter().flatten() {
                    for l in bfm.limits() {
                        if self.blend_limit[l.uid()].updated {
                            changed = true;
                            break 'bfm;
                        }
                    }
                }
            }

            if !changed && self.artmesh.contains_key(artmesh.uid()) {
                continue;
            }

            let Some((forms, weights)) = self.get_form_set(
                model,
                artmesh.param_maps(),
                artmesh.forms(),
                artmesh.blend_form_maps(),
            ) else {
                // Out of range, return default (invisible) state
                debug!("ArtMesh #{} {}: Out of range", artmesh.uid(), artmesh.id());
                self.artmesh.insert(artmesh.uid(), Default::default());
                continue;
            };

            let values: Vec<ArtMeshFormVals> =
                forms.iter().map(|f| ArtMeshFormVals::new(&**f)).collect();

            let arrays: Vec<&[Coord]> = forms.into_iter().map(|f| f.vertices()).collect();

            let form = blend(&values, &weights);
            let mut vertices = Vec::new();
            vertices.resize(artmesh.vertex_count() as usize, Vec2::ZERO);

            blend_arrays(&arrays, &mut vertices, &weights);

            let mut visual: Visual = form.visual.into();
            visual.visible = artmesh.visible();

            if let Some(uid) = artmesh.deformer().map(|d| d.uid()) {
                let pst = &self.deformer[uid];
                pst.apply(&mut vertices, &mut visual);
            }

            debug!("Updated ArtMesh #{}: {}", artmesh.uid(), artmesh.id());
            let st = ArtMeshState {
                updated: true,
                visual,
                vertices,
                glued_vertices: None,
            };
            self.artmesh.insert(artmesh.uid(), st);
        }

        // Propagate glue invalidation forwards and backwards
        // Until no more glues left to invalidate
        let r = 0..model.glues().count();
        loop {
            let mut progress = false;
            for i in r.clone().into_iter().chain(r.clone().rev().into_iter()) {
                let glue = model.glues().index(i).unwrap();
                let uid_1 = glue.artmesh_1().uid();
                let uid_2 = glue.artmesh_2().uid();
                if self.artmesh[uid_1].updated {
                    let st = self.artmesh.get_mut(uid_2);
                    if !st.updated {
                        progress = true;
                        st.updated = true;
                    }
                    st.glued_vertices = None;
                }
                if self.artmesh[uid_2].updated {
                    let st = self.artmesh.get_mut(uid_1);
                    if !st.updated {
                        progress = true;
                        st.updated = true;
                    }
                    st.glued_vertices = None;
                }
            }
            if !progress {
                break;
            }
        }

        // Apply Glue
        'glue: for glue in model.glues() {
            let uid_1 = glue.artmesh_1().uid();
            let uid_2 = glue.artmesh_2().uid();

            // First, copy clean vertices over
            let mut updated = false;
            for uid in [uid_1, uid_2] {
                let st = self.artmesh.get_mut(uid);
                if st.vertices.is_empty() {
                    // If no vertices one is invalid, skip this glue item
                    continue 'glue;
                }
                if st.glued_vertices.is_none() {
                    st.glued_vertices = Some(st.vertices.clone());
                }
                updated = updated || st.updated;
            }

            // No changes for this glue item
            if !updated {
                continue;
            }

            debug!("Applying glue #{}: {}", glue.uid(), glue.id());
            // Then take them, to apply glue
            let mut verts_1 = self.artmesh.get_mut(uid_1).glued_vertices.take().unwrap();
            let mut verts_2 = self.artmesh.get_mut(uid_2).glued_vertices.take().unwrap();

            let Some((forms, weights)) = self.get_form_set(
                model,
                glue.param_maps(),
                glue.forms(),
                glue.blend_form_maps(),
            ) else {
                // Out of range, ignore
                info!("Glue #{} {}: Out of range", glue.uid(), glue.id());
                continue;
            };

            let values: Vec<f32> = forms.into_iter().map(|f| f.compatibility()).collect();

            let compatibility = blend(&values, &weights);

            for [coord_1, coord_2] in glue.coords() {
                let v1 = &mut verts_1[coord_1.vertex_index as usize];
                let v2 = &mut verts_2[coord_2.vertex_index as usize];
                let vg1 = v1.lerp(*v2, coord_1.weight);
                let vg2 = v2.lerp(*v1, coord_2.weight);
                *v1 = v1.lerp(vg1, compatibility);
                *v2 = v2.lerp(vg2, compatibility);
            }

            // Put them back
            let st = self.artmesh.get_mut(uid_1);
            st.glued_vertices = Some(verts_1);
            let st = self.artmesh.get_mut(uid_2);
            st.glued_vertices = Some(verts_2);
        }

        for param in model.params() {
            self.param.get_mut(param.uid()).clean = true;
            for map in param.param_maps() {
                self.param_map.get_mut(map.uid()).clean = true;
            }
        }
    }

    pub fn artmesh_state(&self, uid: T::Uid) -> Option<DrivenArtMesh<'_>> {
        let st = self.artmesh.get(uid)?;
        Some(DrivenArtMesh {
            updated: st.updated,
            visual: st.visual.clone(),
            vertices: st.glued_vertices.as_ref().unwrap_or(&st.vertices),
        })
    }
}
