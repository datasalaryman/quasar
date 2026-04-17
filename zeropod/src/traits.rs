use crate::error::ZeroPodError;
use crate::pod::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Fixed,
    Compact,
}

/// Validation trait for stored (pod) types.
/// Each pod type knows how to validate itself.
pub trait ZcValidate: Copy {
    /// Validate that this value's bytes represent a valid state.
    fn validate_ref(value: &Self) -> Result<(), ZeroPodError>;
}

// --- ZcValidate: trivially valid types (all bit patterns valid) ---

impl ZcValidate for u8 {
    #[inline(always)]
    fn validate_ref(_: &Self) -> Result<(), ZeroPodError> { Ok(()) }
}

impl ZcValidate for i8 {
    #[inline(always)]
    fn validate_ref(_: &Self) -> Result<(), ZeroPodError> { Ok(()) }
}

macro_rules! impl_zc_validate_trivial {
    ($($ty:ty),*) => {
        $(
            impl ZcValidate for $ty {
                #[inline(always)]
                fn validate_ref(_: &Self) -> Result<(), ZeroPodError> { Ok(()) }
            }
        )*
    };
}

impl_zc_validate_trivial!(PodU16, PodU32, PodU64, PodU128, PodI16, PodI32, PodI64, PodI128);

impl<const N: usize> ZcValidate for [u8; N] {
    #[inline(always)]
    fn validate_ref(_: &Self) -> Result<(), ZeroPodError> { Ok(()) }
}

// --- ZcValidate: PodBool (byte must be 0 or 1) ---

impl ZcValidate for PodBool {
    #[inline(always)]
    fn validate_ref(value: &Self) -> Result<(), ZeroPodError> {
        let byte = unsafe { *(value as *const PodBool as *const u8) };
        if byte > 1 { Err(ZeroPodError::InvalidBool) } else { Ok(()) }
    }
}

// --- ZcValidate: PodString (len <= N, active bytes valid UTF-8) ---

impl<const N: usize, const PFX: usize> ZcValidate for PodString<N, PFX> {
    #[inline(always)]
    fn validate_ref(value: &Self) -> Result<(), ZeroPodError> {
        if value.decode_len() > N { return Err(ZeroPodError::InvalidLength); }
        if core::str::from_utf8(value.as_bytes()).is_err() {
            return Err(ZeroPodError::InvalidUtf8);
        }
        Ok(())
    }
}

// --- ZcValidate: PodVec (len <= N) ---

impl<T: Copy + ZcValidate, const N: usize, const PFX: usize> ZcValidate for PodVec<T, N, PFX> {
    #[inline(always)]
    fn validate_ref(value: &Self) -> Result<(), ZeroPodError> {
        if value.decode_len() > N { return Err(ZeroPodError::InvalidLength); }
        for item in value.as_slice() {
            T::validate_ref(item)?;
        }
        Ok(())
    }
}

// --- ZcValidate: PodOption (tag 0 or 1, inner valid if Some) ---

impl<T: Copy + ZcValidate> ZcValidate for PodOption<T> {
    #[inline(always)]
    fn validate_ref(value: &Self) -> Result<(), ZeroPodError> {
        match value.raw_tag() {
            0 => Ok(()),
            1 => {
                let inner = unsafe { value.assume_init_ref() };
                T::validate_ref(inner)
            }
            _ => Err(ZeroPodError::InvalidTag),
        }
    }
}

pub trait ZeroPodSchema: Sized {
    const LAYOUT: LayoutKind;
}

pub trait ZeroPodFixed: ZeroPodSchema {
    type Zc: Copy;
    const SIZE: usize;
    fn from_bytes(data: &[u8]) -> Result<&Self::Zc, ZeroPodError>;
    fn from_bytes_mut(data: &mut [u8]) -> Result<&mut Self::Zc, ZeroPodError>;
    fn validate(data: &[u8]) -> Result<(), ZeroPodError>;
    /// # Safety
    /// Caller must ensure `data` is at least `Self::SIZE` bytes and contains valid content.
    unsafe fn from_bytes_unchecked(data: &[u8]) -> &Self::Zc {
        &*(data.as_ptr() as *const Self::Zc)
    }
    /// # Safety
    /// Caller must ensure `data` is at least `Self::SIZE` bytes and contains valid content.
    unsafe fn from_bytes_mut_unchecked(data: &mut [u8]) -> &mut Self::Zc {
        &mut *(data.as_mut_ptr() as *mut Self::Zc)
    }
}

pub trait ZeroPodCompact: ZeroPodSchema {
    type Header: Copy;
    const HEADER_SIZE: usize;
    fn header(data: &[u8]) -> Result<&Self::Header, ZeroPodError>;
    fn header_mut(data: &mut [u8]) -> Result<&mut Self::Header, ZeroPodError>;
    fn validate(data: &[u8]) -> Result<(), ZeroPodError>;
}

pub trait ZcField: Sized {
    type Pod: Copy;
    const POD_SIZE: usize;
}

// Built-in ZcField impls
macro_rules! impl_zc_field {
    ($native:ty, $pod:ty, $size:expr) => {
        impl ZcField for $native {
            type Pod = $pod;
            const POD_SIZE: usize = $size;
        }
    };
}

impl_zc_field!(u8, u8, 1);
impl_zc_field!(u16, PodU16, 2);
impl_zc_field!(u32, PodU32, 4);
impl_zc_field!(u64, PodU64, 8);
impl_zc_field!(u128, PodU128, 16);
impl_zc_field!(i8, i8, 1);
impl_zc_field!(i16, PodI16, 2);
impl_zc_field!(i32, PodI32, 4);
impl_zc_field!(i64, PodI64, 8);
impl_zc_field!(i128, PodI128, 16);
impl_zc_field!(bool, PodBool, 1);
