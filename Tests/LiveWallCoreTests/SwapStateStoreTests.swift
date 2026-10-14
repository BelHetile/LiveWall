import Foundation
import Testing
@testable import LiveWallCore

private func makeTempDirectory() -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("lwswap-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@Suite struct SwapStateStoreTests {
    @Test func loadReturnsNilWhenAbsent() async throws {
        let root = makeTempDirectory()
        let store = SwapStateStore(url: root.appendingPathComponent("swap-state.json"))

        let loaded = try await store.load()
        let exists = await store.exists

        #expect(loaded == nil)
        #expect(exists == false)
    }

    @Test func saveLoadClearRoundtrip() async throws {
        let root = makeTempDirectory()
        let store = SwapStateStore(url: root.appendingPathComponent("swap-state.json"))
        let state = SwapState(
            displays: [
                SwapState.DisplayEntry(displayID: 1, originalURL: "file:///orig/one.heic"),
                SwapState.DisplayEntry(displayID: 2, originalURL: "file:///orig/two.jpg")
            ],
            takenAt: Date(timeIntervalSince1970: 1_700_000_000)
        )

        try await store.save(state)

        let exists = await store.exists
        #expect(exists)

        let loaded = try await store.load()
        #expect(loaded == state)

        try await store.clear()

        let afterClear = try await store.load()
        let existsAfterClear = await store.exists
        #expect(afterClear == nil)
        #expect(existsAfterClear == false)
    }

    @Test func clearWhenAbsentIsNoOp() async throws {
        let root = makeTempDirectory()
        let store = SwapStateStore(url: root.appendingPathComponent("swap-state.json"))

        try await store.clear()

        let loaded = try await store.load()
        #expect(loaded == nil)
    }

    @Test func corruptFileThrowsTypedError() async throws {
        let root = makeTempDirectory()
        let url = root.appendingPathComponent("swap-state.json")
        try Data("not-json".utf8).write(to: url)
        let store = SwapStateStore(url: url)

        await #expect(throws: LiveWallError.swapStateCorrupt(path: url.path)) {
            _ = try await store.load()
        }
    }

    @Test func defaultURLUsesApplicationSupport() {
        let url = SwapStateStore.defaultURL()
        #expect(url.lastPathComponent == "swap-state.json")
        #expect(url.deletingLastPathComponent().lastPathComponent == "LiveWall")
    }
}
