import CoreAudio
import Foundation

/// Delivers audio to the host. `buffers` holds `bufferCount` pointers, each to
/// `channelsPerBuffer` interleaved Float32 channels of `frames` frames.
/// `hostTimeNs` is the host time of the first frame. Pointers are valid only
/// during the call.
public typealias PGAudioCallback = @convention(c) (
    _ context: UnsafeMutableRawPointer?,
    _ buffers: UnsafePointer<UnsafePointer<Float>?>,
    _ bufferCount: UInt32,
    _ channelsPerBuffer: UInt32,
    _ frames: UInt32,
    _ hostTimeNs: UInt64,
    _ sampleRate: Double
) -> Void

struct CaptureFailure: Error {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    init(_ action: String, status: OSStatus) {
        message = "\(action) failed (OSStatus \(status))"
    }
}

/// Runs `start`, returning its retained result as an opaque handle, or null
/// with a message in `errorOut` that the caller frees with
/// `pg_audio_free_string`.
func startHandle<T: AnyObject>(
    _ errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>,
    _ start: () throws -> T
) -> UnsafeMutableRawPointer? {
    do {
        return Unmanaged.passRetained(try start()).toOpaque()
    } catch {
        errorOut.pointee = strdup((error as? CaptureFailure)?.message ?? "\(error)")
        return nil
    }
}

@_cdecl("pg_audio_free_string")
public func pgAudioFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
