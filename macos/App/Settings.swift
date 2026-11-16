import AppKit
import KaramdBarCore

/// Paths, overridable with `defaults write dev.karamd.bar <key> <path>`.
enum Karamd {
    static var binary: String {
        UserDefaults.standard.string(forKey: "karamdPath") ?? "/run/current-system/sw/bin/karamd"
    }

    static var vault: URL {
        UserDefaults.standard.string(forKey: "vaultPath").map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: "Notes")
    }

    static var terminal: String {
        UserDefaults.standard.string(forKey: "terminalPath")
            ?? NSHomeDirectory() + "/.nix-profile/bin/alacritty"
    }

    /// The app bundle of a Nix-installed terminal (store layout: bin/x next to Applications/X.app), so it starts
    /// through Launch Services. Only the bundle that contains the same binary counts.
    static var terminalApp: URL? {
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
            let c = NSWorkspace.OpenConfiguration()
            c.arguments = args
            c.createsNewApplicationInstance = true
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
