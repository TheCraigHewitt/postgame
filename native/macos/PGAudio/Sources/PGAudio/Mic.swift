import AVFoundation
import CoreAudio
import Foundation

private func requestMicAccess() -> Bool {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized:
        return true
    case .notDetermined:
        let answered = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var granted = false
        AVCaptureDevice.requestAccess(for: .audio) { result in
            granted = result
            answered.signal()
        }
        answered.wait()
        return granted
    default:
        return false
    }
}

/// Records the default input device.
final class MicCapture {
    private let engine = AVAudioEngine()

    init(callback: @escaping PGAudioCallback, context: UnsafeMutableRawPointer?) throws {
        guard requestMicAccess() else {
            throw CaptureFailure("microphone access is denied")
        }
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw CaptureFailure("the default input device has no usable format")
        }
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, time in
            guard let channels = buffer.floatChannelData, time.isHostTimeValid else { return }
            let pointers = (0..<Int(format.isInterleaved ? 1 : format.channelCount)).map {
                UnsafePointer<Float>?(channels[$0])
            }
            pointers.withUnsafeBufferPointer { buffers in
                callback(
                    context, buffers.baseAddress!, UInt32(pointers.count),
                    format.isInterleaved ? format.channelCount : 1, buffer.frameLength,
                    AudioConvertHostTimeToNanos(time.hostTime), format.sampleRate)
            }
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            input.removeTap(onBus: 0)
            throw CaptureFailure("starting the mic: \(error.localizedDescription)")
        }
    }

    func shutDown() {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
    }
}

/// Starts the default mic, asking for permission first if needed.
@_cdecl("pg_audio_mic_start")
public func pgAudioMicStart(
    _ callback: @escaping PGAudioCallback,
    _ context: UnsafeMutableRawPointer?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>
) -> UnsafeMutableRawPointer? {
    startHandle(errorOut) { try MicCapture(callback: callback, context: context) }
}

/// Stops the mic. A callback already in flight may still finish.
@_cdecl("pg_audio_mic_stop")
public func pgAudioMicStop(_ handle: UnsafeMutableRawPointer) {
    let mic = Unmanaged<MicCapture>.fromOpaque(handle).takeRetainedValue()
    mic.shutDown()
}
