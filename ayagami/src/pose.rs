use glam::FloatExt;

use crate::core::{self, Item, Param, Part};
use std::{borrow::Cow, collections::HashMap, sync::Arc};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Key<'a> {
    Param(Cow<'a, str>),
    Part(Cow<'a, str>),
}

impl<'a> Key<'a> {
    pub fn param(k: &str) -> Key<'_> {
        Key::Param(Cow::Borrowed(k))
    }
    pub fn part(k: &str) -> Key<'_> {
        Key::Part(Cow::Borrowed(k))
    }
    pub fn from_param(k: String) -> Key<'a> {
        Key::Param(Cow::Owned(k))
    }
    pub fn from_part(k: String) -> Key<'a> {
        Key::Part(Cow::Owned(k))
    }
    pub fn as_owned<'b>(value: Key<'b>) -> Self {
        match value {
            Key::Param(k) => Key::from_param(k.clone().into()),
            Key::Part(k) => Key::from_part(k.clone().into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Descriptor {
    pub key: Key<'static>,
    pub uid: u64,
    pub name: Option<String>,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Debug, Default, Clone)]
pub struct PoseMap {
    descriptors: Vec<Descriptor>,
    key_map: HashMap<Key<'static>, usize>,
}

impl PoseMap {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn from_model<T: core::Model>(model: &T) -> Self {
        let mut map = Self::new();

        for param in model.params() {
            map.add(Descriptor {
                key: Key::from_param(param.id().to_string()),
                uid: param.uid().into(),
                name: None,
                default: param.default(),
                min: param.min(),
                max: param.max(),
            });
        }

        for part in model.parts() {
            map.add(Descriptor {
                key: Key::from_part(part.id().to_string()),
                uid: part.uid().into(),
                name: None,
                default: 1.0,
                min: 0.0,
                max: 1.0,
            });
        }

        map
    }

    pub fn add(&mut self, desc: Descriptor) {
        let idx = self.descriptors.len();
        if self.key_map.insert(desc.key.clone(), idx).is_some() {
            panic!("Duplicate ID");
        }
        self.descriptors.push(desc);
    }

    pub fn add_key(&mut self, key: Key<'static>) {
        if self
            .key_map
            .insert(key.clone(), self.descriptors.len())
            .is_some()
        {
            panic!("Duplicate ID");
        }
        self.descriptors.push(Descriptor {
            key,
            uid: 0,
            name: None,
            min: f32::NEG_INFINITY,
            max: f32::INFINITY,
            default: 0.,
        });
    }

    pub fn index(&self, idx: usize) -> Option<&Descriptor> {
        self.descriptors.get(idx)
    }

    pub fn get(&self, key: &Key) -> Option<(usize, &Descriptor)> {
        let idx = *self.key_map.get(key)?;
        Some((idx, &self.descriptors[idx]))
    }

    pub fn has_key(&self, key: &Key) -> bool {
        self.key_map.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &Key<'static>> {
        self.key_map.keys()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Key<'static>, &Descriptor)> {
        self.key_map.iter().map(|(k, i)| (k, &self.descriptors[*i]))
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &Descriptor> {
        self.descriptors.iter()
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Value {
    pub value: f32,
    pub opacity: f32,
}

impl Value {
    pub fn new(value: f32, opacity: f32) -> Self {
        Self { value, opacity }
    }

    pub fn opaque(value: f32) -> Self {
        Self { value, opacity: 1. }
    }

    pub fn opacity(&self, opacity: f32) -> Self {
        Self {
            value: self.value,
            opacity: self.opacity * opacity,
        }
    }

    pub fn blend(&self, over: &Self, weight: f32) -> Self {
        let a_over = over.opacity * weight;
        if a_over == 1. {
            *over
        } else if a_over == 0. {
            *self
        } else {
            let opacity = a_over + self.opacity * (1. - a_over);
            Self {
                value: (over.value * a_over + self.value * self.opacity * (1. - a_over)) / opacity,
                opacity,
            }
        }
    }

    pub fn disjoint(&self, over: &Self, weight: f32) -> Self {
        let a_over = over.opacity * weight;
        let a_under = self.opacity.min(1. - a_over);
        if a_over == 1. {
            *over
        } else if a_over == 0. {
            *self
        } else {
            let opacity = a_over + a_under;
            Self {
                value: (over.value * a_over + self.value * a_under) / opacity,
                opacity,
            }
        }
    }

    pub fn multiply(&self, over: &Self, weight: f32) -> Self {
        let a_over = over.opacity * weight;
        Self {
            value: self.value.lerp(self.value * over.value, a_over),
            opacity: self.opacity,
        }
    }

    pub fn add(&self, over: &Self, weight: f32) -> Self {
        let a_over = over.opacity * weight;
        let opacity = (self.opacity + a_over).min(1.);
        Self {
            value: self.value.lerp(self.value + over.value, a_over),
            opacity,
        }
    }

    pub fn flatten(&self, default: f32) -> f32 {
        self.value * self.opacity + default * (1. - self.opacity)
    }
}

#[derive(Debug, Clone)]
pub struct Pose {
    map: Arc<PoseMap>,
    values: HashMap<usize, Value>,
}

impl Pose {
    pub fn new<T: core::Model>(model: &T) -> Self {
        Self::with_map(model.pose_map().clone())
    }

    pub fn with_map(map: Arc<PoseMap>) -> Self {
        Self {
            map,
            values: Default::default(),
        }
    }

    pub fn empty() -> Self {
        Self {
            map: Default::default(),
            values: Default::default(),
        }
    }

    pub fn set_value(&mut self, key: &Key, mut value: Value) {
        let Some((idx, _)) = self.map.get(key) else {
            return;
        };
        value.opacity = value.opacity.clamp(0., 1.);
        self.values.insert(idx, value);
    }

    pub fn set(&mut self, key: &Key, value: f32) {
        self.set_value(key, Value::opaque(value));
    }

    pub fn unset(&mut self, key: &Key) {
        if let Some((idx, _)) = self.map.get(key) {
            self.values.remove(&idx);
        }
    }

    pub fn has_key(&self, key: &Key) -> bool {
        self.map.has_key(key)
    }

    pub fn get(&self, key: &Key) -> Option<&Value> {
        self.values.get(&self.map.get(key)?.0)
    }

    pub fn get_mut(&mut self, key: &Key) -> Option<&mut Value> {
        self.values.get_mut(&self.map.get(key)?.0)
    }

    pub fn get_flattened(&self, key: &Key) -> Option<f32> {
        let (idx, desc) = self.map.get(key)?;
        Some(
            self.values
                .get(&idx)
                .map(|v| v.flatten(desc.default))
                .unwrap_or(desc.default),
        )
    }

    pub fn clamp(&mut self) {
        self.iter_desc_mut().for_each(|(desc, v)| {
            *v = Value {
                value: v.value.clamp(desc.min, desc.max),
                opacity: v.opacity.clamp(0., 1.),
            }
        });
    }

    pub fn apply<B, N>(&mut self, other: &Self, blend: B, new: N)
    where
        B: Fn(&Value, &Value) -> Value,
        N: Fn(&Value) -> Option<Value>,
    {
        if Arc::ptr_eq(&self.map, &other.map) {
            for (i2, v2) in other.values.iter() {
                if let Some(v1) = self.values.get_mut(i2) {
                    *v1 = blend(v1, v2);
                } else {
                    if let Some(v) = new(v2) {
                        self.values.insert(*i2, v);
                    }
                }
            }
        } else {
            for (i2, v2) in other.values.iter() {
                let Some((vi, _)) = self.map.get(&other.map.index(*i2).unwrap().key) else {
                    continue;
                };
                if let Some(v1) = self.values.get_mut(&vi) {
                    *v1 = blend(v1, v2);
                } else {
                    if let Some(v) = new(v2) {
                        self.values.insert(*i2, v);
                    }
                }
            }
        }
    }

    pub fn update(&mut self, other: &Self) {
        self.apply(other, |_, b| *b, |b| Some(*b));
    }

    pub fn blend(&mut self, other: &Self, weight: f32) {
        self.apply(
            other,
            |a, b| a.blend(b, weight),
            |b| Some(b.opacity(weight)),
        );
    }

    pub fn disjoint(&mut self, other: &Self, weight: f32) {
        self.apply(
            other,
            |a, b| a.disjoint(b, weight),
            |b| Some(b.opacity(weight)),
        );
    }

    pub fn add(&mut self, other: &Self, weight: f32) {
        self.apply(other, |a, b| a.add(b, weight), |b| Some(b.opacity(weight)));
    }

    pub fn multiply(&mut self, other: &Self, weight: f32) {
        self.apply(other, |a, b| a.multiply(b, weight), |_| None);
    }

    pub fn flatten(&mut self) {
        for (i, v) in self.values.iter_mut() {
            let default = self.map.index(*i).unwrap().default;
            if v.opacity != 1.0 {
                *v = Value {
                    value: v.flatten(default),
                    opacity: 1.,
                }
            }
        }
    }

    pub fn populate(&mut self, opacity: f32) {
        for (i, desc) in self.map.descriptors().enumerate() {
            self.values.entry(i).or_insert(Value {
                value: desc.default,
                opacity,
            });
        }
    }

    pub fn map(&self) -> &Arc<PoseMap> {
        &self.map
    }

    pub fn map_mut(&mut self) -> &mut PoseMap {
        Arc::make_mut(&mut self.map)
    }

    pub fn add_key(&mut self, key: Key<'static>) -> bool {
        if self.map.has_key(&key) {
            false
        } else {
            self.map_mut().add_key(key);
            true
        }
    }

    pub fn set_or_add_value(&mut self, key: Key<'static>, mut value: Value) {
        let (idx, _) = match self.map.get(&key) {
            Some((idx, desc)) => (idx, desc),
            None => {
                let idx = self.map.descriptors.len();
                self.add_key(key);
                (idx, self.map.index(idx).unwrap())
            }
        };
        value.opacity = value.opacity.clamp(0., 1.);
        self.values.insert(idx, value);
    }

    pub fn set_or_add(&mut self, key: Key<'static>, value: f32) {
        self.set_or_add_value(key, Value::opaque(value));
    }

    pub fn keys(&self) -> impl Iterator<Item = &Key<'static>> {
        self.descriptors().map(|d| &d.key)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &Descriptor> {
        self.values.keys().map(|i| self.map.index(*i).unwrap())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Key<'static>, &Value)> {
        self.values
            .iter()
            .map(|(i, v)| (&self.map.index(*i).unwrap().key, v))
    }

    pub fn iter_all(&self) -> impl Iterator<Item = (&Key<'static>, Option<&Value>)> {
        self.map
            .keys()
            .enumerate()
            .map(|(i, k)| (k, self.values.get(&i)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Key<'static>, &mut Value)> {
        self.values
            .iter_mut()
            .map(|(i, v)| (&self.map.index(*i).unwrap().key, v))
    }

    pub fn iter_desc(&self) -> impl Iterator<Item = (&Descriptor, &Value)> {
        self.values
            .iter()
            .map(|(i, v)| (self.map.index(*i).unwrap(), v))
    }

    pub fn iter_desc_all(&self) -> impl Iterator<Item = (&Descriptor, Option<&Value>)> {
        self.map
            .descriptors()
            .enumerate()
            .map(|(i, d)| (d, self.values.get(&i)))
    }

    pub fn iter_desc_mut(&mut self) -> impl Iterator<Item = (&Descriptor, &mut Value)> {
        self.values
            .iter_mut()
            .map(|(i, v)| (self.map.index(*i).unwrap(), v))
    }
}
