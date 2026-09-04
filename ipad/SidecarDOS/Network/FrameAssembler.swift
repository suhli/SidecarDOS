import Foundation
struct ReceivedFrame: Sendable {
    let video: VideoFrame
    let receiveTimestamp: UInt64
}
struct FrameAssembler {
    private struct Partial {
        let meta: VideoFragment
        let arrival: UInt64
        var parts: [UInt16: Data] = [:]
        var bytes = 0
        var complete: Bool { parts.count == Int(meta.count) && bytes == Int(meta.totalBytes) }
    }
    private var frames: [UInt64: Partial] = [:]
    private var last: UInt64 = 0
    private(set) var losses: UInt32 = 0
    private(set) var receivedFragments: UInt32 = 0
    private(set) var missingFragments: UInt32 = 0
    var requiresKeyframe = true
    mutating func ingest(_ f: VideoFragment, now: UInt64) throws {
        guard f.count > 0, f.count <= 4096, f.index < f.count, f.totalBytes > 0,
              f.totalBytes <= Packet.maxFrame, !f.data.isEmpty, f.data.count <= 1100,
              f.keyframe <= 1 else { throw WireError.malformed }
        if f.frameId <= last { return }
        receivedFragments &+= 1
        expire(now)
        if frames[f.frameId] == nil {
            if frames.count >= 2, let oldest = frames.keys.min() { discard(oldest) }
            frames[f.frameId] = Partial(meta: f, arrival: now)
        }
        guard var partial = frames[f.frameId] else { return }
        let m = partial.meta
        guard m.generation == f.generation, m.count == f.count, m.totalBytes == f.totalBytes,
              m.captureTimestamp == f.captureTimestamp, m.encodeTimestamp == f.encodeTimestamp,
              m.keyframe == f.keyframe else { throw WireError.malformed }
        if partial.parts[f.index] == nil {
            guard partial.bytes + f.data.count <= Int(f.totalBytes) else { throw WireError.oversized }
            partial.parts[f.index] = f.data; partial.bytes += f.data.count
        } else if partial.parts[f.index] != f.data { throw WireError.malformed }
        frames[f.frameId] = partial
    }
    mutating func expire(_ now: UInt64) {
        for id in Array(frames.keys) {
            if let p = frames[id], now >= p.arrival, now - p.arrival > 40_000 { discard(id) }
        }
    }
    private mutating func discard(_ id: UInt64) {
        if let p = frames.removeValue(forKey: id) {
            missingFragments &+= UInt32(Int(p.meta.count) - p.parts.count)
            losses &+= 1; requiresKeyframe = true
        }
    }
    mutating func next(now: UInt64) -> ReceivedFrame? {
        for id in frames.keys.sorted() {
            guard let p = frames[id], p.complete else { continue }
            if last != 0, id != last + 1, p.meta.keyframe == 0, now - p.arrival < 8_000 { return nil }
            if last != 0, id != last + 1 { requiresKeyframe = true }
            if requiresKeyframe && p.meta.keyframe == 0 { discard(id); continue }
            var data = Data(capacity: p.bytes)
            for index in 0..<p.meta.count { guard let part = p.parts[index] else { return nil }; data.append(part) }
            frames.removeValue(forKey: id)
            for old in frames.keys.filter({ $0 < id }) { frames.removeValue(forKey: old) }
            last = id; requiresKeyframe = false
            let m = p.meta
            return ReceivedFrame(video: VideoFrame(generation: m.generation, frameId: id, captureTimestamp: m.captureTimestamp,
                encodeTimestamp: m.encodeTimestamp, sendTimestamp: m.sendTimestamp, keyframe: m.keyframe, data: data), receiveTimestamp: now)
        }
        return nil
    }
}
