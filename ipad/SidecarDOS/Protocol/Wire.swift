import Foundation
enum WireError: Error { case malformed, oversized, version, unexpected, replay }
protocol WireMessage: Sendable {
    static var kind: UInt16 { get }
    func write(to writer: inout WireWriter)
    init(from reader: inout WireReader) throws
}
struct WireWriter {
    var data = Data()
    mutating func u8(_ v: UInt8) { data.append(v) }
    mutating func integer<T: FixedWidthInteger>(_ v: T) {
        var n = v.littleEndian
        withUnsafeBytes(of: &n) { data.append(contentsOf: $0) }
    }
    mutating func u16(_ v: UInt16) { integer(v) }
    mutating func u32(_ v: UInt32) { integer(v) }
    mutating func u64(_ v: UInt64) { integer(v) }
    mutating func i32(_ v: Int32) { integer(v) }
    mutating func f32(_ v: Float) { u32(v.bitPattern) }
    mutating func bytes(_ v: Data) { u32(UInt32(v.count)); data.append(v) }
    mutating func string(_ v: String) { bytes(Data(v.utf8)) }
}
struct WireReader {
    let data: Data
    var offset = 0
    mutating func take(_ count: Int) throws -> Data {
        guard count >= 0, count <= data.count - offset else { throw WireError.malformed }
        defer { offset += count }
        return data.subdata(in: offset..<(offset + count))
    }
    mutating func integer<T: FixedWidthInteger>(_ type: T.Type) throws -> T {
        let bytes = try take(MemoryLayout<T>.size)
        return bytes.withUnsafeBytes { T(littleEndian: $0.loadUnaligned(as: T.self)) }
    }
    mutating func u8() throws -> UInt8 { try integer(UInt8.self) }
    mutating func u16() throws -> UInt16 { try integer(UInt16.self) }
    mutating func u32() throws -> UInt32 { try integer(UInt32.self) }
    mutating func u64() throws -> UInt64 { try integer(UInt64.self) }
    mutating func i32() throws -> Int32 { try integer(Int32.self) }
    mutating func f32() throws -> Float {
        let v = Float(bitPattern: try u32())
        guard v.isFinite else { throw WireError.malformed }
        return v
    }
    mutating func bytes() throws -> Data {
        let count = Int(try u32())
        guard count <= Packet.maxFrame else { throw WireError.oversized }
        return try take(count)
    }
    mutating func string() throws -> String {
        let d = try bytes()
        guard d.count <= 1024, let s = String(data: d, encoding: .utf8) else { throw WireError.malformed }
        return s
    }
    func finish() throws { guard offset == data.count else { throw WireError.malformed } }
}
struct Packet: Sendable {
    static let header = 36, maxControl = 65_536, maxFrame = 2 * 1024 * 1024
    var kind: UInt16
    var sequence: UInt64
    var session: Data
    var body: Data
    init<T: WireMessage>(_ message: T, sequence: UInt64, session: Data) {
        var writer = WireWriter(); message.write(to: &writer)
        self.kind = T.kind; self.sequence = sequence; self.session = session; body = writer.data
    }
    init(data: Data, limit: Int = maxControl) throws {
        guard data.count >= Self.header, data.count <= limit else { throw WireError.oversized }
        var r = WireReader(data: data)
        guard try r.take(4) == Data("SDOS".utf8) else { throw WireError.malformed }
        guard try r.u8() == 1 else { throw WireError.version }
        _ = try r.u8(); kind = try r.u16(); let length = Int(try r.u32())
        sequence = try r.u64(); session = try r.take(16)
        guard length == data.count - Self.header else { throw WireError.malformed }
        body = try r.take(length)
    }
    func encode() -> Data {
        var w = WireWriter(data: Data("SDOS".utf8))
        w.u8(1); w.u8(0); w.u16(kind); w.u32(UInt32(body.count)); w.u64(sequence)
        w.data.append(session); w.data.append(body); return w.data
    }
    func message<T: WireMessage>(_ type: T.Type) throws -> T {
        guard kind == T.kind else { throw WireError.unexpected }
        var r = WireReader(data: body); let result = try T(from: &r); try r.finish(); return result
    }
}
/// Bounded stream parser; arbitrary QUIC receive segmentation does not affect framing.
struct StreamParser {
    private var buffer = Data()
    mutating func append(_ bytes: Data) throws -> [Packet] {
        guard buffer.count + bytes.count <= Packet.maxControl * 2 else { throw WireError.oversized }
        buffer.append(bytes)
        var result: [Packet] = []
        while buffer.count >= Packet.header {
            var r = WireReader(data: Data(buffer.prefix(Packet.header)))
            guard try r.take(4) == Data("SDOS".utf8) else { throw WireError.malformed }
            _ = try r.take(4)
            let length = Int(try r.u32()) + Packet.header
            guard length <= Packet.maxControl else { throw WireError.oversized }
            if buffer.count < length { break }
            result.append(try Packet(data: Data(buffer.prefix(length))))
            buffer.removeFirst(length)
            // Data may retain a nonzero startIndex after removeFirst.
            buffer = Data(buffer)
        }
        return result
    }
}
