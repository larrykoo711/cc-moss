import Testing
@testable import MossKit

@Suite struct CatchphraseTests {
    @Test func bothLanguages() {
        var p = Catchphrases(seed: 1)
        let zh = Set((0..<50).map { _ in p.next(.zh) })
        let en = Set((0..<50).map { _ in p.next(.en) })
        #expect(zh == Set(Catchphrases.chinese))
        #expect(en == Set(Catchphrases.english))
        #expect(Catchphrases.chinese.count == Catchphrases.english.count)
        #expect(Catchphrases.chinese.contains("嗯？"))
        #expect(Catchphrases.english.contains("Hm?"))
    }

    @Test func neverTheSameTwiceInARow() {
        for seed in UInt64(0)..<20 {
            var p = Catchphrases(seed: seed)
            var last = ""
            for _ in 0..<100 {
                let next = p.next(.en)
                #expect(next != last)
                last = next
            }
        }
    }
}
