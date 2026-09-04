import XCTest
@testable import SidecarDOS
final class ProtocolTests: XCTestCase {
    func testFramingAcrossEverySplit() throws {
        let packet = Packet(Ping(clientTimestamp: 42), sequence: 1, session: Data(repeating: 7, count: 16)).encode()
        for split in 0...packet.count {
            var parser = StreamParser()
            let a = try parser.append(Data(packet.prefix(split)))
            let b = try parser.append(Data(packet.dropFirst(split)))
            XCTAssertEqual((a+b).count,1)
            XCTAssertEqual(try (a+b)[0].message(Ping.self).clientTimestamp,42)
        }
    }
    func testOversizeAndTruncation() throws {
        var packet = Packet(Ping(clientTimestamp: 42), sequence: 1, session: Data(repeating: 0, count: 16)).encode()
        for n in 0..<packet.count { XCTAssertThrowsError(try Packet(data: Data(packet.prefix(n)))) }
        packet[8] = 255; packet[9] = 255; packet[10] = 255; packet[11] = 255
        var parser = StreamParser()
        XCTAssertThrowsError(try parser.append(packet))
    }
    func testAuthenticationBinding() {
        let key = Data(repeating: 3, count: 32), id = Data(repeating: 1, count: 16), nonce = Data(repeating: 4, count: 32)
        let proof = TrustStore.proof(key: key, certificate: Data("cert".utf8), client: id, nonce: nonce, role: 1)
        XCTAssertTrue(TrustStore.verify(proof, key: key, certificate: Data("cert".utf8), client: id, nonce: nonce, role: 1))
        XCTAssertFalse(TrustStore.verify(proof, key: key, certificate: Data("mitm".utf8), client: id, nonce: nonce, role: 1))
        XCTAssertFalse(TrustStore.verify(proof, key: key, certificate: Data("cert".utf8), client: id, nonce: nonce, role: 2))
    }
    func testOutOfOrderFragmentsAndLossRecovery() throws {
        func fragment(_ index: UInt16, _ frame: UInt64 = 1) -> VideoFragment {
            VideoFragment(generation: 1, frameId: frame, captureTimestamp: 100, encodeTimestamp: 110, sendTimestamp: 120,
                keyframe: 1, index: index, count: 2, totalBytes: 4, data: Data(repeating: UInt8(index), count: 2))
        }
        var assembler = FrameAssembler()
        try assembler.ingest(fragment(1), now: 200)
        XCTAssertNil(assembler.next(now: 200))
        try assembler.ingest(fragment(0), now: 210)
        XCTAssertEqual(assembler.next(now: 210)?.video.data, Data([0,0,1,1]))
        try assembler.ingest(fragment(0, 2), now: 220)
        assembler.expire(50_000)
        XCTAssertTrue(assembler.requiresKeyframe)
        XCTAssertEqual(assembler.missingFragments, 1)
    }
    @MainActor func testAnnexBParsing() throws {
        let data = Data([0,0,0,1,0x67,0xaa,0,0,1,0x68,0xbb,0,0,0,1,0x65,0xcc])
        XCTAssertEqual(try H264Decoder.nals(data), [Data([0x67,0xaa]),Data([0x68,0xbb]),Data([0x65,0xcc])])
        XCTAssertThrowsError(try H264Decoder.nals(Data([1,2,3])))
    }
}
