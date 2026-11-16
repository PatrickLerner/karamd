import Foundation

/// taskmd status and priority values, and the names and SF Symbols the app shows for them.
public enum Vocab {
    public static let priorities = ["critical", "high", "medium", "low"]
    public static let statuses = ["pending", "in-progress", "in-review", "blocked", "completed", "cancelled"]
    public static let terminalStatuses: Set<String> = ["completed", "cancelled"]

    /// Lower sorts first; unknown and missing count as medium.
    public static func priorityRank(_ p: String?) -> Int {
        switch p {
        case "critical": 0
        case "high": 1
        case "low": 3
        default: 2
        }
    }

    public static func isUrgent(_ p: String?) -> Bool { priorityRank(p) < 2 }

    /// `fallback` is the symbol for medium and unknown.
    public static func priorityIcon(_ p: String?, fallback: String = "minus") -> String {
        switch p {
        case "critical": "exclamationmark.3"
        case "high": "exclamationmark.2"
        case "low": "arrow.down"
        default: fallback
        }
    }

    public static func statusIcon(_ s: String) -> String {
        switch s {
        case "in-progress": "circle.lefthalf.filled"
        case "in-review": "eye.circle"
        case "blocked": "exclamationmark.octagon"
        case "completed": "checkmark.circle.fill"
        case "cancelled": "xmark.circle"
        default: "circle"
        }
    }

    public static func statusName(_ s: String) -> String {
        s == "pending" ? "To Do" : s.replacingOccurrences(of: "-", with: " ").capitalized
    }
}
