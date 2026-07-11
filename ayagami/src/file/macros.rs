#![allow(unused)]

use super::parse::ReadArray;

macro_rules! primitive_reader {
    ($type:ty, $fn:path) => {
        impl ReadArray for $type {
            fn read_array(dest: &mut [Self], data: &mut SectionReader) -> Result<(), ParseError> {
                data.next_section()?;
                $fn(data.rdr, dest);
                data.p += dest.len() * std::mem::size_of::<Self>();
                Ok(())
            }
        }
    };
}

macro_rules! transparent_reader {
    ($type:ty, $prim:ty) => {
        impl ReadArray for $type {
            fn read_array(dest: &mut [Self], data: &mut SectionReader) -> Result<(), ParseError> {
                use zerocopy::transmute_mut;

                let map_dest: &mut [$prim] = transmute_mut!(dest);
                <$prim as ReadArray>::read_array(map_dest, data)?;
                Ok(())
            }
        }
    };
}

macro_rules! enum_conversion {
    ($type:ty, $prim:ty) => {
        impl TryFrom<$prim> for $type {
            type Error = ParseError;

            fn try_from(raw: $prim) -> Result<Self, ParseError> {
                <$type>::from_repr(raw)
                    .ok_or_else(|| ParseError::InvalidValue(format!("{:?}", raw)))
            }
        }
    };
}

macro_rules! declare_object_fields {
    ( @ $obj:ident { $field:ident : &&[$type:ty] $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [<i_ $field>]: ArrayType<<$type as RawObject>::Idx>,
            pub(crate) [<cnt_ $field>]: ArrayType<u32>,
        ));
    };

    ( @ $obj:ident { $field:ident : &[$type:ty] $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [<i_ $field>]: ArrayType<<$type as RawObject>::Idx>,
            pub(crate) [<cnt_ $field>]: ArrayType<u32>,
        ));
    };

    ( @ $obj:ident { $field:ident : &&$type:ty $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [<i_ $field>]: ArrayType<<$type as RawObject>::Idx>,
        ));
    };

    ( @ $obj:ident { $field:ident : &$type:ty $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [<i_ $field>]: ArrayType<<$type as RawObject>::Idx>,
        ));
    };

    ( @ $obj:ident { $field:ident : Option<&&$type:ty> $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [<i_ $field>]: ArrayType<<$type as RawObject>::OptIdx>,
        ));
    };

    ( @ $obj:ident { $field:ident : $file_type:ty => $mem_type: ty $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) $field: ArrayType<$mem_type>,
        ));
    };

    ( @ $obj:ident { $field:ident : $type:ty $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_object_fields!(@ $obj { $($($rest)*)? } -> (
            $($result)*
            pub(crate) $field: ArrayType<$type>,
        ));
    };

    ( @ $obj:ident { $(,)? } -> ($($result:tt)*)) => {
        paste!{
            #[derive(Debug, Default)]
            pub(crate) struct [<$obj Fields>] {
                pub(crate) count: usize,
                $($result)*
            }
        }
    };

    ( $obj:ident, {
        $($pass:ident {
            $($field:tt)+
        }),+ $(,)?
    }) => {
        declare_object_fields!(@ $obj {
            $($($field)+)+
        } -> ());
    };
}

macro_rules! parse_object_fields {
    ( @ $self:ident $obj:ident $data:ident { $field:ident : &&[$type:ty] $(,$($rest:tt)*)? }) => {
        paste!{ Self::parse_arrayref(
            &mut $self.[<i_ $field>],
            &mut $self.[<cnt_ $field>],
            stringify!($obj.[<i_ $field>]),
            stringify!($obj.[<cnt_ $field>]),
            $self.count,
            $data
        )?;}
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : &[$type:ty] $(,$($rest:tt)*)? }) => {
        paste!{ Self::parse_arrayref(
            &mut $self.[<i_ $field>],
            &mut $self.[<cnt_ $field>],
            stringify!($obj.[<i_ $field>]),
            stringify!($obj.[<cnt_ $field>]),
            $self.count,
            $data
        )?;}
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : &&$type:ty $(,$($rest:tt)*)? }) => {
        paste!{ Self::parse_ref(&mut $self.[<i_ $field>], stringify!($obj.$field), $self.count, $data)?; }
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : &$type:ty $(,$($rest:tt)*)? }) => {
        paste!{ Self::parse_ref(&mut $self.[<i_ $field>], stringify!($obj.$field), $self.count, $data)?; }
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : Option<&&$type:ty> $(,$($rest:tt)*)? }) => {
        paste!{ Self::parse_opt_ref(&mut $self.[<i_ $field>], stringify!($obj.$field), $self.count, $data)?; }
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : $file_type:ty => $mem_type: ty $(,$($rest:tt)*)? }) => {
        Self::parse_convert::<$file_type, $mem_type>(&mut $self.$field, stringify!($obj.$field), $self.count, $data)?;
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $field:ident : $type:ty $(,$($rest:tt)*)? }) => {
        Self::parse_prim(&mut $self.$field, stringify!($obj.$field), $self.count, $data)?;
        parse_object_fields!(@ $self $obj $data { $($($rest)*)? });
    };

    ( @ $self:ident $obj:ident $data:ident { $(,)? } ) => {};

    ( $self:ident, $obj:ident, $parsing_pass:expr, $data:ident, {
        $($pass:ident {
            $($field:tt)+
        }),+ $(,)?
    }) => {
        $(
            if ($parsing_pass == Pass::$pass) {
                parse_object_fields!(@ $self $obj $data { $($field)+ });
            }
        )+
    };
}

macro_rules! impl_view_getters {
    ( @ { $field:ident : &&[$type:ty] $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [<range_ $field>](&self) -> Range<[<I $type>]> {
                let i = self.fields().[<i_ $field>][self.idx as usize];
                let cnt = self.fields().[<cnt_ $field>][self.idx as usize];
                i..(i.offset(cnt))
            }
            pub fn [<cnt_ $field>](&self) -> u32 {
                self.fields().[<cnt_ $field>][self.idx as usize]
            }
            pub fn [<$field _views>](&self) -> ItemCollection::<'model, [<$type View>]<'model>> {
                let range = self.[<range_ $field>]();
                let mut c = ItemCollection::new(self.model, range.start.0, range.end.0);
                c.parent = Some(self.idx);
                c
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : &[$type:ty] $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [<range_ $field>](&self) -> Range<[<I $type>]> {
                let i = self.fields().[<i_ $field>][self.idx as usize];
                let cnt = self.fields().[<cnt_ $field>][self.idx as usize];
                i..(i.offset(cnt))
            }
            pub fn [<cnt_ $field>](&self) -> u32 {
                self.fields().[<cnt_ $field>][self.idx as usize]
            }
            pub fn [<$field _slice>](&self) -> &'model [[<$type Type>]] {
                let i = self.fields().[<i_ $field>][self.idx as usize];
                let cnt = self.fields().[<cnt_ $field>][self.idx as usize];
                self.model.[<$type:snake>].slice(self.[<range_ $field>]())
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : &&$type:ty $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [<i_ $field>](&self) -> [<I $type>] {
                self.fields().[<i_ $field>][self.idx as usize]
            }
            pub fn [<$field _view>](&self) -> [<$type View>]<'model> {
                let idx = self.[<i_ $field>]();
                [<$type View>]::get(self.model, idx).unwrap()
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : &$type:ty $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [<i_ $field>](&self) -> [<I $type>] {
                self.fields().[<i_ $field>][self.idx as usize]
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : Option<&&$type:ty> $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [<i_ $field>](&self) -> <$type as RawObject>::OptIdx {
                self.fields().[<i_ $field>][self.idx as usize]
            }
            pub fn [<$field _view>](&self) -> Option<[<$type View>]<'model>> {
                let idx: Option<_> = self.[<i_ $field>]().into();
                idx.map(|a| {
                    [<$type View>]::get(self.model, a).unwrap()
                })
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : $file_type:ty => $mem_type: ty $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [< f_ $field>](&self) -> &$mem_type {
                &self.fields().$field[self.idx as usize]
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $field:ident : $type:ty $(,$($rest:tt)*)? }) => {
        paste! {
            pub fn [< f_ $field>](&self) -> &$type {
                &self.fields().$field[self.idx as usize]
            }
        }
        impl_view_getters!(@ { $($($rest)*)? });
    };

    ( @ { $(,)? } ) => {};

    ( {
        $($pass:ident {
            $($field:tt)+
        }),+ $(,)?
    }) => {
        impl_view_getters!(@ {
            $($($field)+)+
        });
    };
}

macro_rules! count_object_fields {
    ( @ $count:ident { $field:ident : &&[$type:ty] $(,$($rest:tt)*)? }) => {
        $count = $count + 2;
        count_object_fields!(@ $count { $($($rest)*)? });
    };
    ( @ $count:ident { $field:ident : &[$type:ty] $(,$($rest:tt)*)? }) => {
        $count = $count + 2;
        count_object_fields!(@ $count { $($($rest)*)? });
    };

    ( @ $count:ident { $field:ident : $type:ty $(=> $type2:ty)? $(,$($rest:tt)*)? }) => {
        $count = $count + 1;
        count_object_fields!(@ $count { $($($rest)*)? });
    };

    ( @ $count:ident { $(,)? } ) => {};

    ( $count:ident, $parsing_pass:expr, {
        $($pass:ident {
            $($field:tt)+
        }),+ $(,)?
    }) => {
        $(
            if ($parsing_pass as usize == Pass::$pass as usize) {
                count_object_fields!(@ $count { $($field)+ });
            }
        )+
    };
}

macro_rules! declare_index_types {
    ( $obj:ident ) => {
        declare_index_types!($obj, 1);
    };
    ( $obj:ident, $stride:expr ) => {
        paste! {
            #[derive(Debug, Default, Copy, Clone, PartialEq, Eq, zerocopy_derive::IntoBytes, zerocopy_derive::FromBytes)]
            #[repr(transparent)]
            pub struct [<I $obj>](pub(crate) u32);

            impl Reference for [<I $obj>] {
                fn get(&self) -> u32 {
                    self.0
                }
            }

            impl PrivRef for [<I $obj>] {
                const STRIDE: u32 = $stride;

                fn new(i: u32) -> Self {
                    Self(i)
                }
            }

            transparent_reader!([<I $obj>], u32);

            #[derive(Debug, Default, Copy, Clone, PartialEq, Eq, zerocopy_derive::IntoBytes, zerocopy_derive::FromBytes)]
            #[repr(transparent)]
            pub struct [<OptI $obj>](pub(crate) i32);

            impl OptReference for [<OptI $obj>] {
                fn get(&self) -> Option<u32> {
                    if self.0 == -1 {
                        None
                    } else {
                        Some(self.0 as u32)
                    }
                }
            }

            impl PrivOptRef for [<OptI $obj>] {
                const STRIDE: u32 = $stride;

                fn new(i: Option<u32>) -> Self {
                    Self(match i {
                        Some(i) => i as i32,
                        None => -1
                    })
                }
            }

            transparent_reader!([<OptI $obj>], i32);

            impl From<[<OptI $obj>]> for Option<[<I $obj>]> {
                fn from(value: [<OptI $obj>]) -> Self {
                    value.get().map(|i| [<I $obj>](i))
                }
            }

            impl From<Option<[<I $obj>]>> for [<OptI $obj>] {
                fn from(value: Option<[<I $obj>]>) -> Self {
                    [<OptI $obj>]::new(value.map(|i| i.0))
                }
            }
        }
    };
}

macro_rules! declare_object {
    ( $obj:ident $spec:tt ) => {
        paste! {
            pub struct $obj();
            impl RawObject for $obj {
                type Idx = [<I $obj>];
                type OptIdx = [<OptI $obj>];
            }
            impl Object for $obj {
                type View<'a> = [<$obj View>]<'a>;
            }

            #[derive(Clone, derive_more::Debug)]
            pub struct [<$obj View>]<'model> {
                #[debug(skip)]
                pub(crate) model: &'model Model,
                pub(crate) idx: u32,
                pub(crate) parent: Option<u32>,
            }

            impl<'model> [<$obj View>]<'model> {
                pub(crate) fn idx(&self) -> [<I $obj>] {
                    [<I $obj>]::new(self.idx as u32)
                }
                fn fields(&self) -> &[<$obj Fields>] {
                    &self.model.[< $obj:snake >]
                }
                impl_view_getters!($spec);
            }

            impl<'model> View<'model> for [<$obj View>]<'model> {
                type Object = $obj;

                fn get(model: &'model Model, idx: [<I $obj>]) -> Option<Self> {
                    if model.[< $obj:snake >].count > idx.0 as usize {
                        Some(Self {
                            model,
                            idx: idx.0,
                            parent: None
                        })
                    } else {
                        None
                    }
                }

                fn get_ref(model: &'model Model, idx: [<I $obj>]) -> Option<ViewRef<'model, Self>> {
                    Self::get(model, idx).map(ViewRef::new)
                }

                fn into_ref(self) -> ViewRef<'model, Self> {
                    ViewRef::new(self)
                }

                fn set_parent_idx(&mut self, idx: u32) {
                    self.parent = Some(idx);
                }
            }
        }

        declare_index_types!($obj);
        declare_object_fields!($obj, $spec);

        paste! {
            impl Parsable for [<$obj Fields>] {
                fn parse(&mut self, pass: Pass, data: &mut SectionReader) -> Result<(), ParseError> {
                    parse_object_fields!(self, $obj, pass, data, $spec);

                    Ok(())
                }
            }

            impl [<$obj Fields>] {
                const fn num_fields(pass: Pass) -> usize {
                    let mut count = 0;
                    count_object_fields!(count, pass, $spec);
                    count
                }
            }
        }
    };
}

macro_rules! declare_parent {
    ( $obj:ident, $parent:ident ) => {
        paste! {
            impl<'a> ChildView<'a> for [<$obj View>]<'a> {
                type Parent = $parent;
                fn with_parent(mut self, parent: &<Self::Parent as Object>::View<'a>) -> Self {
                    self.with_parent_idx(parent.idx)
                }
                fn with_parent_idx(mut self, idx: u32) -> Self {
                    self.parent = Some(idx);
                    self
                }
                fn parent(&self) -> <Self::Parent as Object>::View<'a> {
                    <Self::Parent as Object>::View::get(
                        self.model,
                        <Self::Parent as RawObject>::Idx::new(self.parent.unwrap()))
                    .unwrap()
                }
            }
        }
    };
}

macro_rules! declare_primitive {
    ( $obj:ident($type:ty), $pass:ident ) => {
        declare_primitive!($obj($type => $type), $pass);
    };
    ( $obj:ident($file_type:ty => $mem_type:ty ), $pass:ident ) => {
        pub(crate) struct $obj($mem_type);

        paste! {
            impl RawObject for $obj {
                type Idx = [<I $obj>];
                type OptIdx = [<OptI $obj>];
            }

            type [<$obj Type>] = $mem_type;
            const [<$obj:snake:upper _STRIDE>]: usize =
                std::mem::size_of::<$mem_type>() / std::mem::size_of::<$file_type>();

            #[derive(Debug, Default)]
            pub(crate) struct [<$obj Fields>] {
                pub(crate) count: usize,
                pub(crate) values: ArrayType<$mem_type>,
            }
        }

        declare_index_types!($obj, paste! {[<$obj:snake:upper _STRIDE>] as u32});

        paste! {
            impl Parsable for [<$obj Fields>] {
                fn parse(&mut self, pass: Pass, data: &mut SectionReader) -> Result<(), ParseError> {
                    if pass as usize == Pass::$pass as usize {
                        if self.count % [<$obj:snake:upper _STRIDE>] != 0 {
                            return Err(ParseError::UnalignedItemCount(
                                stringify!($obj),
                                self.count,
                                [<$obj:snake:upper _STRIDE>]
                            ));
                        }
                        let count = self.count / [<$obj:snake:upper _STRIDE>];
                        Self::parse_prim(&mut self.values, stringify!($obj), count, data);
                    }
                    Ok(())
                }
            }

            impl [<$obj Fields>] {
                pub(crate) fn slice(&self, range: Range<[<I $obj>]>) -> &[$mem_type] {
                    &self.values[Self::get_range(range)]
                }

                fn get_index(idx: [<I $obj>]) -> usize {
                    let i = idx.0 as usize;
                    if i % [<$obj:snake:upper _STRIDE>] != 0 {
                        panic!("Unaligned index {:?}", idx);
                    }
                    return i / [<$obj:snake:upper _STRIDE>];
                }

                fn get_range(r: Range<[<I $obj>]>) -> Range<usize> {
                    Self::get_index(r.start)..Self::get_index(r.end)
                }

                const fn num_fields(pass: Pass) -> usize {
                    if pass as usize == Pass::$pass as usize {
                        1
                    } else {
                        0
                    }
                }
            }
        }
    };
}

macro_rules! declare_file_fields {
    ( @ $objs:ident { $obj:ident $(,$($rest:tt)*)?
    } -> ($($result:tt)*)) => {
        declare_file_fields!(@ $objs { $($($rest)*)? } -> (
            $($result)*
            pub(crate) [< $obj:snake >] : [< $obj Fields >],
        ));
    };

    ( @ $objs:ident { $(,)? } -> ($($result:tt)*)) => {
        paste!{
            #[derive(Debug, Default)]
            pub struct $objs {
                $($result)*
            }
        }
    };

    ( $objs:ident, {
        Global {
            $($gfield:tt)*
        },
        $($pass:ident {
            $($field:ident),+ $(,)?
        }),+ $(,)?
    }) => {
        declare_file_fields!(@ $objs {
            $($($field),+),+
        } -> ($($gfield)*));
    };
}

macro_rules! for_each_file_class {
    ( $parsing_pass:expr, {
        Global { $($gfield:tt)* },
        $($pass:ident {
            $($obj:ident),+ $(,)?
        }),+ $(,)?
    }, $body:block ) => {
        $(
            // Workaround for non-const PartialOrd
            if ($parsing_pass as usize >= Pass::$pass as usize) {
                $(
                    let _: $obj; // Dummy for macro expansion
                    $body
                )+
            }
        )+
    };

    ( $parsing_pass:expr, $class:ident, {
        Global { $($gfield:tt)* },
        $($pass:ident {
            $($obj:ident),+ $(,)?
        }),+ $(,)?
    }, $body:block ) => {
        $(
            // Workaround for non-const PartialOrd
            if ($parsing_pass as usize >= Pass::$pass as usize) {
                $({
                    type $class = paste! { [< $obj Fields >] };
                    $body
                })+
            }
        )+
    };

    ( $parsing_pass:expr, $self:ident, $var:ident, {
        Global { $($gfield:tt)* },
        $($pass:ident {
            $($obj:ident),+ $(,)?
        }),+ $(,)?
    }, $body:block ) => {
        $(
            // Workaround for non-const PartialOrd
            if ($parsing_pass as usize >= Pass::$pass as usize) {
                $({
                    let $var = &mut paste! { $self.[< $obj:snake >] };
                    $body
                })+
            }
        )+
    };
}

macro_rules! declare_file_objects {
    ( $objs:ident $spec:tt ) => {
        declare_file_fields!($objs, $spec);

        paste! {
            impl $objs {
                pub(crate) const fn num_classes(ver: Version) -> usize {
                    let pass = ver.pass();
                    let mut count = 0;
                    for_each_file_class!(pass, $spec, {count = count + 1;} );
                    count
                }
                pub(crate) const fn num_fields(ver: Version) -> usize {
                    use strum::VariantArray;

                    let mut count = 0;
                    let mut i = 0;
                    while i < Pass::VARIANTS.len() as usize {
                        let pass = Pass::VARIANTS[i];
                        if pass as isize > ver.pass() as isize {
                            break;
                        }
                        for_each_file_class!(pass, Class, $spec, {
                            count = count + Class::num_fields(pass);
                        } );
                        i += 1;
                    }
                    count
                }
                pub(crate) const fn num_sections(ver: Version) -> usize {
                    // Globals & sizes are the first two sections
                    2 + Self::num_fields(ver)
                }
                pub(crate) fn print_classes(ver: Version) {
                    use strum::VariantArray;
                    let mut total = 2;
                    for pass in Pass::VARIANTS {
                        if *pass > ver.pass() {
                            break
                        }
                        println!("### {0:?} ###", pass);
                        for_each_file_class!(*pass, Class, $spec, {
                            let num = Class::num_fields(*pass);
                            if num > 0 {
                                println!("[{0}] {1}: {2}", total, num, std::any::type_name::<Class>());
                            }
                            total += num;
                        });
                    }
                }
                pub(crate) fn load_counts(&mut self, pass: Pass, counts: &[u32]) {
                    let mut idx = 0;
                    for_each_file_class!(pass, self, obj, $spec, {
                        obj.count = counts[idx] as usize;
                        idx += 1;
                    });
                }
                pub(crate) fn parse_objects(&mut self, pass: Pass, data: &mut SectionReader) -> Result<(), ParseError> {
                    for_each_file_class!(pass, self, obj, $spec, {
                        obj.parse(pass, data)?;
                    });
                    Ok(())
                }
            }
        }
    };
}
