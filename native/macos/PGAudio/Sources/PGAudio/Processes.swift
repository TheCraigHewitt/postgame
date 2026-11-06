import CoreAudio
import Foundation

private struct AudioProcess: Encodable {
    let object_id: UInt32
    let pid: Int32
    let bundle_id: String
    let running_output: Bool
}

private func address(_ selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
}

func readValue<T>(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector, _ initial: T) -> T? {
    var property = address(selector)
    var size = UInt32(MemoryLayout<T>.size)
    var value = initial
    let status = withUnsafeMutablePointer(to: &value) {
        AudioObjectGetPropertyData(object, &property, 0, nil, &size, $0)
    }
    return status == noErr ? value : nil
}

func readString(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
    var property = address(selector)
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    var value: Unmanaged<CFString>?
    let status = AudioObjectGetPropertyData(object, &property, 0, nil, &size, &value)
    guard status == noErr else { return nil }
    return value?.takeRetainedValue() as String?
}

private func processObjects() -> [AudioObjectID] {
    let system = AudioObjectID(kAudioObjectSystemObject)
    var property = address(kAudioHardwarePropertyProcessObjectList)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(system, &property, 0, nil, &size) == noErr else {
        return []
    }
    var objects = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(system, &property, 0, nil, &size, &objects) == noErr else {
        return []
    }
    return objects
}

/// The processes known to Core Audio as a JSON array. Free the result with
/// `pg_audio_free_string`.
@_cdecl("pg_audio_processes_json")
public func pgAudioProcessesJson() -> UnsafeMutablePointer<CChar>? {
    let processes = processObjects().map { object in
        AudioProcess(
            object_id: object,
            pid: readValue(object, kAudioProcessPropertyPID, Int32(-1)) ?? -1,
            bundle_id: readString(object, kAudioProcessPropertyBundleID) ?? "",
            running_output: (readValue(object, kAudioProcessPropertyIsRunningOutput, UInt32(0)) ?? 0) != 0)
    }
    guard let data = try? JSONEncoder().encode(processes) else { return nil }
    return strdup(String(decoding: data, as: UTF8.self))
}

/// This process's Core Audio object, or 0 if it has none yet.
@_cdecl("pg_audio_own_process_object")
public func pgAudioOwnProcessObject() -> UInt32 {
    var property = address(kAudioHardwarePropertyTranslatePIDToProcessObject)
    var pid = getpid()
    var object = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    let status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &property,
        UInt32(MemoryLayout<pid_t>.size), &pid, &size, &object)
    return status == noErr ? object : 0
}
