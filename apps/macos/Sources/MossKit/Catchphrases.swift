import Foundation

/// The little things Mo murmurs in a puff when something wants you
/// (DESIGN-pet §6.5, everyday mode): a word or two, never the message
/// itself. Random, but never the same one twice in a row.
public struct Catchphrases: Sendable {
    public static let english = ["Hm?", "…", "Got a sec?", "One thing…"]
    public static let chinese = ["嗯？", "…", "有件事～", "回头看一眼？"]

    public static func all(_ lang: Lang) -> [String] { lang == .zh ? chinese : english }

    private var last: String?
    private var rng: SplitMix

    /// `seed` makes the order repeatable (tests); nil draws from the system.
    public init(seed: UInt64? = nil) {
        rng = SplitMix(seed: seed ?? UInt64.random(in: .min ... .max))
    }

    /// The next one, in the interface language unless told otherwise.
    public mutating func next(_ lang: Lang = L.lang) -> String {
        let pool = Self.all(lang).filter { $0 != last }
        let pick = pool[Int(rng.next() % UInt64(pool.count))]
        last = pick
        return pick
    }
}

/// A tiny seedable generator, so a test can replay the same draws.
struct SplitMix: Sendable {
    private var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
}
