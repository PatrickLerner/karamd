import AppKit
import KaramdBarCore

/// Settings, overridable with `defaults write dev.karamd.bar <key> <value>`.
enum Karamd {
    static var binary: String {
        UserDefaults.standard.string(forKey: "karamdPath") ?? "/run/current-system/sw/bin/karamd"
    }

    static var vault: URL {
        UserDefaults.standard.string(forKey: "vaultPath").map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: "Notes")
    }

    /// Skill quick capture tells claude to use.
    static var captureSkill: String {
        UserDefaults.standard.string(forKey: "captureSkill") ?? CaptureRequest.defaultSkill
    }

    static var terminal: String {
        UserDefaults.standard.string(forKey: "terminalPath")
            ?? NSHomeDirectory() + "/.nix-profile/bin/alacritty"
    }

    /// The terminal app sessions open in. The nix-darwin copy has a stable path, so its TCC grants survive
    /// rebuilds; a store bundle changes path on every update and asks again.
    static var terminalApp: URL? {
        if let p = UserDefaults.standard.string(forKey: "terminalAppPath") { return URL(fileURLWithPath: p) }
        let name = URL(fileURLWithPath: terminal).resolvingSymlinksInPath().lastPathComponent
        let main = URL(fileURLWithPath: "/Applications/Nix Apps/\(name.capitalized).app")
        return FileManager.default.fileExists(atPath: main.path) ? main : storeTerminalApp
    }

    /// The app bundle of a Nix-installed terminal (store layout: bin/x next to Applications/X.app), so it starts
    /// through Launch Services. Only the bundle that contains the same binary counts.
    static var storeTerminalApp: URL? {
        let binary = URL(fileURLWithPath: terminal).resolvingSymlinksInPath()
        guard binary.path.hasPrefix("/nix/store/") else { return nil }
        let root = binary.deletingLastPathComponent().deletingLastPathComponent().appending(path: "Applications")
        let apps = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
        return apps.first {
            $0.pathExtension == "app"
                && FileManager.default.fileExists(
                    atPath: $0.appending(path: "Contents/MacOS/\(binary.lastPathComponent)").path)
        }
    }

    static var cleanEnv: [String: String] { KaramdCLI.cleanEnv(ProcessInfo.processInfo.environment) }
}

extension KaramdCLI {
    /// Re-read on every use, so `defaults write` takes effect without a restart.
    static var live: KaramdCLI { KaramdCLI(binary: Karamd.binary, vault: Karamd.vault) }

    /// Open an interactive agent session on the task in the terminal, prompt seeded.
    func openSession(_ id: String) async throws {
        let args = Self.terminalArgs(try await launch(id))
        // Via Launch Services the terminal is its own responsible process for TCC,
        // so it uses its own microphone grant instead of ours.
        if let app = Karamd.terminalApp {
            // A window in a running instance, the main app first: no second Dock icon, same TCC grants.
            // Launch Services would only activate a running instance and drop the args.
            let running = NSRunningApplication.runningApplications(withBundleIdentifier: "org.alacritty")
                .sorted { a, _ in a.bundleURL?.standardizedFileURL == app.standardizedFileURL }
            for r in running {
                let socket = NSTemporaryDirectory() + "Alacritty-\(r.processIdentifier).sock"
                let result = try? await Subprocess.run(
                    Karamd.terminal, Self.createWindowArgs(socket: socket, args), timeout: 10)
                if result?.status == 0 { return }
            }
            let c = NSWorkspace.OpenConfiguration()
            c.arguments = args
            c.createsNewApplicationInstance = !running.isEmpty
            do { try await NSWorkspace.shared.openApplication(at: app, configuration: c) } catch {
                throw CLIError("cannot open \(app.path): \(error.localizedDescription)")
            }
            return
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: Karamd.terminal)
        p.environment = Karamd.cleanEnv
        p.arguments = args
        do { try p.run() } catch {
            throw CLIError("cannot run \(Karamd.terminal): \(error.localizedDescription)")
        }
    }
}
