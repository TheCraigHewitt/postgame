use std::ffi::{CStr, c_char};

unsafe extern "C" {
    fn pg_audio_version() -> *const c_char;
}

/// The version reported by the Swift shim.
pub fn shim_version() -> String {
    // SAFETY: the shim returns a static NUL-terminated string that is never freed.
    unsafe { CStr::from_ptr(pg_audio_version()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn swift_shim_is_linked() {
        assert_eq!(super::shim_version(), "0.1.0");
    }
}
