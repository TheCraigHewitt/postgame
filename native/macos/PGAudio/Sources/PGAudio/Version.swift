import Foundation

private let version = strdup("0.1.0")!

/// The shim's version as a NUL-terminated UTF-8 string. The pointer stays valid
/// for the life of the process; callers must not free it.
@_cdecl("pg_audio_version")
public func pgAudioVersion() -> UnsafePointer<CChar> {
    UnsafePointer(version)
}
