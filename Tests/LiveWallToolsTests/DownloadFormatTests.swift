import Testing
@testable import LiveWallTools

@Suite struct DownloadFormatSelectorTests {
    @Test func clampsHeightToBounds() {
        #expect(DownloadFormatSelector.clampedHeight(0) == 720)
        #expect(DownloadFormatSelector.clampedHeight(480) == 720)
        #expect(DownloadFormatSelector.clampedHeight(720) == 720)
        #expect(DownloadFormatSelector.clampedHeight(1440) == 1440)
        #expect(DownloadFormatSelector.clampedHeight(2160) == 2160)
        #expect(DownloadFormatSelector.clampedHeight(4320) == 2160)
    }

    @Test func prefersH264AtOrBelow1080() {
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 720, hwAV1: true) == "avc1")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1080, hwAV1: false) == "avc1")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1080, hwAV1: true) == "avc1")
    }

    @Test func prefersAV1Above1080WhenHardwareSupported() {
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1081, hwAV1: true) == "av01")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1440, hwAV1: true) == "av01")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 2160, hwAV1: true) == "av01")
    }

    @Test func fallsBackToVP9Above1080WithoutHardwareAV1() {
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1081, hwAV1: false) == "vp9")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 1440, hwAV1: false) == "vp9")
        #expect(DownloadFormatSelector.preferredCodec(maxHeight: 4320, hwAV1: false) == "vp9")
    }

    @Test func formatExpressionTable() {
        #expect(
            DownloadFormatSelector.formatExpression(maxHeight: 480, hwAV1: false)
                == "bv[height<=720][vcodec^=avc1]+ba/bv[height<=720]+ba/b[height<=720]/b"
        )
        #expect(
            DownloadFormatSelector.formatExpression(maxHeight: 1080, hwAV1: true)
                == "bv[height<=1080][vcodec^=avc1]+ba/bv[height<=1080]+ba/b[height<=1080]/b"
        )
        #expect(
            DownloadFormatSelector.formatExpression(maxHeight: 1440, hwAV1: true)
                == "bv[height<=1440][vcodec^=av01]+ba/bv[height<=1440]+ba/b[height<=1440]/b"
        )
        #expect(
            DownloadFormatSelector.formatExpression(maxHeight: 1440, hwAV1: false)
                == "bv[height<=1440][vcodec^=vp9]+ba/bv[height<=1440]+ba/b[height<=1440]/b"
        )
        #expect(
            DownloadFormatSelector.formatExpression(maxHeight: 4320, hwAV1: false)
                == "bv[height<=2160][vcodec^=vp9]+ba/bv[height<=2160]+ba/b[height<=2160]/b"
        )
    }

    @Test func splitPassExpressions() {
        #expect(
            DownloadFormatSelector.videoExpression(maxHeight: 1080, hwAV1: false)
                == "bv[height<=1080][vcodec^=avc1]/bv[height<=1080]"
        )
        #expect(
            DownloadFormatSelector.videoExpression(maxHeight: 2160, hwAV1: true)
                == "bv[height<=2160][vcodec^=av01]/bv[height<=2160]"
        )
        #expect(DownloadFormatSelector.audioExpression() == "ba")
    }

    @Test func singleFormatFallbackExpression() {
        #expect(DownloadFormatSelector.singleExpression(maxHeight: 480) == "b[height<=720]/b")
        #expect(DownloadFormatSelector.singleExpression(maxHeight: 1080) == "b[height<=1080]/b")
        #expect(DownloadFormatSelector.singleExpression(maxHeight: 9999) == "b[height<=2160]/b")
    }
}
