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

    /// App bundle next to the terminal binary (nix layout: bin/x -> Applications/X.app).
    static var terminalApp: URL? {
        let root = URL(fileURLWithPath: terminal).resolvingSymlinksInPath()
            .deletingLastPathComponent().deletingLastPathComponent().appending(path: "Applications")
        let apps = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
        return apps.first { $0.pathExtension == "app" }
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
