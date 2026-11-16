import Foundation

@testable import KaramdBarCore

enum Fixture {
    static let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures")
    static let vault = dir.appending(path: "vault")

    /// Golden output of the real CLI, written by `tests/macos_contract.rs`.
    static func cli<T: Decodable>(_ name: String, as: T.Type = T.self) throws -> T {
        try JSONDecoder().decode(T.self, from: Data(contentsOf: dir.appending(path: "cli/\(name).json")))
    }

    static var tasks: [KTask] { get throws { try cli("list") } }
    static var config: KConfig { get throws { try cli("config") } }
}

/// UTC Gregorian calendar, so date tests do not depend on the machine.
let utc: Calendar = {
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: "UTC")!
    return c
}()

func day(_ s: String) -> Date { Day.date(s, calendar: utc)! }

/// Scratch folders for one test run. The first use deletes what earlier runs left behind.
enum Scratch {
    private static let prefix = "karamd-bar-tests-"
    private static let root: URL = {
        let tmp = FileManager.default.temporaryDirectory
        let old = (try? FileManager.default.contentsOfDirectory(atPath: tmp.path)) ?? []
        for name in old where name.hasPrefix(prefix) {
            try? FileManager.default.removeItem(at: tmp.appending(path: name))
        }
        return tmp.appending(path: prefix + UUID().uuidString)
    }()

    static func dir() throws -> URL {
        let url = root.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}

/// A shell script standing in for karamd: answers from the golden files and logs every call.
struct FakeKaramd {
    let dir: URL
    let timeout: TimeInterval

    /// `script` runs before the canned answers, so it can override them.
    init(_ script: String = "", timeout: TimeInterval = 60) throws {
        dir = try Scratch.dir()
        self.timeout = timeout
        let cli = Fixture.dir.appending(path: "cli").path
        // One call per line, arguments separated by 0x1F, so argument boundaries survive.
        let body = """
            #!/bin/sh
            printf "%s\\037" "$@" >> "$(dirname "$0")/calls"
            echo >> "$(dirname "$0")/calls"
            \(script)
            case "$1" in
              list) cat '\(cli)/list.json' ;;
              show) cat '\(cli)/show.json' ;;
              config) cat '\(cli)/config.json' ;;
              terminal-argv) cat '\(cli)/terminal-argv.json' ;;
              create) cat '\(cli)/create.json' ;;
            esac
            """
        let path = dir.appending(path: "karamd")
        try body.write(to: path, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: path.path)
    }

    var cli: KaramdCLI {
        KaramdCLI(
            binary: dir.appending(path: "karamd").path, vault: URL(fileURLWithPath: "/vault"), timeout: timeout)
    }

    /// Each call without the `--vault=/vault` that every call carries.
    var calls: [[String]] {
        let text = (try? String(contentsOf: dir.appending(path: "calls"), encoding: .utf8)) ?? ""
        return text.split(separator: "\n").map { line in
            line.split(separator: "\u{1F}", omittingEmptySubsequences: false).dropLast().map(String.init)
                .filter { $0 != "--vault=/vault" }
        }
    }
}
