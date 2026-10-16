import Foundation

/// "Stop reminding": `cc-moss close`, after which a session is no longer
/// offered for restore (`cc-moss ls --all` still lists it).
public enum Close {
    /// `close -- <target>…`; `--` keeps a name starting with "-" a name.
    public static func arguments(targets: [String]) -> [String] {
        ["close", "--"] + targets
    }
}

extension MossClient {
    /// `cc-moss close -- <target>…`; cc-moss's own words when it says no.
    public func close(_ targets: [String]) async throws {
        _ = try await plainData(Close.arguments(targets: targets))
    }
}
