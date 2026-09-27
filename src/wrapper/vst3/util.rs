use std::cmp;
use std::mem;
use std::ops::Deref;
use vst3_sys::base::{kResultOk, tchar};
use vst3_sys::interfaces::IUnknown;
use vst3_sys::vst::{kChannelColorKey, kChannelNameKey, IAttributeList, TChar};
use vst3_sys::ComInterface;
use widestring::U16CString;

use crate::prelude::TrackInfo;

/// When `Plugin::MIDI_INPUT` is set to `MidiConfig::MidiCCs` or higher then we'll register 130*16
/// additional parameters to handle MIDI CCs, channel pressure, and pitch bend, in that order.
/// vst3-sys doesn't expose these constants.
pub const VST3_MIDI_CCS: u32 = 130;
pub const VST3_MIDI_CHANNELS: u32 = 16;
/// The number of parameters we'll need to register if the plugin accepts MIDI CCs.
pub const VST3_MIDI_NUM_PARAMS: u32 = VST3_MIDI_CCS * VST3_MIDI_CHANNELS;
/// The start of the MIDI CC parameter ranges. We'll print an assertion failure if any of the
/// plugin's parameters overlap with this range. The mapping to a parameter index is
/// `VST3_MIDI_PARAMS_START + (cc_idx + (channel * VST3_MIDI_CCS))`.
pub const VST3_MIDI_PARAMS_START: u32 = VST3_MIDI_PARAMS_END - VST3_MIDI_NUM_PARAMS;
/// The (exclusive) end of the MIDI CC parameter range. Anything above this is reserved by the host.
pub const VST3_MIDI_PARAMS_END: u32 = 1 << 31;

/// Early exit out of a VST3 function when one of the passed pointers is null
macro_rules! check_null_ptr {
    ($ptr:expr $(, $ptrs:expr)* $(, )?) => {
        check_null_ptr_msg!("Null pointer passed to function", $ptr $(, $ptrs)*)
    };
}

/// The same as [`check_null_ptr!`], but with a custom message.
macro_rules! check_null_ptr_msg {
    ($msg:expr, $ptr:expr $(, $ptrs:expr)* $(, )?) => {
        if $ptr.is_null() $(|| $ptrs.is_null())* {
            nih_debug_assert_failure!($msg);
            return kInvalidArgument;
        }
    };
}

/// The same as [`strlcpy()`], but for VST3's fun UTF-16 strings instead.
pub fn u16strlcpy(dest: &mut [TChar], src: &str) {
    if dest.is_empty() {
        return;
    }

    let src_utf16 = match U16CString::from_str(src) {
        Ok(s) => s,
        Err(err) => {
            nih_debug_assert_failure!("Invalid UTF-16 string: {}", err);
            return;
        }
    };
    let src_utf16_chars = src_utf16.as_slice();
    let src_utf16_chars_signed: &[TChar] =
        unsafe { &*(src_utf16_chars as *const [u16] as *const [TChar]) };

    // Make sure there's always room for a null terminator
    let copy_len = cmp::min(dest.len() - 1, src_utf16_chars_signed.len());
    dest[..copy_len].copy_from_slice(&src_utf16_chars_signed[..copy_len]);
    dest[copy_len] = 0;
}

/// The size in UTF-16 code units of the buffer a track name sent by the host is read into. Longer
/// names are cut off.
const TRACK_NAME_CAPACITY: usize = 1024;

/// Read the track name and color from the attribute list a host passes to
/// `IInfoListener::setChannelContextInfos()`. The host's data is not trusted: the name is read into
/// a fixed size buffer that is never read past its end, whether or not the host terminated the
/// string, and invalid UTF-16 is replaced with U+FFFD. An empty name counts as no name.
///
/// # Safety
///
/// `list` must be a valid attribute list, which is the case for any list passed by the host.
pub unsafe fn track_info_from_attribute_list(list: &impl IAttributeList) -> TrackInfo {
    let mut name_buffer: [tchar; TRACK_NAME_CAPACITY] = [0; TRACK_NAME_CAPACITY];
    let name = if list.get_string(
        kChannelNameKey,
        name_buffer.as_mut_ptr(),
        mem::size_of_val(&name_buffer) as u32,
    ) == kResultOk
    {
        let name_utf16: Vec<u16> = name_buffer
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u16)
            .collect();

        Some(String::from_utf16_lossy(&name_utf16)).filter(|name| !name.is_empty())
    } else {
        None
    };

    let mut color: i64 = 0;
    let color = if list.get_int(kChannelColorKey, &mut color) == kResultOk {
        // The color is a 32-bit `ColorSpec` packed as 0xAARRGGBB. Some hosts sign extend it when
        // storing it as a 64-bit integer, so only the lower 32 bits are meaningful.
        Some(color as u32)
    } else {
        None
    };

    TrackInfo { name, color }
}

/// Send+Sync wrapper for these interface pointers.
#[repr(transparent)]
pub struct VstPtr<T: vst3_sys::ComInterface + ?Sized> {
    ptr: vst3_sys::VstPtr<T>,
}

/// The same as [`VstPtr`] with shared semnatics, but for objects we defined ourself since `VstPtr`
/// only works for interfaces.
#[repr(transparent)]
pub struct ObjectPtr<T: IUnknown> {
    ptr: *const T,
}

impl<T: ComInterface + ?Sized> Deref for VstPtr<T> {
    type Target = vst3_sys::VstPtr<T>;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl<T: IUnknown> Deref for ObjectPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T: vst3_sys::ComInterface + ?Sized> From<vst3_sys::VstPtr<T>> for VstPtr<T> {
    fn from(ptr: vst3_sys::VstPtr<T>) -> Self {
        Self { ptr }
    }
}

impl<T: IUnknown> From<&T> for ObjectPtr<T> {
    /// Create a smart pointer for an existing reference counted object.
    fn from(obj: &T) -> Self {
        unsafe { obj.add_ref() };
        Self { ptr: obj }
    }
}

impl<T: IUnknown> Drop for ObjectPtr<T> {
    fn drop(&mut self) {
        unsafe { (*self).release() };
    }
}

/// SAFETY: Sharing these pointers across thread is s safe as they have internal atomic reference
/// counting, so as long as a `VstPtr<T>` handle exists the object will stay alive.
unsafe impl<T: ComInterface + ?Sized> Send for VstPtr<T> {}
unsafe impl<T: ComInterface + ?Sized> Sync for VstPtr<T> {}

unsafe impl<T: IUnknown> Send for ObjectPtr<T> {}
unsafe impl<T: IUnknown> Sync for ObjectPtr<T> {}

#[cfg(test)]
mod tests {
    use std::ffi::{c_void, CStr};
    use vst3_sys::base::{kResultFalse, tresult};
    use vst3_sys::vst::AttrID;
    use vst3_sys::VST3;

    use super::*;

    // Alias needed for the VST3 attribute macro
    use vst3_sys as vst3_com;

    /// A host side attribute list that only answers the channel name and color keys.
    #[VST3(implements(IAttributeList))]
    struct TestAttributeList {
        /// The name as raw UTF-16 code units, copied as is without adding a null terminator.
        name: Option<Vec<u16>>,
        color: Option<i64>,
    }

    impl TestAttributeList {
        fn create(name: Option<Vec<u16>>, color: Option<i64>) -> Box<Self> {
            Self::allocate(name, color)
        }
    }

    unsafe fn key_is(id: AttrID, key: AttrID) -> bool {
        CStr::from_ptr(id) == CStr::from_ptr(key)
    }

    impl IAttributeList for TestAttributeList {
        unsafe fn set_int(&self, _id: AttrID, _value: i64) -> tresult {
            kResultFalse
        }

        unsafe fn get_int(&self, id: AttrID, value: *mut i64) -> tresult {
            match self.color {
                Some(color) if key_is(id, kChannelColorKey) => {
                    *value = color;
                    kResultOk
                }
                _ => kResultFalse,
            }
        }

        unsafe fn set_float(&self, _id: AttrID, _value: f64) -> tresult {
            kResultFalse
        }

        unsafe fn get_float(&self, _id: AttrID, _value: *mut f64) -> tresult {
            kResultFalse
        }

        unsafe fn set_string(&self, _id: AttrID, _value: *const tchar, _size: u32) -> tresult {
            kResultFalse
        }

        unsafe fn get_string(&self, id: AttrID, value: *mut tchar, size: u32) -> tresult {
            match &self.name {
                Some(name) if key_is(id, kChannelNameKey) => {
                    // Like a careless host, fill the buffer up to the size without terminating it
                    let capacity = size as usize / mem::size_of::<tchar>();
                    let copy_len = cmp::min(capacity, name.len());
                    // SAFETY: The caller provides a buffer of `size` bytes, and at most that many
                    //         bytes are written here
                    let dest = std::slice::from_raw_parts_mut(value, copy_len);
                    for (dest, src) in dest.iter_mut().zip(name) {
                        *dest = *src as tchar;
                    }

                    kResultOk
                }
                _ => kResultFalse,
            }
        }

        unsafe fn set_binary(&self, _id: AttrID, _ptr: *const c_void, _size: u32) -> tresult {
            kResultFalse
        }

        unsafe fn get_binary(
            &self,
            _id: AttrID,
            _ptr: *const *mut c_void,
            _size: *mut u32,
        ) -> tresult {
            kResultFalse
        }
    }

    /// Parse the list the same way the wrapper does: through a COM pointer to the host's object.
    fn parse(list: Box<TestAttributeList>) -> TrackInfo {
        let raw = Box::into_raw(list);
        // SAFETY: `raw` points to a live COM object implementing `IAttributeList` as its first
        //         interface, and `shared()` takes its own reference which is released when `ptr`
        //         is dropped. The last reference is released below.
        unsafe {
            let ptr =
                vst3_sys::VstPtr::<dyn IAttributeList>::shared(raw as *mut _).expect("Null list");
            let info = track_info_from_attribute_list(&ptr);
            drop(ptr);
            (*raw).release();

            info
        }
    }

    #[test]
    fn track_info_name_and_color() {
        let name: Vec<u16> = "Kick Bus".encode_utf16().chain([0]).collect();
        let info = parse(TestAttributeList::create(Some(name), Some(0xFF20_4080)));

        assert_eq!(
            info,
            TrackInfo {
                name: Some(String::from("Kick Bus")),
                color: Some(0xFF20_4080),
            }
        );
    }

    #[test]
    fn track_info_sign_extended_color() {
        // A host storing the ColorSpec as a signed 32-bit integer before widening it
        let color = 0xFF20_4080u32 as i32 as i64;
        let info = parse(TestAttributeList::create(None, Some(color)));

        assert_eq!(info.name, None);
        assert_eq!(info.color, Some(0xFF20_4080));
    }

    #[test]
    fn track_info_empty_list() {
        let info = parse(TestAttributeList::create(None, None));

        assert_eq!(info, TrackInfo::default());
    }

    #[test]
    fn track_info_empty_name_is_none() {
        let info = parse(TestAttributeList::create(Some(vec![0]), None));

        assert_eq!(info.name, None);
    }

    #[test]
    fn track_info_unterminated_name_is_cut_off() {
        let name = vec![b'a' as u16; TRACK_NAME_CAPACITY * 2];
        let info = parse(TestAttributeList::create(Some(name), None));

        assert_eq!(info.name, Some("a".repeat(TRACK_NAME_CAPACITY)));
    }

    #[test]
    fn track_info_invalid_utf16_is_replaced() {
        // A lone high surrogate
        let name = vec![b'A' as u16, 0xD800, b'B' as u16, 0];
        let info = parse(TestAttributeList::create(Some(name), None));

        assert_eq!(info.name, Some(String::from("A\u{FFFD}B")));
    }
}

#[cfg(test)]
mod miri {
    use widestring::U16CStr;

    use super::*;

    #[test]
    fn u16strlcpy_normal() {
        let mut dest = [0; 256];
        u16strlcpy(&mut dest, "Hello, world!");

        assert_eq!(
            unsafe { U16CStr::from_ptr_str(dest.as_ptr() as *const u16) }
                .to_string()
                .unwrap(),
            "Hello, world!"
        );
    }

    #[test]
    fn u16strlcpy_overflow() {
        let mut dest = [0; 6];
        u16strlcpy(&mut dest, "Hello, world!");

        assert_eq!(
            unsafe { U16CStr::from_ptr_str(dest.as_ptr() as *const u16) }
                .to_string()
                .unwrap(),
            "Hello"
        );
    }
}
