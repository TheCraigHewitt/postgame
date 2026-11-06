//! Safe wrappers over the PGAudio C ABI.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use crate::CaptureError;
use crate::target::AudioProcess;
use crate::track::{Buffer, downmix};

type AudioCallback = unsafe extern "C" fn(*mut c_void, *const *const f32, u32, u32, u32, u64, f64);

unsafe extern "C" {
    fn pg_audio_version() -> *const c_char;
    fn pg_audio_free_string(string: *mut c_char);
    fn pg_audio_processes_json() -> *mut c_char;
    fn pg_audio_own_process_object() -> u32;
    fn pg_audio_system_start(
        processes: *const u32,
        count: isize,
        exclude: bool,
        callback: AudioCallback,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn pg_audio_system_stop(handle: *mut c_void);
    fn pg_audio_mic_start(
        callback: AudioCallback,
        context: *mut c_void,
        error: *mut *mut c_char,
    ) -> *mut c_void;
    fn pg_audio_mic_stop(handle: *mut c_void);
}

/// The version reported by the Swift shim.
pub fn shim_version() -> String {
    // SAFETY: the shim returns a static NUL-terminated string that is never freed.
    unsafe { CStr::from_ptr(pg_audio_version()) }
        .to_string_lossy()
        .into_owned()
}

/// Takes ownership of a string allocated by the shim.
unsafe fn take_string(raw: *mut c_char) -> String {
    // SAFETY: the caller passes a live NUL-terminated string from the shim.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `raw` came from the shim and is not used again.
    unsafe { pg_audio_free_string(raw) };
    text
}

/// The processes Core Audio currently knows about.
pub fn audio_processes() -> Vec<AudioProcess> {
    // SAFETY: the call takes no arguments and returns an owned string or null.
    let raw = unsafe { pg_audio_processes_json() };
    if raw.is_null() {
        return Vec::new();
    }
    // SAFETY: `raw` is non-null and owned by us.
    let json = unsafe { take_string(raw) };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Our own Core Audio process object, if we have one yet.
pub fn own_process_object() -> Option<u32> {
    // SAFETY: the call takes no arguments.
    match unsafe { pg_audio_own_process_object() } {
        0 => None,
        object => Some(object),
    }
}

/// Where a callback sends audio. Each sink is leaked: Core Audio may still be
/// finishing a callback when stop returns, so its memory must stay valid.
/// `close` drops the sender so that the writer thread can end.
struct Sink {
    sender: Mutex<Option<Sender<Buffer>>>,
    on_extra_buffers: Box<dyn Fn(u32) + Send + Sync>,
    warned: AtomicBool,
}

impl Sink {
    fn leak(
        sender: Sender<Buffer>,
        on_extra_buffers: Box<dyn Fn(u32) + Send + Sync>,
    ) -> &'static Self {
        Box::leak(Box::new(Self {
            sender: Mutex::new(Some(sender)),
            on_extra_buffers,
            warned: AtomicBool::new(false),
        }))
    }

    fn close(&self) {
        self.sender.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn context(&'static self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

unsafe extern "C" fn on_audio(
    context: *mut c_void,
    buffers: *const *const f32,
    buffer_count: u32,
    channels_per_buffer: u32,
    frames: u32,
    host_time_ns: u64,
    sample_rate: f64,
) {
    // SAFETY: `context` is a leaked `Sink`, so it is valid for any lifetime.
    let sink = unsafe { &*context.cast::<Sink>() };
    let samples_per_buffer = (frames * channels_per_buffer) as usize;
    // SAFETY: the shim passes `buffer_count` pointers, valid during this call.
    let pointers = unsafe { std::slice::from_raw_parts(buffers, buffer_count as usize) };
    let slices: Vec<&[f32]> = pointers
        .iter()
        .filter(|p| !p.is_null())
        // SAFETY: each buffer holds `frames * channels_per_buffer` samples.
        .map(|p| unsafe { std::slice::from_raw_parts(*p, samples_per_buffer) })
        .collect();

    if buffer_count > 1 && !sink.warned.swap(true, Ordering::Relaxed) {
        (sink.on_extra_buffers)(buffer_count);
    }
    let buffer = Buffer {
        samples: downmix(&slices, channels_per_buffer as usize, frames as usize),
        host_time_ns,
        sample_rate,
    };
    if let Some(sender) = &*sink.sender.lock().unwrap_or_else(|e| e.into_inner()) {
        // A send only fails once the writer is gone, during shutdown.
        let _ = sender.send(buffer);
    }
}

/// Reads the error the shim wrote after a failed start.
fn start_error(error: *mut c_char) -> CaptureError {
    if error.is_null() {
        return CaptureError::Start("unknown failure".into());
    }
    // SAFETY: a failed start leaves an owned message in `error`.
    CaptureError::Start(unsafe { take_string(error) })
}

/// A running process tap. Stops when dropped.
pub struct SystemTap {
    handle: *mut c_void,
    sink: &'static Sink,
}

// SAFETY: the handle is an opaque retained Swift object, used only to stop it.
unsafe impl Send for SystemTap {}

impl SystemTap {
    /// Taps `processes`, or everything except them when `exclude` is true.
    pub fn start(
        processes: &[u32],
        exclude: bool,
        sender: Sender<Buffer>,
        on_extra_buffers: Box<dyn Fn(u32) + Send + Sync>,
    ) -> Result<Self, CaptureError> {
        let sink = Sink::leak(sender, on_extra_buffers);
        let mut error = ptr::null_mut();
        // SAFETY: the pointers are valid for the call, and `sink` is leaked.
        let handle = unsafe {
            pg_audio_system_start(
                processes.as_ptr(),
                processes.len() as isize,
                exclude,
                on_audio,
                sink.context(),
                &mut error,
            )
        };
        if handle.is_null() {
            sink.close();
            return Err(start_error(error));
        }
        Ok(Self { handle, sink })
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful start and is released once.
        unsafe { pg_audio_system_stop(self.handle) };
        self.sink.close();
    }
}

/// A running mic capture. Stops when dropped.
pub struct Mic {
    handle: *mut c_void,
    sink: &'static Sink,
}

// SAFETY: the handle is an opaque retained Swift object, used only to stop it.
unsafe impl Send for Mic {}

impl Mic {
    pub fn start(sender: Sender<Buffer>) -> Result<Self, CaptureError> {
        // A mic with several channels arrives as planar buffers, which is normal.
        let sink = Sink::leak(sender, Box::new(|_| {}));
        let mut error = ptr::null_mut();
        // SAFETY: `error` outlives the call, and `sink` is leaked.
        let handle = unsafe { pg_audio_mic_start(on_audio, sink.context(), &mut error) };
        if handle.is_null() {
            sink.close();
            return Err(start_error(error));
        }
        Ok(Self { handle, sink })
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful start and is released once.
        unsafe { pg_audio_mic_stop(self.handle) };
        self.sink.close();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;

    #[test]
    fn swift_shim_is_linked() {
        assert_eq!(shim_version(), "0.1.0");
    }

    #[test]
    fn process_list_comes_back_as_valid_json() {
        // CI runners may have no audio processes, so only the call is checked.
        let _ = audio_processes();
    }

    #[test]
    fn a_callback_downmixes_and_forwards_audio() {
        let (sender, receiver) = channel();
        let sink = Sink::leak(sender, Box::new(|_| {}));
        let left = [1.0_f32, 0.0];
        let right = [0.0_f32, 0.0];
        let pointers = [left.as_ptr(), right.as_ptr()];
        // SAFETY: the pointers match the counts given.
        unsafe { on_audio(sink.context(), pointers.as_ptr(), 2, 1, 2, 42, 48_000.0) };
        let buffer = receiver.try_recv().unwrap();
        assert_eq!(buffer.samples, vec![0.5, 0.0]);
        assert_eq!(buffer.host_time_ns, 42);
    }

    #[test]
    fn a_closed_sink_drops_audio_quietly_and_ends_the_channel() {
        let (sender, receiver) = channel();
        let sink = Sink::leak(sender, Box::new(|_| {}));
        sink.close();
        let samples = [0.5_f32];
        let pointers = [samples.as_ptr()];
        // SAFETY: the pointers match the counts given.
        unsafe { on_audio(sink.context(), pointers.as_ptr(), 1, 1, 1, 0, 48_000.0) };
        assert!(receiver.recv().is_err());
    }
}
