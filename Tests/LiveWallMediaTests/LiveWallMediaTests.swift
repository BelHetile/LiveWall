import CoreGraphics
import Testing
@testable import LiveWallMedia

private let tolerance: CGFloat = 0.001

private func approx(_ a: CGFloat, _ b: CGFloat) -> Bool {
    return abs(a - b) <= tolerance
}

private func expectRect(
    _ rect: CGRect,
    _ x: CGFloat,
    _ y: CGFloat,
    _ width: CGFloat,
    _ height: CGFloat,
    sourceLocation: SourceLocation = #_sourceLocation
) {
    #expect(approx(rect.minX, x), "minX \(rect.minX) != \(x)", sourceLocation: sourceLocation)
    #expect(approx(rect.minY, y), "minY \(rect.minY) != \(y)", sourceLocation: sourceLocation)
    #expect(approx(rect.width, width), "width \(rect.width) != \(width)", sourceLocation: sourceLocation)
    #expect(approx(rect.height, height), "height \(rect.height) != \(height)", sourceLocation: sourceLocation)
}

@Suite struct FillModeTests {
    @Test func hasExpectedCases() {
        #expect(FillMode.allCases == [.fill, .fit, .stretch, .center])
        #expect(FillMode.fill.rawValue == "fill")
    }

    @Test func dispatchesToGeometry() {
        let bounds = CGRect(x: 0, y: 0, width: 1000, height: 1000)
        let source = CGSize(width: 1920, height: 1080)
        expectRect(MediaGeometry.rect(for: .fill, source: source, in: bounds), -388.88889, 0, 1777.77778, 1000)
        expectRect(MediaGeometry.rect(for: .fit, source: source, in: bounds), 0, 218.75, 1000, 562.5)
        expectRect(MediaGeometry.rect(for: .stretch, source: source, in: bounds), 0, 0, 1000, 1000)
    }
}

@Suite struct AspectFitTests {
    @Test func landscapeSource() {
        expectRect(
            MediaGeometry.aspectFitRect(source: CGSize(width: 1920, height: 1080), in: CGRect(x: 0, y: 0, width: 1000, height: 1000)),
            0, 218.75, 1000, 562.5
        )
    }

    @Test func portraitSource() {
        expectRect(
            MediaGeometry.aspectFitRect(source: CGSize(width: 1080, height: 1920), in: CGRect(x: 0, y: 0, width: 1000, height: 1000)),
            218.75, 0, 562.5, 1000
        )
    }

    @Test func offsetBounds() {
        expectRect(
            MediaGeometry.aspectFitRect(source: CGSize(width: 400, height: 400), in: CGRect(x: 10, y: 20, width: 800, height: 600)),
            110, 20, 600, 600
        )
    }

    @Test func degenerateSourceReturnsBounds() {
        let bounds = CGRect(x: 3, y: 4, width: 100, height: 200)
        #expect(MediaGeometry.aspectFitRect(source: .zero, in: bounds) == bounds)
    }
}

@Suite struct AspectFillTests {
    @Test func landscapeSource() {
        expectRect(
            MediaGeometry.aspectFillRect(source: CGSize(width: 1920, height: 1080), in: CGRect(x: 0, y: 0, width: 1000, height: 1000)),
            -388.88889, 0, 1777.77778, 1000
        )
    }

    @Test func portraitSource() {
        expectRect(
            MediaGeometry.aspectFillRect(source: CGSize(width: 1080, height: 1920), in: CGRect(x: 0, y: 0, width: 1000, height: 1000)),
            0, -388.88889, 1000, 1777.77778
        )
    }

    @Test func offsetBounds() {
        expectRect(
            MediaGeometry.aspectFillRect(source: CGSize(width: 400, height: 400), in: CGRect(x: 10, y: 20, width: 800, height: 600)),
            10, -80, 800, 800
        )
    }

    @Test func degenerateSourceReturnsBounds() {
        let bounds = CGRect(x: 3, y: 4, width: 100, height: 200)
        #expect(MediaGeometry.aspectFillRect(source: .zero, in: bounds) == bounds)
    }
}

@Suite struct StretchAndCenterTests {
    @Test func stretchReturnsBounds() {
        let bounds = CGRect(x: 10, y: 20, width: 800, height: 600)
        #expect(MediaGeometry.stretchRect(source: CGSize(width: 400, height: 400), in: bounds) == bounds)
    }

    @Test func centerKeepsNaturalSize() {
        expectRect(
            MediaGeometry.centerRect(source: CGSize(width: 200, height: 100), in: CGRect(x: 0, y: 0, width: 1000, height: 1000)),
            400, 450, 200, 100
        )
    }

    @Test func centerInOffsetBounds() {
        expectRect(
            MediaGeometry.centerRect(source: CGSize(width: 400, height: 400), in: CGRect(x: 10, y: 20, width: 800, height: 600)),
            210, 120, 400, 400
        )
    }

    @Test func aspectRatioComputed() {
        #expect(approx(MediaGeometry.aspectRatio(CGSize(width: 1920, height: 1080)), 1.77777778))
        #expect(approx(MediaGeometry.aspectRatio(CGSize(width: 1080, height: 1920)), 0.5625))
        #expect(MediaGeometry.aspectRatio(CGSize(width: 10, height: 0)) == 0)
    }
}
