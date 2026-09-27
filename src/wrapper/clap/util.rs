use clap_sys::color::clap_color;
use clap_sys::ext::track_info::{
    clap_track_info, CLAP_TRACK_INFO_HAS_TRACK_COLOR, CLAP_TRACK_INFO_HAS_TRACK_NAME,
};
use clap_sys::stream::{clap_istream, clap_ostream};
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::os::raw::c_void;

use crate::prelude::TrackInfo;
use crate::wrapper::util::string_from_c_chars;

/// Early exit out of a function with the specified return value when one of the passed pointers is
/// null.
macro_rules! check_null_ptr {
    ($ret:expr, $ptr:expr $(, $ptrs:expr)* $(, )?) => {
        $crate::wrapper::clap::util::check_null_ptr_msg!("Null pointer passed to function", $ret, $ptr $(, $ptrs)*)
    };
}

/// The same as [`check_null_ptr!`], but with a custom message.
macro_rules! check_null_ptr_msg {
    ($msg:expr, $ret:expr, $ptr:expr $(, $ptrs:expr)* $(, )?) => {
        // Clippy doesn't understand it when we use a unit in our `check_null_ptr!()` macro, even
        // if we explicitly pattern match on that unit
        #[allow(clippy::unused_unit)]
        if $ptr.is_null() $(|| $ptrs.is_null())* {
            nih_debug_assert_failure!($msg);
            return $ret;
        }
    };
}

/// Call a CLAP function. This is needed because even though none of CLAP's functions are allowed to
/// be null pointers, people will still use null pointers for some of the function arguments. This
/// also happens in the official `clap-helpers`. As such, these functions are now `Option<fn(...)>`
/// optional function pointers in `clap-sys`. This macro asserts that the pointer is not null, and
/// prints a nicely formatted error message containing the struct and function name if it is. It
/// also emulates C's syntax for accessing fields struct through a pointer. Except that it uses `=>`
/// instead of `->`. Because that sounds like it would be hilarious.
macro_rules! clap_call {
    { $obj_ptr:expr=>$function_name:ident($($args:expr),* $(, )?) } => {
        match (*$obj_ptr).$function_name {
            Some(function_ptr) => function_ptr($($args),*),
            None => panic!("'{}::{}' is a null pointer, but this is not allowed", $crate::wrapper::clap::util::type_name_of_ptr($obj_ptr), stringify!($function_name)),
        }
    }
}

/// [`clap_call!()`], wrapped in an unsafe block.
macro_rules! unsafe_clap_call {
    { $($args:tt)* } => {
        unsafe { $crate::wrapper::clap::util::clap_call! { $($args)* } }
    }
}

/// Similar to, [`std::any::type_name_of_val()`], but on stable Rust, and stripping away the pointer
/// part.
#[must_use]
pub fn type_name_of_ptr<T: ?Sized>(_ptr: *const T) -> &'static str {
    std::any::type_name::<T>()
}

pub(crate) use check_null_ptr_msg;
pub(crate) use clap_call;
// The wrapper reaches this macro through `#[macro_use]`, so the path import is not used right now
#[allow(unused_imports)]
pub(crate) use unsafe_clap_call;

/// Send+Sync wrapper around CLAP host extension pointers.
pub struct ClapPtr<T> {
    inner: *const T,
}

impl<T> Deref for ClapPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner }
    }
}

unsafe impl<T> Send for ClapPtr<T> {}
unsafe impl<T> Sync for ClapPtr<T> {}

impl<T> ClapPtr<T> {
    /// Create a wrapper around a CLAP object pointer.
    ///
    /// # Safety
    ///
    /// The pointer must point to a valid object with a lifetime that exceeds this object.
    pub unsafe fn new(ptr: *const T) -> Self {
        Self { inner: ptr }
    }
}

/// A buffer a stream can be read into. This is needed to allow reading into uninitialized vectors
/// using slices without invoking UB.
///
/// # Safety
///
/// This may only be implemented by slices of `u8` and types with the same representation as `u8`.
pub unsafe trait ByteReadBuffer {
    /// The length of the slice, in bytes.
    fn len(&self) -> usize;

    /// Get a pointer to the start of the stream.
    fn as_mut_ptr(&mut self) -> *mut u8;
}

unsafe impl ByteReadBuffer for &mut [u8] {
    fn len(&self) -> usize {
        // Bit of a fun one since we reuse the names of the original functions
        <[u8]>::len(self)
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        <[u8]>::as_mut_ptr(self)
    }
}

unsafe impl ByteReadBuffer for &mut [MaybeUninit<u8>] {
    fn len(&self) -> usize {
        <[MaybeUninit<u8>]>::len(self)
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        <[MaybeUninit<u8>]>::as_mut_ptr(self) as *mut u8
    }
}

/// Read from a stream until either the byte slice as been filled, or the stream doesn't contain any
/// data anymore. This correctly handles streams that only allow smaller, buffered reads. If the
/// stream ended before the entire slice has been filled, then this will return `false`.
pub fn read_stream(stream: &clap_istream, mut slice: impl ByteReadBuffer) -> bool {
    let mut read_pos = 0;
    while read_pos < slice.len() {
        let bytes_read = unsafe_clap_call! {
            stream=>read(
                stream,
                slice.as_mut_ptr().add(read_pos) as *mut c_void,
                (slice.len() - read_pos) as u64,
            )
        };
        if bytes_read <= 0 {
            return false;
        }

        read_pos += bytes_read as usize;
    }

    true
}

/// Convert the track information a host wrote through `clap_host_track_info::get()`. Only the
/// fields the host marked as present through the flags are used. The name is read up to its first
/// null character or the end of the fixed size array, so a host that does not terminate it cannot
/// cause an out of bounds read. An empty name counts as no name.
pub fn track_info_from_clap(info: &clap_track_info) -> TrackInfo {
    let name = if info.flags & CLAP_TRACK_INFO_HAS_TRACK_NAME != 0 {
        Some(string_from_c_chars(&info.name)).filter(|name| !name.is_empty())
    } else {
        None
    };

    let color = if info.flags & CLAP_TRACK_INFO_HAS_TRACK_COLOR != 0 {
        let clap_color {
            alpha,
            red,
            green,
            blue,
        } = info.color;

        Some(u32::from_be_bytes([alpha, red, green, blue]))
    } else {
        None
    };

    TrackInfo { name, color }
}

/// Write the data from a slice to a stream until either all data has been written, or the stream
/// returns an error. This correctly handles streams that only allow smaller, buffered writes. This
/// returns `false` if the stream returns an error or doesn't allow any writes anymore.
pub fn write_stream(stream: &clap_ostream, slice: &[u8]) -> bool {
    let mut write_pos = 0;
    while write_pos < slice.len() {
        let bytes_written = unsafe_clap_call! {
            stream=>write(
                stream,
                slice.as_ptr().add(write_pos) as *const c_void,
                (slice.len() - write_pos) as u64,
            )
        };
        if bytes_written <= 0 {
            return false;
        }

        write_pos += bytes_written as usize;
    }

    true
}

#[cfg(test)]
mod tests {
    use clap_sys::ext::track_info::CLAP_TRACK_INFO_HAS_AUDIO_CHANNEL;
    use clap_sys::string_sizes::CLAP_NAME_SIZE;
    use std::os::raw::c_char;

    use super::*;
    use crate::wrapper::util::strlcpy;

    fn empty_info() -> clap_track_info {
        clap_track_info {
            flags: 0,
            name: [0; CLAP_NAME_SIZE],
            color: clap_color {
                alpha: 0,
                red: 0,
                green: 0,
                blue: 0,
            },
            audio_channel_count: 0,
            audio_port_type: std::ptr::null(),
        }
    }

    #[test]
    fn track_info_name_and_color() {
        let mut info = empty_info();
        info.flags = CLAP_TRACK_INFO_HAS_TRACK_NAME | CLAP_TRACK_INFO_HAS_TRACK_COLOR;
        strlcpy(&mut info.name, "Kick Bus");
        info.color = clap_color {
            alpha: 0xFF,
            red: 0x20,
            green: 0x40,
            blue: 0x80,
        };

        assert_eq!(
            track_info_from_clap(&info),
            TrackInfo {
                name: Some(String::from("Kick Bus")),
                color: Some(0xFF20_4080),
            }
        );
    }

    #[test]
    fn track_info_ignores_fields_without_flags() {
        let mut info = empty_info();
        info.flags = CLAP_TRACK_INFO_HAS_AUDIO_CHANNEL;
        strlcpy(&mut info.name, "Not Flagged");
        info.color.red = 0xFF;

        assert_eq!(track_info_from_clap(&info), TrackInfo::default());
    }

    #[test]
    fn track_info_empty_name_is_none() {
        let mut info = empty_info();
        info.flags = CLAP_TRACK_INFO_HAS_TRACK_NAME;

        assert_eq!(track_info_from_clap(&info).name, None);
    }

    #[test]
    fn track_info_unterminated_name_is_cut_off() {
        let mut info = empty_info();
        info.flags = CLAP_TRACK_INFO_HAS_TRACK_NAME;
        info.name = [b'a' as c_char; CLAP_NAME_SIZE];

        assert_eq!(
            track_info_from_clap(&info).name,
            Some("a".repeat(CLAP_NAME_SIZE))
        );
    }

    #[test]
    fn track_info_invalid_utf8_is_replaced() {
        let mut info = empty_info();
        info.flags = CLAP_TRACK_INFO_HAS_TRACK_NAME;
        info.name[0] = b'A' as c_char;
        info.name[1] = 0xFFu8 as c_char;
        info.name[2] = b'B' as c_char;

        assert_eq!(
            track_info_from_clap(&info).name,
            Some(String::from("A\u{FFFD}B"))
        );
    }
}
