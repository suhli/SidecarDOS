import Foundation
import VideoToolbox
import CoreMedia
import CoreVideo

struct DecodedFrame {
    let pixelBuffer: CVPixelBuffer
    let received: ReceivedFrame
    let decodeStart: UInt64
    let decodeEnd: UInt64
}
func monotonicUS() -> UInt64 { DispatchTime.now().uptimeNanoseconds / 1000 }

@MainActor final class H264Decoder {
    var onFrame: ((DecodedFrame) -> Void)?
    var onResetNeeded: (() -> Void)?
    private var session: VTDecompressionSession?
    private var format: CMVideoFormatDescription?
    private var sps = Data(), pps = Data()
    private var epoch: UInt64 = 0
    private(set) var inFlight = 0
    private var waitingForIDR = true

    private final class Job: @unchecked Sendable {
        weak var decoder: H264Decoder?
        let frame: ReceivedFrame
        let start: UInt64
        let epoch: UInt64
        init(_ decoder: H264Decoder, _ frame: ReceivedFrame, epoch: UInt64) {
            self.decoder = decoder; self.frame = frame; self.epoch = epoch; start = monotonicUS()
        }
    }
    func reset() {
        epoch += 1
        if let session {
            VTDecompressionSessionWaitForAsynchronousFrames(session)
            VTDecompressionSessionInvalidate(session)
        }
        session = nil; format = nil; sps = Data(); pps = Data()
        inFlight = 0; waitingForIDR = true
    }
    func decode(_ received: ReceivedFrame) {
        guard inFlight < 2 else { waitingForIDR = true; onResetNeeded?(); return }
        do {
            let nals = try Self.nals(received.video.data)
            var changed = false
            for nal in nals {
                guard let first = nal.first else { continue }
                if first & 0x1f == 7, nal != sps { sps = nal; changed = true }
                if first & 0x1f == 8, nal != pps { pps = nal; changed = true }
            }
            let idr = nals.contains { ($0.first ?? 0) & 0x1f == 5 }
            guard !waitingForIDR || idr else { return }
            if changed || session == nil {
                guard !sps.isEmpty, !pps.isEmpty, idr else { onResetNeeded?(); return }
                try configure()
            }
            guard let session, let format else { throw WireError.unexpected }
            var avcc = Data()
            for nal in nals {
                var count = UInt32(nal.count).bigEndian
                withUnsafeBytes(of: &count) { avcc.append(contentsOf: $0) }
                avcc.append(nal)
            }
            var block: CMBlockBuffer?
            try status(CMBlockBufferCreateWithMemoryBlock(allocator: kCFAllocatorDefault, memoryBlock: nil,
                blockLength: avcc.count, blockAllocator: kCFAllocatorDefault, customBlockSource: nil,
                offsetToData: 0, dataLength: avcc.count, flags: 0, blockBufferOut: &block))
            guard let block else { throw WireError.malformed }
            try avcc.withUnsafeBytes {
                guard let base = $0.baseAddress else { throw WireError.malformed }
                try status(CMBlockBufferReplaceDataBytes(with: base, blockBuffer: block, offsetIntoDestination: 0, dataLength: avcc.count))
            }
            var sample: CMSampleBuffer?
            var size = avcc.count
            var timing = CMSampleTimingInfo(duration: .invalid,
                presentationTimeStamp: CMTime(value: Int64(received.video.captureTimestamp), timescale: 1_000_000),
                decodeTimeStamp: .invalid)
            try status(CMSampleBufferCreateReady(allocator: kCFAllocatorDefault, dataBuffer: block,
                formatDescription: format, sampleCount: 1, sampleTimingEntryCount: 1, sampleTimingArray: &timing,
                sampleSizeEntryCount: 1, sampleSizeArray: &size, sampleBufferOut: &sample))
            guard let sample else { throw WireError.malformed }
            let job = Unmanaged.passRetained(Job(self, received, epoch: epoch))
            inFlight += 1
            let result = VTDecompressionSessionDecodeFrame(session, sampleBuffer: sample,
                flags: [._EnableAsynchronousDecompression, ._1xRealTimePlayback],
                frameRefcon: job.toOpaque(), infoFlagsOut: nil)
            if result != noErr {
                inFlight -= 1; job.release(); try status(result)
            }
            waitingForIDR = false
        } catch {
            reset(); onResetNeeded?()
        }
    }
    private func status(_ result: OSStatus) throws {
        guard result == noErr else { throw NSError(domain: NSOSStatusErrorDomain, code: Int(result)) }
    }
    private func configure() throws {
        if let session {
            VTDecompressionSessionWaitForAsynchronousFrames(session)
            VTDecompressionSessionInvalidate(session)
        }
        epoch += 1; inFlight = 0; session = nil
        try sps.withUnsafeBytes { a in
            try pps.withUnsafeBytes { b in
                guard let ap = a.bindMemory(to: UInt8.self).baseAddress,
                      let bp = b.bindMemory(to: UInt8.self).baseAddress else { throw WireError.malformed }
                let pointers = [ap, bp], sizes = [a.count, b.count]
                try pointers.withUnsafeBufferPointer { ptr in
                    try sizes.withUnsafeBufferPointer { sizes in
                        guard let p = ptr.baseAddress, let n = sizes.baseAddress else { throw WireError.malformed }
                        try status(CMVideoFormatDescriptionCreateFromH264ParameterSets(allocator: kCFAllocatorDefault,
                            parameterSetCount: 2, parameterSetPointers: p, parameterSetSizes: n,
                            nalUnitHeaderLength: 4, formatDescriptionOut: &format))
                    }
                }
            }
        }
        guard let format else { throw WireError.malformed }
        var callback = VTDecompressionOutputCallbackRecord(decompressionOutputCallback: { _, source, result, _, image, _, _ in
            guard let source else { return }
            let job = Unmanaged<Job>.fromOpaque(source).takeRetainedValue()
            let end = monotonicUS()
            DispatchQueue.main.async {
                guard let decoder = job.decoder, decoder.epoch == job.epoch else { return }
                decoder.inFlight = max(0, decoder.inFlight - 1)
                if result == noErr, let image {
                    decoder.onFrame?(DecodedFrame(pixelBuffer: image, received: job.frame, decodeStart: job.start, decodeEnd: end))
                } else {
                    decoder.waitingForIDR = true; decoder.onResetNeeded?()
                }
            }
        }, decompressionOutputRefCon: nil)
        let specification = [kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder: true] as CFDictionary
        let attributes: [CFString: Any] = [
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferMetalCompatibilityKey: true,
            kCVPixelBufferIOSurfacePropertiesKey: [:]]
        try status(VTDecompressionSessionCreate(allocator: kCFAllocatorDefault, formatDescription: format,
            decoderSpecification: specification, imageBufferAttributes: attributes as CFDictionary,
            outputCallback: &callback, decompressionSessionOut: &session))
        if let session { try status(VTSessionSetProperty(session, key: kVTDecompressionPropertyKey_RealTime, value: kCFBooleanTrue)) }
    }
    static func nals(_ data: Data) throws -> [Data] {
        let bytes = [UInt8](data)
        guard bytes.count <= Packet.maxFrame else { throw WireError.oversized }
        var starts: [(Int, Int)] = []; var i = 0
        while i + 3 <= bytes.count {
            if bytes[i] == 0, bytes[i + 1] == 0 {
                if bytes[i + 2] == 1 { starts.append((i, i + 3)); i += 3; continue }
                if i + 4 <= bytes.count, bytes[i + 2] == 0, bytes[i + 3] == 1 { starts.append((i, i + 4)); i += 4; continue }
            }
            i += 1
        }
        guard !starts.isEmpty, starts.count <= 4096 else { throw WireError.malformed }
        var nals: [Data] = []
        for index in starts.indices {
            let start = starts[index].1, end = index + 1 < starts.count ? starts[index + 1].0 : bytes.count
            guard end > start else { throw WireError.malformed }
            nals.append(Data(bytes[start..<end]))
        }
        return nals
    }
}
