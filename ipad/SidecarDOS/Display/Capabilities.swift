import UIKit
@MainActor enum Capabilities {
    static func current() -> DisplayCapabilities {
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        let screen = scene?.screen ?? UIScreen.main
        let landscape = scene?.interfaceOrientation.isLandscape ?? (screen.bounds.width > screen.bounds.height)
        let native = screen.nativeBounds.size
        let w = landscape ? max(native.width,native.height) : min(native.width,native.height)
        let h = landscape ? min(native.width,native.height) : max(native.width,native.height)
        let safe = scene?.windows.first(where: \.isKeyWindow)?.safeAreaInsets ?? .zero
        return DisplayCapabilities(physicalWidth: UInt32(w), physicalHeight: UInt32(h),
            logicalWidth: UInt32(screen.bounds.width), logicalHeight: UInt32(screen.bounds.height),
            maxFps: UInt32(screen.maximumFramesPerSecond), nativeScale: Float(screen.nativeScale),
            safeTop: Float(safe.top), safeRight: Float(safe.right), safeBottom: Float(safe.bottom), safeLeft: Float(safe.left),
            orientation: landscape ? 1 : 0)
    }
}
