import AppKit
import KaramdBarCore
import SwiftUI

/// Borderless glass window on the desktop layer: what WidgetKit can't do on macOS
/// (transparent background, height that follows the task count, live updates).
@MainActor
final class DesktopPanel {
    private let panel: NSPanel

    init(store: Store) {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 100),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopIconWindow)) + 1)
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        panel.isMovable = false
        panel.hidesOnDeactivate = false

        let host = NSHostingView(
            rootView: DeskView(onHeight: { [weak self] h in self?.fit(height: h) })
                .environmentObject(store))
        host.sizingOptions = []

        // Fake glass: a pre-blurred crop of the wallpaper. Real behind-window blur renders
        // black for ~1 s after every Space switch; a static image is there from frame one.
        backdrop.wantsLayer = true
        backdrop.layer?.cornerRadius = 18
        backdrop.layer?.cornerCurve = .continuous
        backdrop.layer?.masksToBounds = true
        backdrop.layer?.contentsGravity = .resize
        backdrop.layer?.backgroundColor = NSColor(white: 0.15, alpha: 0.6).cgColor
        host.frame = backdrop.bounds
        host.autoresizingMask = [.width, .height]
        backdrop.addSubview(host)
        panel.contentView = backdrop

        let screen = NSScreen.screens.first?.visibleFrame ?? .zero
        panel.setFrameTopLeftPoint(NSPoint(x: screen.minX + 40, y: screen.maxY - 30))
        panel.orderFront(nil)
        updateBackdrop()

        let nc = NotificationCenter.default
        for name in [NSWindow.didMoveNotification, NSWindow.didChangeScreenNotification] {
            nc.addObserver(forName: name, object: panel, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.updateBackdrop() }
            }
        }
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in MainActor.assumeIsolated { self?.updateBackdrop() } }
    }

    private let backdrop = NSView()
    private let wallpaper = BlurredWallpaper()

    private func updateBackdrop() {
        guard let screen = panel.screen else { return }
        backdrop.layer?.contents = wallpaper.crop(panel.frame, on: screen)

    }

    /// Resize keeping the top edge fixed, so the list grows downwards.
    private func fit(height: CGFloat) {
        var f = panel.frame
        guard abs(f.height - height) > 0.5 else { return }
        f.origin.y += f.height - height
        f.size.height = height
        panel.setFrame(f, display: true)
        updateBackdrop()
    }
}

struct DeskView: View {
    @EnvironmentObject private var store: Store
    let onHeight: (CGFloat) -> Void
    private let maxRows = 20

    var body: some View {
        let b = store.board
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text("Tasks").font(.headline)
                Spacer()
                if !b.overdue.isEmpty {
                    Label("\(b.overdue.count)", systemImage: "exclamationmark.circle.fill")
                        .font(.caption).foregroundStyle(.red)
                }
                Button {
                    Capture.shared.show()
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help("New task (⌥⌘T)")
                Button {
                    Overview.shared.show()
                } label: {
                    Image(systemName: "arrow.up.right.square")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .help("Overview (⌥⌘O)")
            }
            if b.error != nil {
                Text("karamd failed").font(.caption).foregroundStyle(.secondary)
            } else if b.sections.isEmpty {
                Text("Nothing open").font(.caption).foregroundStyle(.secondary)
            }
            SectionList(board: b, maxRows: maxRows) { t, overdue in
                TaskRow(task: t, overdue: overdue).font(.callout)
            }
        }
        .padding(14)
        .frame(width: 320, alignment: .topLeading)
        .fixedSize(horizontal: false, vertical: true)
        .onGeometryChange(for: CGFloat.self, of: { $0.size.height }, action: onHeight)
        .frame(maxHeight: .infinity, alignment: .top)
    }
}
