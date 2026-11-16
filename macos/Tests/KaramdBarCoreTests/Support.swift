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

/// A shell script standing in for karamd: answers from the golden files and logs every call.
struct FakeKaramd {
    let dir: URL
    var cli: KaramdCLI { KaramdCLI(binary: dir.appending(path: "karamd").path, vault: URL(fileURLWithPath: "/vault")) }

    /// `script` runs before the canned answers, so it can override them.
    init(_ script: String = "") throws {
        dir = FileManager.default.temporaryDirectory.appending(path: "fake-karamd-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let cli = Fixture.dir.appending(path: "cli").path
        let body = """
            #!/bin/sh
            echo "$*" >> "$(dirname "$0")/calls"
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

    var calls: [String] {
        let text = (try? String(contentsOf: dir.appending(path: "calls"), encoding: .utf8)) ?? ""
        return text.split(separator: "\n").map { $0.replacingOccurrences(of: " --vault /vault", with: "") }
    }
}
