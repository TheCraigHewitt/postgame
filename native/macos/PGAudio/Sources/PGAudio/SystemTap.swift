import CoreAudio
import Foundation

/// A process tap held in a private aggregate device. Every Core Audio object it
/// creates is released in `shutDown`, in reverse order of creation.
final class SystemTap {
    private let queue = DispatchQueue(label: "dev.postgame.system-tap", qos: .userInteractive)
    private var tap = AudioObjectID(kAudioObjectUnknown)
    private var aggregate = AudioObjectID(kAudioObjectUnknown)
    private var ioProc: AudioDeviceIOProcID?

    init(description: CATapDescription, callback: @escaping PGAudioCallback, context: UnsafeMutableRawPointer?) throws {
        do {
            try build(description, callback, context)
        } catch {
            shutDown()
            throw error
        }
    }

    private func build(
        _ description: CATapDescription,
        _ callback: @escaping PGAudioCallback,
        _ context: UnsafeMutableRawPointer?
    ) throws {
        description.isPrivate = true
        description.muteBehavior = .unmuted

        var status = AudioHardwareCreateProcessTap(description, &tap)
        guard status == noErr else { throw CaptureFailure("creating the process tap", status: status) }

        let output = readValue(
            AudioObjectID(kAudioObjectSystemObject), kAudioHardwarePropertyDefaultSystemOutputDevice,
            AudioObjectID(kAudioObjectUnknown))
        guard let output, let outputUID = readString(output, kAudioDevicePropertyDeviceUID) else {
            throw CaptureFailure("no default output device")
        }
        let device: [String: Any] = [
            kAudioAggregateDeviceNameKey: "postgame system tap",
            kAudioAggregateDeviceUIDKey: UUID().uuidString,
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputUID]],
            kAudioAggregateDeviceTapListKey: [
                [kAudioSubTapDriftCompensationKey: true, kAudioSubTapUIDKey: description.uuid.uuidString]
            ],
        ]
        status = AudioHardwareCreateAggregateDevice(device as CFDictionary, &aggregate)
        guard status == noErr else { throw CaptureFailure("creating the aggregate device", status: status) }

        guard let format = readValue(tap, kAudioTapPropertyFormat, AudioStreamBasicDescription()) else {
            throw CaptureFailure("reading the tap format")
        }
        guard format.mFormatID == kAudioFormatLinearPCM,
            format.mFormatFlags & kAudioFormatFlagIsFloat != 0,
            format.mBitsPerChannel == 32
        else {
            throw CaptureFailure("the tap format is not Float32")
        }

        let sampleRate = format.mSampleRate
        status = AudioDeviceCreateIOProcIDWithBlock(&ioProc, aggregate, queue) { _, input, inputTime, _, _ in
            deliver(input, inputTime, sampleRate, callback, context)
        }
        guard status == noErr else { throw CaptureFailure("creating the IO proc", status: status) }

        status = AudioDeviceStart(aggregate, ioProc)
        guard status == noErr else { throw CaptureFailure("starting the aggregate device", status: status) }
    }

    func shutDown() {
        if let ioProc {
            AudioDeviceStop(aggregate, ioProc)
            AudioDeviceDestroyIOProcID(aggregate, ioProc)
            self.ioProc = nil
        }
        // After this, no callback is running or will run.
        queue.sync {}
        if aggregate != kAudioObjectUnknown {
            AudioHardwareDestroyAggregateDevice(aggregate)
            aggregate = AudioObjectID(kAudioObjectUnknown)
        }
        if tap != kAudioObjectUnknown {
            AudioHardwareDestroyProcessTap(tap)
            tap = AudioObjectID(kAudioObjectUnknown)
        }
    }
}

private func deliver(
    _ input: UnsafePointer<AudioBufferList>,
    _ time: UnsafePointer<AudioTimeStamp>,
    _ sampleRate: Double,
    _ callback: PGAudioCallback,
    _ context: UnsafeMutableRawPointer?
) {
    let list = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: input))
    guard let first = list.first, first.mNumberChannels > 0,
        time.pointee.mFlags.contains(.hostTimeValid)
    else { return }

    let frames = first.mDataByteSize / (first.mNumberChannels * UInt32(MemoryLayout<Float>.size))
    let pointers = list.map { UnsafePointer($0.mData?.assumingMemoryBound(to: Float.self)) }
    pointers.withUnsafeBufferPointer { buffers in
        callback(
            context, buffers.baseAddress!, UInt32(list.count), first.mNumberChannels, frames,
            AudioConvertHostTimeToNanos(time.pointee.mHostTime), sampleRate)
    }
}

/// Taps `processes`, or everything except them when `exclude` is true.
@_cdecl("pg_audio_system_start")
public func pgAudioSystemStart(
    _ processes: UnsafePointer<UInt32>?,
    _ processCount: Int,
    _ exclude: Bool,
    _ callback: @escaping PGAudioCallback,
    _ context: UnsafeMutableRawPointer?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> UnsafeMutableRawPointer? {
    let ids = Array(UnsafeBufferPointer(start: processes, count: processCount))
    return startHandle(errorOut) {
        let description =
            exclude
            ? CATapDescription(stereoGlobalTapButExcludeProcesses: ids)
            : CATapDescription(stereoMixdownOfProcesses: ids)
        return try SystemTap(description: description, callback: callback, context: context)
    }
}

/// Stops the tap. No callback runs after this returns.
@_cdecl("pg_audio_system_stop")
public func pgAudioSystemStop(_ handle: UnsafeMutableRawPointer) {
    let tap = Unmanaged<SystemTap>.fromOpaque(handle).takeRetainedValue()
    tap.shutDown()
}
