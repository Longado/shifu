import Foundation

/// A conservative rendering-only delta. Nil asks the caller to return the full tree.
public enum TreeTextDiff {
    private struct Row {
        let index: Int
        let depth: Int
        let parent: Int?
        let text: String
    }

    private static func rows(_ tree: String) -> [Row]? {
        var result: [Row] = []
        var ancestors: [(depth: Int, index: Int)] = []
        var seen = Set<Int>()
        for line in tree.components(separatedBy: "\n") {
            let trimmed = line.drop(while: { $0 == "\t" || $0 == " " })
            let digits = trimmed.prefix(while: { $0.isASCII && $0.isNumber })
            guard !digits.isEmpty, trimmed.dropFirst(digits.count).first == " ", let index = Int(digits) else { continue }
            guard seen.insert(index).inserted else { return nil }
            let depth = line.count - trimmed.count
            while let last = ancestors.last, last.depth >= depth { ancestors.removeLast() }
            result.append(Row(index: index, depth: depth, parent: ancestors.last?.index, text: line))
            ancestors.append((depth, index))
        }
        return result.isEmpty ? nil : result
    }

    public static func render(before: String, after: String) -> String? {
        guard let old = rows(before), let new = rows(after) else { return nil }
        let oldByIndex = Dictionary(uniqueKeysWithValues: old.map { ($0.index, $0) })
        let newIndexes = Set(new.map(\.index))
        guard new.allSatisfy({ oldByIndex[$0.index] != nil }),
              old.filter({ newIndexes.contains($0.index) }).map(\.index) == new.map(\.index),
              new.allSatisfy({ row in
                  guard let previous = oldByIndex[row.index] else { return false }
                  return row.depth == previous.depth && row.parent == previous.parent
              }) else { return nil }
        let removed = old.filter { !newIndexes.contains($0.index) }
        // A removed container could hide a structural change. Only leaf removal is compacted.
        guard removed.allSatisfy({ removedRow in !old.contains(where: { $0.parent == removedRow.index }) }) else { return nil }
        let changed = new.filter { oldByIndex[$0.index]?.text != $0.text }
        let changes = removed.count + changed.count
        guard changes <= 20, changes * 2 <= old.count else { return nil }
        let controls = Set(new.map(\.text))
        var output = after.components(separatedBy: "\n").filter { !controls.contains($0) }
        if changes == 0 { output.append("No change") }
        else {
            output.append(contentsOf: changed.map { "~ " + $0.text })
            output.append(contentsOf: removed.map { "- " + $0.text })
        }
        return output.joined(separator: "\n")
    }
}
