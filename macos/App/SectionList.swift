import KaramdBarCore
import SwiftUI

/// Section headers + rows, capped at `maxRows` tasks in total.
struct SectionList<Row: View>: View {
    let board: Board
    let maxRows: Int
    @ViewBuilder let row: (KTask, Bool) -> Row

    var body: some View {
        ForEach(board.capped(maxRows)) { s in
            Text(s.title.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.top, 4)
            ForEach(s.tasks) { t in row(t, board.overdue.contains(t.id)) }
        }
        if board.total > maxRows {
            Text("+\(board.total - maxRows) more").font(.caption).foregroundStyle(.secondary)
        }
    }
}

struct PriorityIcon: View {
    let task: KTask

    var body: some View {
        Image(systemName: Vocab.priorityIcon(task.priority, fallback: "circle"))
            .font(task.priorityRank == 2 ? .system(size: 6) : .caption2.weight(.bold))
            .foregroundStyle(.secondary)
            .frame(width: 14)
    }
}
