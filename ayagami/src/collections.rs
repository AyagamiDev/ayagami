use crate::core::uid_type::{Sequential, Sparse};

mod private {
    pub trait UidType {
        type Collection<D, S>;
        fn do_new<D, S>(
            sequential: impl FnOnce() -> D,
            sparse: impl FnOnce() -> S,
        ) -> Self::Collection<D, S>;
        fn do_visit<'a, D: 'a, S: 'a, R>(
            collection: &'a Self::Collection<D, S>,
            sequential: impl FnOnce(&'a D) -> R,
            sparse: impl FnOnce(&'a S) -> R,
        ) -> R;
        fn do_visit_mut<'a, D: 'a, S: 'a, R>(
            collection: &'a mut Self::Collection<D, S>,
            sequential: impl FnOnce(&'a mut D) -> R,
            sparse: impl FnOnce(&'a mut S) -> R,
        ) -> R;
        fn do_put_mut<'a, D: 'a, S: 'a, T, R>(
            collection: &'a mut Self::Collection<D, S>,
            sequential: impl FnOnce(&'a mut D, T) -> R,
            sparse: impl FnOnce(&'a mut S, T) -> R,
            args: T,
        ) -> R;
    }
}

pub(crate) use private::UidType;

pub struct UidCollection<K: UidType, D, S>(K::Collection<D, S>);
impl<K: UidType, D, S> UidCollection<K, D, S> {
    pub fn new(sequential: impl FnOnce() -> D, sparse: impl FnOnce() -> S) -> Self {
        Self(K::do_new(sequential, sparse))
    }
    pub fn visit<'a, R>(
        &'a self,
        sequential: impl FnOnce(&'a D) -> R,
        sparse: impl FnOnce(&'a S) -> R,
    ) -> R {
        K::do_visit(&self.0, sequential, sparse)
    }
    pub fn visit_mut<'a, R>(
        &'a mut self,
        sequential: impl FnOnce(&'a mut D) -> R,
        sparse: impl FnOnce(&'a mut S) -> R,
    ) -> R {
        K::do_visit_mut(&mut self.0, sequential, sparse)
    }
    pub fn put_mut<'a, T, R>(
        &'a mut self,
        sequential: impl FnOnce(&'a mut D, T) -> R,
        sparse: impl FnOnce(&'a mut S, T) -> R,
        args: T,
    ) -> R {
        K::do_put_mut(&mut self.0, sequential, sparse, args)
    }
}

impl UidType for Sequential {
    type Collection<D, S> = D;
    fn do_new<D, S>(
        sequential: impl FnOnce() -> D,
        sparse: impl FnOnce() -> S,
    ) -> Self::Collection<D, S> {
        sequential()
    }
    fn do_visit<'a, D: 'a, S: 'a, R>(
        collection: &'a Self::Collection<D, S>,
        sequential: impl FnOnce(&'a D) -> R,
        sparse: impl FnOnce(&'a S) -> R,
    ) -> R {
        sequential(collection)
    }
    fn do_visit_mut<'a, D: 'a, S: 'a, R>(
        collection: &'a mut Self::Collection<D, S>,
        sequential: impl FnOnce(&'a mut D) -> R,
        sparse: impl FnOnce(&'a mut S) -> R,
    ) -> R {
        sequential(collection)
    }
    fn do_put_mut<'a, D: 'a, S: 'a, T, R>(
        collection: &'a mut Self::Collection<D, S>,
        sequential: impl FnOnce(&'a mut D, T) -> R,
        sparse: impl FnOnce(&'a mut S, T) -> R,
        args: T,
    ) -> R {
        sequential(collection, args)
    }
}

impl UidType for Sparse {
    type Collection<D, S> = S;
    fn do_new<D, S>(
        sequential: impl FnOnce() -> D,
        sparse: impl FnOnce() -> S,
    ) -> Self::Collection<D, S> {
        sparse()
    }
    fn do_visit<'a, D: 'a, S: 'a, R>(
        collection: &'a Self::Collection<D, S>,
        sequential: impl FnOnce(&'a D) -> R,
        sparse: impl FnOnce(&'a S) -> R,
    ) -> R {
        sparse(collection)
    }
    fn do_visit_mut<'a, D: 'a, S: 'a, R>(
        collection: &'a mut Self::Collection<D, S>,
        sequential: impl FnOnce(&'a mut D) -> R,
        sparse: impl FnOnce(&'a mut S) -> R,
    ) -> R {
        sparse(collection)
    }
    fn do_put_mut<'a, D: 'a, S: 'a, T, R>(
        collection: &'a mut Self::Collection<D, S>,
        sequential: impl FnOnce(&'a mut D, T) -> R,
        sparse: impl FnOnce(&'a mut S, T) -> R,
        args: T,
    ) -> R {
        sparse(collection, args)
    }
}
