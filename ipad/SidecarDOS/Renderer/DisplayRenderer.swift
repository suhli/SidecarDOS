import Foundation
import MetalKit
import CoreVideo
@MainActor final class DisplayRenderer: NSObject, MTKViewDelegate {
    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private var cache: CVMetalTextureCache?
    private var pending: DecodedFrame?
    private var inFlight = 0
    var onPresent: ((DecodedFrame, UInt64, UInt64) -> Void)?
    static func makeDefault() throws -> DisplayRenderer {
        guard let device = MTLCreateSystemDefaultDevice() else { throw WireError.unexpected }
        return try DisplayRenderer(device: device)
    }
    init(device: MTLDevice) throws {
        guard let queue = device.makeCommandQueue(),
              let library = device.makeDefaultLibrary() else { throw WireError.unexpected }
        self.device = device; self.queue = queue
        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.vertexFunction = library.makeFunction(name: "displayVertex")
        descriptor.fragmentFunction = library.makeFunction(name: "displayFragment")
        descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
        pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
        super.init()
        guard CVMetalTextureCacheCreate(kCFAllocatorDefault, nil, device, nil, &cache) == kCVReturnSuccess else { throw WireError.unexpected }
    }
    func enqueue(_ frame: DecodedFrame) { pending = frame }
    func reset() { pending = nil; if let cache { CVMetalTextureCacheFlush(cache, 0) } }
    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}
    static func rect(container: CGSize, video: CGSize) -> CGRect {
        guard container.width > 0, container.height > 0, video.width > 0, video.height > 0 else { return .zero }
        let scale = min(container.width / video.width, container.height / video.height)
        let size = CGSize(width: video.width * scale, height: video.height * scale)
        return CGRect(x: (container.width - size.width) / 2, y: (container.height - size.height) / 2, width: size.width, height: size.height)
    }
    func draw(in view: MTKView) {
        guard inFlight < 2, let frame = pending, let cache,
              let drawable = view.currentDrawable, let pass = view.currentRenderPassDescriptor,
              let command = queue.makeCommandBuffer() else { return }
        pending = nil; let start = monotonicUS()
        let buffer = frame.pixelBuffer
        var yRef: CVMetalTexture?, uvRef: CVMetalTexture?
        guard CVMetalTextureCacheCreateTextureFromImage(kCFAllocatorDefault, cache, buffer, nil, .r8Unorm,
                CVPixelBufferGetWidthOfPlane(buffer, 0), CVPixelBufferGetHeightOfPlane(buffer, 0), 0, &yRef) == kCVReturnSuccess,
              CVMetalTextureCacheCreateTextureFromImage(kCFAllocatorDefault, cache, buffer, nil, .rg8Unorm,
                CVPixelBufferGetWidthOfPlane(buffer, 1), CVPixelBufferGetHeightOfPlane(buffer, 1), 1, &uvRef) == kCVReturnSuccess,
              let yRef, let uvRef, let y = CVMetalTextureGetTexture(yRef), let uv = CVMetalTextureGetTexture(uvRef),
              let encoder = command.makeRenderCommandEncoder(descriptor: pass) else { return }
        let rect = Self.rect(container: view.drawableSize, video: CGSize(width: CVPixelBufferGetWidth(buffer), height: CVPixelBufferGetHeight(buffer)))
        encoder.setViewport(MTLViewport(originX: rect.minX, originY: rect.minY, width: rect.width, height: rect.height, znear: 0, zfar: 1))
        encoder.setRenderPipelineState(pipeline)
        encoder.setFragmentTexture(y, index: 0); encoder.setFragmentTexture(uv, index: 1)
        encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        encoder.endEncoding(); inFlight += 1
        command.addCompletedHandler { [weak self, yRef, uvRef, buffer] command in
            _ = (yRef, uvRef, buffer)
            #if targetEnvironment(simulator)
            // Simulator has no drawable presentation handler. Completion is only a timing estimate.
            let completed = command.status == .completed
            let end = monotonicUS()
            #endif
            DispatchQueue.main.async {
                guard let self else { return }
                self.inFlight = max(0, self.inFlight - 1)
                #if targetEnvironment(simulator)
                if completed { self.onPresent?(frame, start, end) }
                #endif
            }
        }
        #if !targetEnvironment(simulator)
        drawable.addPresentedHandler { [weak self] drawable in
            let time = drawable.presentedTime
            guard time > 0 else { return } // A skipped drawable was not presented.
            let present = UInt64(time * 1_000_000)
            DispatchQueue.main.async { self?.onPresent?(frame, start, present) }
        }
        #endif
        command.present(drawable); command.commit()
    }
}
