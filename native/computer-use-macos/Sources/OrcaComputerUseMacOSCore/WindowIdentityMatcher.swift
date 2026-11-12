import CoreGraphics

public enum WindowIdentityMatcher {
    public struct Candidate {
        public let id: CGWindowID?
        public let readable: Bool
        public let frame: CGRect?
        public let allowsGeometry: Bool
        public init(id: CGWindowID?, readable: Bool = true, frame: CGRect?, allowsGeometry: Bool = true) {
            self.id = id; self.readable = readable; self.frame = frame; self.allowsGeometry = allowsGeometry
        }
    }
    public static func match(_ candidates: [Candidate], target: CGWindowID, frame: CGRect) -> Int? {
        let exact = candidates.indices.filter { candidates[$0].readable && candidates[$0].id == target }
        if exact.count == 1 { return exact[0] }
        if !exact.isEmpty { return nil }
        let matches = candidates.indices.filter { index in
            let candidate = candidates[index]
            guard candidate.readable, candidate.allowsGeometry, candidate.id == nil, let bounds = candidate.frame,
                  !bounds.isEmpty, !frame.isEmpty else { return false }
            return abs(bounds.minX - frame.minX) <= 2 && abs(bounds.minY - frame.minY) <= 2 &&
                abs(bounds.width - frame.width) <= 2 && abs(bounds.height - frame.height) <= 2
        }
        return matches.count == 1 ? matches[0] : nil
    }
    public static func recoveryTarget(_ candidates: [Candidate], target: CGWindowID?, frame: CGRect?) -> Int? {
        if let target { return match(candidates, target: target, frame: frame ?? .null) }
        guard let frame else { return candidates.isEmpty ? nil : 0 }
        let matches = candidates.indices.filter { index in
            let candidate = candidates[index]
            guard candidate.readable, candidate.allowsGeometry, let bounds = candidate.frame else { return false }
            return abs(bounds.minX - frame.minX) <= 2 && abs(bounds.minY - frame.minY) <= 2 &&
                abs(bounds.width - frame.width) <= 2 && abs(bounds.height - frame.height) <= 2
        }
        return matches.count == 1 ? matches[0] : nil
    }

}
