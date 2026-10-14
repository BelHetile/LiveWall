import Testing
@testable import LiveWallTools

@Suite struct DownloadProgressAccumulatorTests {
    @Test func mapsVideoToFirstNinetyPercent() {
        var accumulator = DownloadProgressAccumulator()
        #expect(accumulator.apply(0, stage: .video) == 0)
        #expect(accumulator.apply(50, stage: .video) == 45)
        #expect(accumulator.apply(100, stage: .video) == 90)
    }

    @Test func mapsAudioToFinalTenPercent() {
        var accumulator = DownloadProgressAccumulator()
        #expect(accumulator.apply(0, stage: .audio) == 90)
        #expect(accumulator.apply(50, stage: .audio) == 95)
        #expect(accumulator.apply(100, stage: .audio) == 100)
    }

    @Test func neverDecreasesAcrossStages() {
        var accumulator = DownloadProgressAccumulator()
        #expect(accumulator.apply(90, stage: .video) == 81)
        #expect(accumulator.apply(10, stage: .audio) == 91)
        #expect(accumulator.apply(5, stage: .audio) == 91)
        #expect(accumulator.apply(100, stage: .audio) == 100)
    }

    @Test func outOfOrderVideoLinesAreCapped() {
        var accumulator = DownloadProgressAccumulator()
        #expect(accumulator.apply(100, stage: .video) == 90)
        #expect(accumulator.apply(40, stage: .video) == 90)
        #expect(accumulator.apply(99, stage: .video) == 90)
    }

    @Test func interleavedOutOfOrderStagesStayMonotonic() {
        var accumulator = DownloadProgressAccumulator()
        #expect(accumulator.apply(100, stage: .audio) == 100)
        #expect(accumulator.apply(100, stage: .video) == 100)
        #expect(accumulator.apply(50, stage: .audio) == 100)
        #expect(accumulator.apply(10, stage: .video) == 100)
    }

    @Test func valuesAreClampedToBounds() {
        var accumulator = DownloadProgressAccumulator(percent: 150)
        #expect(accumulator.percent == 100)
        accumulator = DownloadProgressAccumulator(percent: -20)
        #expect(accumulator.percent == 0)
        #expect(accumulator.apply(150, stage: .video) == 100)
        #expect(accumulator.apply(-50, stage: .video) == 100)
    }
}
