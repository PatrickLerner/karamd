import AppKit
import CoreImage

/// The screen's wallpaper, aspect-filled like macOS draws it, blurred and darkened once;
/// `crop` cuts out the part behind a window.
@MainActor
final class BlurredWallpaper {
    private var key = ""
    private var image: CGImage?
    private let context = CIContext()

    func crop(_ windowFrame: NSRect, on screen: NSScreen) -> CGImage? {
        guard let url = NSWorkspace.shared.desktopImageURL(for: screen) else { return nil }
        let k = "\(url.path)|\(screen.frame)"
        if k != key {
            key = k
            image = render(url, size: screen.frame.size, scale: screen.backingScaleFactor)
        }
        guard let full = image else { return nil }
        let px = CGFloat(full.width) / screen.frame.width
        let r = windowFrame.offsetBy(dx: -screen.frame.minX, dy: -screen.frame.minY)
        let rect = CGRect(
            x: r.minX * px, y: (screen.frame.height - r.maxY) * px,
            width: r.width * px, height: r.height * px
        ).integral
        return full.cropping(to: rect)
    }

    private func render(_ url: URL, size: CGSize, scale: CGFloat) -> CGImage? {
        guard let src = CIImage(contentsOf: url) else { return nil }
        let w = size.width * scale
        let h = size.height * scale
        let s = max(w / src.extent.width, h / src.extent.height)
        let dx = (src.extent.width * s - w) / 2
        let dy = (src.extent.height * s - h) / 2
        let bounds = CGRect(x: 0, y: 0, width: w, height: h)
        let filled =
            src
            .transformed(by: CGAffineTransform(scaleX: s, y: s).concatenating(.init(translationX: -dx, y: -dy)))
        let blurred = filled.clampedToExtent().applyingGaussianBlur(sigma: 30 * scale).cropped(to: bounds)
        let tint = CIImage(color: CIColor(red: 0.1, green: 0.11, blue: 0.13, alpha: 0.35)).cropped(to: bounds)
        return context.createCGImage(tint.composited(over: blurred), from: bounds)
    }
}
