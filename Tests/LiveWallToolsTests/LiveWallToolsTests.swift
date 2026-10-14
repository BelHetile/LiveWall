import Testing
@testable import LiveWallTools

@Suite struct ProgressTemplateTests {
    @Test func templateIsSingleSourceOfTruth() {
        #expect(YTDLP.progressTemplateFlag == "--progress-template")
        #expect(YTDLP.progressTemplateValue.hasPrefix("download:LWPROG:"))
        #expect(YTDLP.progressTemplateValue.contains("%(progress._percent_str)s"))
        #expect(YTDLP.progressTemplateValue.contains("%(progress.eta)s"))
        #expect(YTDLP.progressTemplateValue.contains("%(info.format_id)s"))
        #expect(YTDLP.progressArguments == [YTDLP.progressTemplateFlag, YTDLP.progressTemplateValue])
    }
}

@Suite struct ProgressParserTests {
    @Test func parsesNormalLine() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 42.5%:  37: 1.25MiB/s:137"))
        #expect(update.percent == 42.5)
        #expect(update.etaSeconds == 37)
        #expect(update.speed == "1.25MiB/s")
        #expect(update.formatID == "137")
    }

    @Test func integerPercentParses() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG:100%:0: 0.00B/s:22"))
        #expect(update.percent == 100)
        #expect(update.etaSeconds == 0)
        #expect(update.formatID == "22")
    }

    @Test func naiveETABecomesNil() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 10.0%:NA: 2.00MiB/s:140"))
        #expect(update.percent == 10)
        #expect(update.etaSeconds == nil)
        #expect(update.formatID == "140")
    }

    @Test func slashETABecomesNil() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 10.0%:N/A: 2.00MiB/s:140"))
        #expect(update.etaSeconds == nil)
    }

    @Test func unavailableSpeedBecomesNil() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 5.0%:10:NA:18"))
        #expect(update.etaSeconds == 10)
        #expect(update.speed == nil)
        #expect(update.formatID == "18")
    }

    @Test func trailingGarbageIsIgnored() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 10.0%:30: 1MiB/s:137:extra:junk"))
        #expect(update.percent == 10)
        #expect(update.etaSeconds == 30)
        #expect(update.formatID == "137")
    }

    @Test func carriageReturnIsStripped() throws {
        let update = try #require(YTDLP.parseProgress(line: "LWPROG: 3.0%:4: 1KiB/s:22\r"))
        #expect(update.formatID == "22")
    }

    @Test func missingFieldsAreRejected() {
        #expect(YTDLP.parseProgress(line: "LWPROG: 10.0%:30") == nil)
        #expect(YTDLP.parseProgress(line: "LWPROG: 10.0%") == nil)
        #expect(YTDLP.parseProgress(line: "LWPROG:") == nil)
    }

    @Test func emptyFormatIDIsRejected() {
        #expect(YTDLP.parseProgress(line: "LWPROG: 10.0%:30: 1MiB/s:") == nil)
    }

    @Test func outOfRangePercentIsRejected() {
        #expect(YTDLP.parseProgress(line: "LWPROG: 150.0%:10: 1MiB/s:18") == nil)
        #expect(YTDLP.parseProgress(line: "LWPROG:-5.0%:10: 1MiB/s:18") == nil)
    }

    @Test func nonNumericPercentIsRejected() {
        #expect(YTDLP.parseProgress(line: "LWPROG:abc:10: 1MiB/s:18") == nil)
    }

    @Test func invalidETATokenIsRejected() {
        #expect(YTDLP.parseProgress(line: "LWPROG: 10.0%:soon: 1MiB/s:18") == nil)
    }

    @Test func nonLWPROGLineIsIgnored() {
        #expect(YTDLP.parseProgress(line: "[download]  12.3% of 10.00MiB at 1.25MiB/s ETA 00:07") == nil)
        #expect(YTDLP.parseProgress(line: "") == nil)
        #expect(YTDLP.parseProgress(line: "LWPRO") == nil)
    }
}
