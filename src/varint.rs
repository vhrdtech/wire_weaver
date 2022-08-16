use core::marker::PhantomData;

#[derive(Copy, Clone, Debug)]
pub struct VarInt<F> {
    _phantom: PhantomData<F>,
}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
pub struct vlu4;