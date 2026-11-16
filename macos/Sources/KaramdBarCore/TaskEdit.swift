import Foundation

/// What the editor's Save turns into: `karamd edit` flags plus a tag list.
public struct TaskEdit: Equatable, Sendable {
    /// `edit` flags without `--`, in order.
    public var fields: [Field]
    /// Replaces all tags; nil leaves them alone.
    public var tags: [String]?
    /// The user removed every tag, which `karamd edit` cannot do yet.
    public var clearsAllTags: Bool

    public struct Field: Equatable, Sendable {
        public let flag: String
        public let value: String

        public init(_ flag: String, _ value: String) {
            self.flag = flag
            self.value = value
        }
    }

    public static func plan(from t: KTask, title: String, tags: String, body: String) -> TaskEdit {
        var fields: [Field] = []
        if title != t.title { fields.append(Field("title", title)) }
        if body != (t.body ?? "") { fields.append(Field("body", body)) }
        let newTags = parseTags(tags)
        let changed = newTags != (t.tags ?? [])
        return TaskEdit(
            fields: fields, tags: changed && !newTags.isEmpty ? newTags : nil,
            clearsAllTags: changed && newTags.isEmpty)
    }

    public static func isDirty(_ t: KTask, title: String, tags: String, body: String) -> Bool {
        title != t.title || parseTags(tags) != (t.tags ?? []) || body != (t.body ?? "")
    }

    public static func parseTags(_ s: String) -> [String] {
        s.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    }

    public static func formatTags(_ tags: [String]?) -> String {
        (tags ?? []).joined(separator: ", ")
    }
}
