use std::mem;
use std::ptr;
use std::slice;

use crate::Error;
use crate::Result;

/// Types that can be safely converted to and from raw byte
/// representations.
///
/// `Pod` stands for "plain old data". Types implementing this trait
/// can be freely reinterpreted as byte slices and vice versa, which
/// is useful for interacting with BPF maps and ring buffers that
/// transfer data as raw bytes.
///
/// # Safety
///
/// The implementing type must satisfy **all** of the following:
/// - Is `#[repr(C)]`, `#[repr(transparent)]`, or a primitive integer type.
/// - Is valid for any arbitrary bit pattern of the correct size (no validity invariants beyond "N
///   bytes of initialized memory").
/// - Has no padding bytes (all bytes are part of a field or explicit `[u8; N]` padding).
/// - Is `Copy` (implied by the supertrait bound).
pub unsafe trait Pod: Copy + 'static {
    /// View this value as a byte slice.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: Caller guarantees the type has no padding and is
        //         `repr(C)` or a primitive.
        unsafe { slice::from_raw_parts((self as *const Self).cast::<u8>(), mem::size_of::<Self>()) }
    }

    /// Create a reference to `Self` from a byte slice (zero-copy).
    ///
    /// Returns an error if `bytes` is too small or not properly
    /// aligned for `Self`.
    fn from_bytes(bytes: &[u8]) -> Result<&Self>
    where
        Self: Sized,
    {
        if bytes.len() < mem::size_of::<Self>() {
            return Err(Error::with_invalid_data(format!(
                "buffer size {} < type size {}",
                bytes.len(),
                mem::size_of::<Self>(),
            )));
        }
        if bytes.as_ptr() as usize % mem::align_of::<Self>() != 0 {
            return Err(Error::with_invalid_data(format!(
                "buffer at {:p} is not aligned to {}",
                bytes.as_ptr(),
                mem::align_of::<Self>(),
            )));
        }
        // SAFETY: Size and alignment checked above. Caller guarantees
        //         all bit patterns are valid.
        Ok(unsafe { &*bytes.as_ptr().cast::<Self>() })
    }

    /// Create a mutable reference to `Self` from a mutable byte
    /// slice (zero-copy).
    ///
    /// Returns an error if `bytes` is too small or not properly
    /// aligned for `Self`.
    fn from_bytes_mut(bytes: &mut [u8]) -> Result<&mut Self>
    where
        Self: Sized,
    {
        if bytes.len() < mem::size_of::<Self>() {
            return Err(Error::with_invalid_data(format!(
                "buffer size {} < type size {}",
                bytes.len(),
                mem::size_of::<Self>(),
            )));
        }
        if bytes.as_ptr() as usize % mem::align_of::<Self>() != 0 {
            return Err(Error::with_invalid_data(format!(
                "buffer at {:p} is not aligned to {}",
                bytes.as_ptr(),
                mem::align_of::<Self>(),
            )));
        }
        // SAFETY: Size and alignment checked above. Caller guarantees
        //         all bit patterns are valid. Mutable reference
        //         guarantees exclusivity.
        Ok(unsafe { &mut *bytes.as_mut_ptr().cast::<Self>() })
    }

    /// Copy `Self` out of a byte slice.
    ///
    /// Unlike [`from_bytes`](Pod::from_bytes), this has no alignment
    /// requirement since it copies the bytes rather than aliasing
    /// them. Returns an error if `bytes` is too small.
    fn copy_from_bytes(bytes: &[u8]) -> Result<Self>
    where
        Self: Sized,
    {
        if bytes.len() < mem::size_of::<Self>() {
            return Err(Error::with_invalid_data(format!(
                "buffer size {} < type size {}",
                bytes.len(),
                mem::size_of::<Self>(),
            )));
        }
        let mut val = mem::MaybeUninit::<Self>::uninit();
        // SAFETY: Size checked above. `copy_nonoverlapping` handles
        //         unaligned source. Caller guarantees all bit patterns
        //         are valid.
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                val.as_mut_ptr().cast::<u8>(),
                mem::size_of::<Self>(),
            );
            Ok(val.assume_init())
        }
    }

    /// Reinterpret a byte slice as a slice of `Self` (zero-copy).
    ///
    /// Returns an error if `bytes` is not properly aligned for `Self`
    /// or its length is not a multiple of `size_of::<Self>()`.
    fn slice_from_bytes(bytes: &[u8]) -> Result<&[Self]>
    where
        Self: Sized,
    {
        let size = mem::size_of::<Self>();
        if size == 0 {
            return Err(Error::with_invalid_data(
                "slice_from_bytes cannot be used with zero-sized types",
            ));
        }
        if bytes.len() % size != 0 {
            return Err(Error::with_invalid_data(format!(
                "buffer size {} is not a multiple of type size {}",
                bytes.len(),
                size,
            )));
        }
        if bytes.as_ptr() as usize % mem::align_of::<Self>() != 0 {
            return Err(Error::with_invalid_data(format!(
                "buffer at {:p} is not aligned to {}",
                bytes.as_ptr(),
                mem::align_of::<Self>(),
            )));
        }
        let count = bytes.len() / size;
        // SAFETY: Size and alignment checked above. Caller guarantees
        //         all bit patterns are valid.
        Ok(unsafe { slice::from_raw_parts(bytes.as_ptr().cast::<Self>(), count) })
    }
}

// All primitive integer types are valid for any bit pattern, have no
// padding, and are `Copy`.
unsafe impl Pod for u8 {}
unsafe impl Pod for u16 {}
unsafe impl Pod for u32 {}
unsafe impl Pod for u64 {}
unsafe impl Pod for u128 {}
unsafe impl Pod for i8 {}
unsafe impl Pod for i16 {}
unsafe impl Pod for i32 {}
unsafe impl Pod for i64 {}
unsafe impl Pod for i128 {}

// Fixed-size arrays of `Pod` types are `Pod`.
unsafe impl<T: Pod, const N: usize> Pod for [T; N] {}


#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that a primitive can be serialized to bytes and deserialized back.
    #[test]
    fn primitive_round_trip() {
        let val: u32 = 0xDEAD_BEEF;
        let bytes = val.as_bytes();
        assert_eq!(bytes.len(), 4);
        let recovered = u32::copy_from_bytes(bytes).unwrap();
        assert_eq!(recovered, val);
    }

    /// Verify zero-copy reinterpretation of an aligned byte buffer.
    #[test]
    fn from_bytes_zero_copy() {
        let bytes = 42u64.to_ne_bytes();
        let r = u64::from_bytes(&bytes).unwrap();
        assert_eq!(*r, 42);
    }

    /// Verify that `from_bytes` rejects a buffer smaller than the type.
    #[test]
    fn from_bytes_too_small() {
        let bytes = [0u8; 2];
        assert!(u32::from_bytes(&bytes).is_err());
    }

    /// Verify that `from_bytes` rejects a misaligned buffer.
    #[test]
    fn from_bytes_misaligned() {
        let bytes = [0u8; 8];
        // Offset by 1 to misalign for u32.
        let result = u32::from_bytes(&bytes[1..5]);
        assert!(result.is_err());
    }

    /// Verify that `copy_from_bytes` succeeds on misaligned input since it copies.
    #[test]
    fn copy_from_bytes_unaligned_ok() {
        let bytes = [0u8; 8];
        let val = u32::copy_from_bytes(&bytes[1..5]).unwrap();
        assert_eq!(val, 0);
    }

    /// Verify that fixed-size arrays of Pod types work as Pod.
    #[test]
    fn array_pod() {
        let arr: [u32; 3] = [1, 2, 3];
        let bytes = arr.as_bytes();
        assert_eq!(bytes.len(), 12);
        let recovered = <[u32; 3]>::copy_from_bytes(bytes).unwrap();
        assert_eq!(recovered, arr);
    }

    /// Verify that writes through `from_bytes_mut` are visible in the underlying buffer.
    #[test]
    fn from_bytes_mut_write_through() {
        let mut bytes = 0u32.to_ne_bytes();
        let r = u32::from_bytes_mut(&mut bytes).unwrap();
        *r = 42;
        assert_eq!(u32::from_ne_bytes(bytes), 42);
    }

    /// Verify that a user-defined `#[repr(C)]` struct works with Pod.
    #[test]
    fn repr_c_struct() {
        #[repr(C)]
        #[derive(Copy, Clone, Debug, PartialEq)]
        struct Pair {
            a: u32,
            b: u32,
        }
        unsafe impl Pod for Pair {}

        let p = Pair { a: 1, b: 2 };
        let bytes = p.as_bytes();
        assert_eq!(bytes.len(), 8);
        let recovered = Pair::copy_from_bytes(bytes).unwrap();
        assert_eq!(recovered, p);
    }

    /// Verify that `slice_from_bytes` reinterprets a byte buffer as a typed slice.
    #[test]
    fn slice_from_bytes_round_trip() {
        let vals: [u32; 3] = [10, 20, 30];
        let bytes = vals.as_bytes();
        let slice = u32::slice_from_bytes(bytes).unwrap();
        assert_eq!(slice, &[10, 20, 30]);
    }

    /// Verify that `slice_from_bytes` rejects a buffer whose size is not a multiple of the type.
    #[test]
    fn slice_from_bytes_bad_length() {
        let bytes = [0u8; 7];
        assert!(u32::slice_from_bytes(&bytes).is_err());
    }
}
