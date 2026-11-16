import Foundation

/// taskmd dates are plain `YYYY-MM-DD` strings in local time.
public enum Day {
    public static func string(_ d: Date, calendar: Calendar = .current) -> String {
        formatter(calendar).string(from: d)
    }

    public static func date(_ s: String, calendar: Calendar = .current) -> Date? {
        formatter(calendar).date(from: s)
    }

    public static func adding(_ days: Int, to d: Date, calendar: Calendar = .current) -> Date {
        // Gregorian day arithmetic on a real date cannot fail.
        calendar.date(byAdding: .day, value: days, to: d)!
    }

    /// "Today", "Tomorrow", "Yesterday", the weekday within a week, else "12 Dec" (plus the year if not this one).
    public static func relative(
        _ s: String, now: Date = .now, calendar: Calendar = .current, locale: Locale = .current
    ) -> String {
        guard let d = date(s, calendar: calendar) else { return s }
        let days = Int(
            (calendar.startOfDay(for: d).timeIntervalSince(calendar.startOfDay(for: now)) / 86_400).rounded())
        var style = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
        switch days {
        case 0: return "Today"
        case 1: return "Tomorrow"
        case -1: return "Yesterday"
        case 2...6: return d.formatted(style.weekday(.wide))
        default:
            style = style.day().month(.abbreviated)
            if calendar.component(.year, from: d) != calendar.component(.year, from: now) {
                style = style.year()
            }
            return d.formatted(style)
        }
    }

    /// Shortcuts in the due date picker.
    public static func quickPicks(now: Date = .now, calendar: Calendar = .current) -> [(label: String, date: Date)] {
        let today = calendar.startOfDay(for: now)
        // Weekday 1 is Sunday; on a Monday, next Monday is a week away.
        let ahead = (9 - calendar.component(.weekday, from: today)) % 7
        let monday = adding(ahead == 0 ? 7 : ahead, to: today, calendar: calendar)
        return [
            ("Today", today),
            ("Tomorrow", adding(1, to: today, calendar: calendar)),
            ("Next Monday", monday),
            ("In a Week", adding(7, to: today, calendar: calendar)),
        ]
    }

    private static func formatter(_ calendar: Calendar) -> DateFormatter {
        let f = DateFormatter()
        f.calendar = calendar
        f.timeZone = calendar.timeZone
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f
    }
}
