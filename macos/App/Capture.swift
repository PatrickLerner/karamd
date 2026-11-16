import AppKit
import Carbon.HIToolbox
import KaramdBarCore
import UserNotifications

/// Quick capture: ⌥⌘T or the + button opens a one-line field; headless Claude files the task
/// with the file-task skill and a notification reports the result.
@MainActor
final class Capture: NSObject, UNUserNotificationCenterDelegate, NSTextFieldDelegate {
    static let shared = Capture()

    private var panel: KeyPanel?
    private var hotKey: EventHotKeyRef?
    private var overviewHotKey: EventHotKeyRef?
    private var resignObserver: NSObjectProtocol?
    private weak var field: NSTextField?
    private var fieldPad: CGFloat = 0
    nonisolated private static let log = FileManager.default.homeDirectoryForCurrentUser
        .appending(path: "Library/Logs/karamd-bar-capture.log")

    func setup() {
        var spec = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)
        )
        InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, _ in
                var hk = EventHotKeyID()
                GetEventParameter(
                    event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                    nil, MemoryLayout<EventHotKeyID>.size, nil, &hk
                )
                let id = hk.id
                Task { @MainActor in
                    if id == 2 { Overview.shared.show() } else { Capture.shared.show() }
                }
                return noErr
            }, 1, &spec, nil, nil)
        RegisterEventHotKey(
            UInt32(kVK_ANSI_T), UInt32(cmdKey | optionKey),
            EventHotKeyID(signature: OSType(0x4B52_4D44), id: 1),
            GetApplicationEventTarget(), 0, &hotKey
        )
        RegisterEventHotKey(
            UInt32(kVK_ANSI_O), UInt32(cmdKey | optionKey),
            EventHotKeyID(signature: OSType(0x4B52_4D44), id: 2),
            GetApplicationEventTarget(), 0, &overviewHotKey
        )

        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func show() {
        hide()
        let w: CGFloat = 560
        let h: CGFloat = 56
        let p = KeyPanel(
            contentRect: NSRect(x: 0, y: 0, width: w, height: h),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered, defer: false
        )
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = true
        p.level = .floating

        let bg = NSVisualEffectView(frame: NSRect(x: 0, y: 0, width: w, height: h))
        bg.material = .popover
        bg.state = .active
        bg.wantsLayer = true
        bg.layer?.cornerRadius = 14
        bg.layer?.cornerCurve = .continuous
        bg.layer?.masksToBounds = true

        let font = NSFont.preferredFont(forTextStyle: .title3)
        let icon = NSImageView(image: NSImage(systemSymbolName: "plus.circle", accessibilityDescription: nil)!)
        icon.symbolConfiguration = .init(textStyle: .title3)
        icon.contentTintColor = .secondaryLabelColor
        icon.frame = NSRect(x: 16, y: (h - 22) / 2, width: 22, height: 22)
        icon.autoresizingMask = [.minYMargin]

        let field = NSTextField()
        field.placeholderString = "New task"
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.font = font
        field.delegate = self
        field.usesSingleLineMode = false
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.lineBreakMode = .byWordWrapping
        field.maximumNumberOfLines = 0
        let fh = ceil(font.ascender - font.descender + font.leading) + 4
        field.frame = NSRect(x: 48, y: (h - fh) / 2, width: w - 64, height: fh)
        field.autoresizingMask = [.height]
        fieldPad = (h - fh) / 2
        self.field = field
        bg.autoresizingMask = [.width, .height]

        bg.addSubview(icon)
        bg.addSubview(field)
        p.contentView = bg
        if let screen = NSScreen.main?.visibleFrame {
            p.setFrameOrigin(NSPoint(x: screen.midX - w / 2, y: screen.minY + screen.height * 0.66))
        }
        resignObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didResignKeyNotification, object: p, queue: .main
        ) { [weak self] _ in MainActor.assumeIsolated { self?.hide() } }
        panel = p
        NSApp.activate()
        p.makeKeyAndOrderFront(nil)
        p.makeFirstResponder(field)
    }

    private func hide() {
        if let o = resignObserver { NotificationCenter.default.removeObserver(o) }
        resignObserver = nil
        panel?.orderOut(nil)
        panel = nil
    }

    /// Grow downwards with the text, top edge fixed, capped at 8 lines.
    func controlTextDidChange(_ obj: Notification) {
        guard let field, let p = panel, let cell = field.cell, let font = field.font else { return }
        field.validateEditing()
        let line = ceil(font.ascender - font.descender + font.leading)
        let need = cell.cellSize(
            forBounds: NSRect(
                x: 0, y: 0, width: field.frame.width, height: .greatestFiniteMagnitude
            )
        ).height
        let target = min(max(need, line + 4), line * 8 + 4) + fieldPad * 2
        var f = p.frame
        guard abs(f.height - target) > 0.5 else { return }
        f.origin.y += f.height - target
        f.size.height = target
        p.setFrame(f, display: true)
    }

    func control(
        _ control: NSControl,
        textView: NSTextView, doCommandBy sel: Selector
    ) -> Bool {
        switch sel {
        case #selector(NSResponder.insertNewline(_:)):
            let t = control.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            if t.isEmpty { hide() } else { submit(t) }
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            hide()
            return true
        default:
            return false
        }
    }

    private func submit(_ text: String) {
        hide()
        Task { await report(await Self.runClaude(text)) }
    }

    nonisolated private static func runClaude(_ text: String) async -> CaptureRequest.Outcome {
        let vault = Karamd.vault
        var env = Karamd.cleanEnv
        env["KARAMD_PROMPT"] = CaptureRequest.prompt(text, vault: vault)
        let started = Date()
        let r: ProcessResult
        do {
            r = try await Subprocess.run(
                "/bin/zsh", ["-lc", CaptureRequest.command], environment: env, directory: vault, timeout: 300)
        } catch {
            return .init(ok: false, message: "Could not start claude: \(error.localizedDescription)", id: nil)
        }
        let output = String(decoding: r.stdout + r.stderr, as: UTF8.self)
        appendLog(
            """
            === \(started) (\(Int(Date().timeIntervalSince(started)))s, exit \(r.status)\(r.timedOut ? ", timed out" : ""))
            > \(text)
            \(output)

            """)
        if r.timedOut { return .init(ok: false, message: "claude timed out after 5 minutes", id: nil) }
        return CaptureRequest.parse(output, exitCode: r.status)
    }

    nonisolated private static func appendLog(_ s: String) {
        let data = Data(s.utf8)
        if let h = try? FileHandle(forWritingTo: log) {
            h.seekToEndOfFile()
            h.write(data)
            try? h.close()
        } else {
            try? data.write(to: log)
        }
    }

    private func report(_ r: CaptureRequest.Outcome) async {
        let c = UNMutableNotificationContent()
        c.title = r.ok ? "Task filed" : "Task capture failed"
        c.body = r.message
        if let id = r.id { c.userInfo = ["id": id] }
        try? await UNUserNotificationCenter.current()
            .add(UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
        await Store.shared.refresh()
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let id = response.notification.request.content.userInfo["id"] as? String
        DispatchQueue.main.async { Overview.shared.show(select: id) }
        completionHandler()
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }
}

/// Borderless panels refuse key status by default; the text field needs it.
final class KeyPanel: NSPanel {
    override var canBecomeKey: Bool { true }
}
