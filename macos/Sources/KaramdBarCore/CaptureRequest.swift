import Foundation

/// Quick capture hands free text to a headless `claude -p`, which files the task through karamd.
public enum CaptureRequest {
    public static func prompt(_ text: String, vault: URL, now: Date = .now, calendar: Calendar = .current) -> String {
        let f = DateFormatter()
        f.calendar = calendar
        f.timeZone = calendar.timeZone
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd (EEEE)"
        return """
            Use the file-task skill to file this as one new task in \(vault.path). Today is \(f.string(from: now)).
            Decide title, priority, tags, due and body yourself. Do not ask questions.
            Keep names exactly as written.

            Request: \(text)

            When done, reply with exactly one line: FILED <id> <title>
            If you cannot file it, reply with exactly one line: FAILED <short reason>
            """
    }

    /// Shell command run with `zsh -lc`; the prompt comes in through `$KARAMD_PROMPT`.
    public static let command = #"exec claude -p "$KARAMD_PROMPT" --allowedTools 'Bash(karamd:*)' Skill Read Glob Grep"#

    public struct Outcome: Equatable, Sendable {
        public let ok: Bool
        public let message: String
        public let id: String?

        public init(ok: Bool, message: String, id: String?) {
            self.ok = ok
            self.message = message
            self.id = id
        }
    }

    /// The last `FILED <id> <title>` wins; else the last `FAILED <reason>`, else the last line.
    public static func parse(_ output: String, exitCode: Int32) -> Outcome {
        let lines = output.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        if let line = lines.last(where: { $0.hasPrefix("FILED ") }) {
            let rest = String(line.dropFirst("FILED ".count))
            return Outcome(ok: true, message: rest, id: rest.split(separator: " ").first.map(String.init))
        }
        let reason =
            lines.last { $0.hasPrefix("FAILED ") }.map { String($0.dropFirst("FAILED ".count)) }
            ?? lines.last ?? "no output (exit \(exitCode))"
        return Outcome(ok: false, message: reason, id: nil)
    }
}
