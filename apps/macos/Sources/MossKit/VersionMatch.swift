import Foundation

/// The app and the cc-moss command should be the same version (A12). When
/// they are not, which one to bring up to date.
public enum VersionMatch: Equatable, Sendable {
    case same
    /// The command is behind: `cc-moss update`.
    case cliOlder
    /// The app is behind (say, after `brew upgrade` alone): `cc-moss install`.
    case appOlder
    /// A dev build or something unreadable: nothing to suggest.
    case unknown

    public static func of(app: String, cli: String) -> VersionMatch {
        guard let a = parse(app), let c = parse(cli) else { return .unknown }
        if a == c { return .same }
        return a.lexicographicallyPrecedes(c) ? .appOlder : .cliOlder
    }

    /// `0.2.0` → [0, 2, 0, 1]; `0.2.0-rc.1` → [0, 2, 0, 0, 1]: a pre-release
    /// sorts before its release.
    static func parse(_ v: String) -> [Int]? {
        let parts = v.split(separator: "-", maxSplits: 1)
        let core = parts[0].split(separator: ".").compactMap { Int($0) }
        guard core.count == 3, parts[0].split(separator: ".").count == 3 else { return nil }
        if parts.count == 1 { return core + [1] }
        let pre = parts[1].split(separator: ".").compactMap { Int($0) }.first ?? 0
        return core + [0, pre]
    }
}

extension MossClient {
    /// `cc-moss update --yes`: the command (and, through it, the app).
    public func updateCLI() async throws {
        _ = try await plain(["update", "--yes"], timeout: 300)
    }

    /// `cc-moss install --yes` for the app only: copies the bundled app in
    /// and restarts it.
    public func installApp() async throws {
        _ = try await plain(["install", "--yes", "--skip", "hooks", "--skip", "tmux", "--skip", "starship", "--skip", "shell"],
                            timeout: 120)
    }
}
