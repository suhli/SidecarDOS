import UIKit
import SwiftUI
import MetalKit
@MainActor final class InputView: MTKView {
    var sendInput: ((InputEvent) -> Void)?
    var videoSize = CGSize(width: 1920, height: 1080)
    private var contacts: [ObjectIdentifier: UInt16] = [:]
    private var keys: Set<UInt16> = []
    private var buttons: UInt32 = 0
    private var lastMouse = CGPoint(x: 0.5, y: 0.5)
    override var canBecomeFirstResponder: Bool { true }
    init(renderer: DisplayRenderer) {
        super.init(frame: .zero, device: renderer.device)
        delegate = renderer; colorPixelFormat = .bgra8Unorm
        clearColor = MTLClearColorMake(0,0,0,1); preferredFramesPerSecond = 60
        isMultipleTouchEnabled = true; autoResizeDrawable = true; framebufferOnly = true
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hover(_:)))
        addGestureRecognizer(hover)
        let scroll = UIPanGestureRecognizer(target: self, action: #selector(scroll(_:)))
        scroll.allowedScrollTypesMask = .all; scroll.allowedTouchTypes = []; scroll.cancelsTouchesInView = false
        addGestureRecognizer(scroll)
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { return nil }
    private func normalized(_ point: CGPoint, clamp: Bool = false) -> CGPoint? {
        let rect = DisplayRenderer.rect(container: bounds.size, video: videoSize)
        guard rect.width > 0, rect.height > 0, clamp || rect.contains(point) else { return nil }
        return CGPoint(x: min(1,max(0,(point.x-rect.minX)/rect.width)), y: min(1,max(0,(point.y-rect.minY)/rect.height)))
    }
    private func event(kind: UInt8, phase: UInt8, id: UInt16 = 0, point: CGPoint = .zero,
                       dx: Float = 0, dy: Float = 0, key: UInt16 = 0, logical: UInt32 = 0,
                       modifiers: UInt16 = 0, repeatKey: Bool = false) {
        sendInput?(InputEvent(kind: kind, phase: phase, contactId: id, x: Float(point.x), y: Float(point.y),
            dx: dx, dy: dy, buttons: buttons, physicalKey: key, logicalKey: logical,
            modifiers: modifiers, isRepeat: repeatKey ? 1 : 0))
    }
    private func touches(_ touches: Set<UITouch>, phase: UInt8, ui: UIEvent?) {
        becomeFirstResponder()
        for touch in touches {
            if touch.type == .indirectPointer {
                if let p = normalized(touch.location(in: self), clamp: true) { lastMouse = p }
                let mask = ui?.buttonMask ?? []
                buttons = (mask.contains(.primary) ? 1 : 0) | (mask.contains(.secondary) ? 2 : 0)
                if mask.rawValue & 4 != 0 { buttons |= 4 }
                event(kind: 1, phase: phase, point: lastMouse)
                continue
            }
            guard touch.type == .direct else { continue }
            let identity = ObjectIdentifier(touch)
            if phase == 0 {
                guard normalized(touch.location(in: self)) != nil,
                      let id = (UInt16(0)..<10).first(where: { !contacts.values.contains($0) }) else { continue }
                contacts[identity] = id
            }
            guard let id = contacts[identity], let p = normalized(touch.location(in: self), clamp: true) else { continue }
            event(kind: 0, phase: phase, id: id, point: p)
            if phase >= 2 { contacts.removeValue(forKey: identity) }
        }
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { self.touches(touches, phase: 0, ui: event) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { self.touches(touches, phase: 1, ui: event) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { self.touches(touches, phase: 2, ui: event) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { self.touches(touches, phase: 3, ui: event) }
    @objc private func hover(_ gesture: UIHoverGestureRecognizer) {
        guard contacts.isEmpty, let p = normalized(gesture.location(in: self)) else { return }
        lastMouse = p; event(kind: 1, phase: 1, point: p)
    }
    @objc private func scroll(_ gesture: UIPanGestureRecognizer) {
        guard contacts.isEmpty else { return }
        let delta = gesture.translation(in: self); gesture.setTranslation(.zero, in: self)
        event(kind: 1, phase: 1, point: lastMouse, dx: Float(delta.x), dy: Float(-delta.y))
    }
    private func presses(_ presses: Set<UIPress>, phase: UInt8) {
        for press in presses {
            guard let key = press.key else { continue }
            let code = UInt16(key.keyCode.rawValue)
            let flags = key.modifierFlags
            let modifiers: UInt16 = (flags.contains(.control) ? 1 : 0) | (flags.contains(.shift) ? 2 : 0) |
                (flags.contains(.alternate) ? 4 : 0) | (flags.contains(.command) ? 8 : 0)
            let repeatKey = phase == 0 && keys.contains(code)
            event(kind: 3, phase: phase, key: code, logical: key.charactersIgnoringModifiers.unicodeScalars.first?.value ?? 0,
                  modifiers: modifiers, repeatKey: repeatKey)
            if phase >= 2 { keys.remove(code) } else { keys.insert(code) }
        }
    }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) { self.presses(presses, phase: 0) }
    override func pressesChanged(_ presses: Set<UIPress>, with event: UIPressesEvent?) { self.presses(presses, phase: 0) }
    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) { self.presses(presses, phase: 2) }
    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) { self.presses(presses, phase: 3) }
    func releaseAll() {
        for id in contacts.values { event(kind: 0, phase: 3, id: id) }
        contacts.removeAll()
        for code in keys { event(kind: 3, phase: 3, key: code) }; keys.removeAll()
        buttons = 0; event(kind: 1, phase: 2, point: lastMouse)
    }
}
struct DisplaySurface: UIViewRepresentable {
    let renderer: DisplayRenderer
    let size: CGSize
    let send: (InputEvent) -> Void
    func makeUIView(context: Context) -> InputView {
        let view = InputView(renderer: renderer); view.sendInput = send
        DispatchQueue.main.async { view.becomeFirstResponder() }; return view
    }
    func updateUIView(_ view: InputView, context: Context) { view.videoSize = size; view.sendInput = send }
    static func dismantleUIView(_ view: InputView, coordinator: ()) {
        view.releaseAll(); view.sendInput = nil; view.isPaused = true; view.delegate = nil
    }
}
