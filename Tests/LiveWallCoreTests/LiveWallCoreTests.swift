import Foundation
import Testing
@testable import LiveWallCore

private func makeTempDir() -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("lwcore-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func sampleLibrary() -> Library {
    let folder = Folder(id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!, name: "Nature", icon: "leaf")
    let video = LibraryItem(
        id: UUID(uuidString: "22222222-2222-2222-2222-222222222222")!,
        kind: .video,
        path: "/wallpapers/a.mp4",
        checksum: "abc123",
        addedAt: Date(timeIntervalSince1970: 1_700_000_000),
        folderID: folder.id,
        config: ItemConfig(mode: "fill", volume: 0.5, speed: 1.0, muted: true, optimized: true, stillTime: 30)
    )
    let image = LibraryItem(
        id: UUID(uuidString: "33333333-3333-3333-3333-333333333333")!,
        kind: .image,
        path: "/wallpapers/b.png",
        checksum: nil,
        addedAt: Date(timeIntervalSince1970: 1_700_000_001),
        folderID: nil,
        isPlaceholder: true
    )
    return Library(
        v: 2,
        items: [video, image],
        folders: [folder],
        current: video.id,
        previous: image.id,
        lockScreen: video.id
    )
}

@Suite struct DomainRoundtripTests {
    @Test func libraryRoundtripThroughAtomicJSON() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")
        let original = sampleLibrary()

        try AtomicJSON.write(original, to: url)
        let loaded = try AtomicJSON.read(Library.self, from: url)

        #expect(loaded == original)
        #expect(loaded.v == Library.currentVersion)
        #expect(loaded.items.count == 2)
        #expect(loaded.folders.first?.name == "Nature")
        #expect(loaded.items.last?.isPlaceholder == true)
    }

    @Test func settingsRoundtrip() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("settings.json")
        var settings = Settings()
        settings.volume = 0.25
        settings.language = "ru"
        settings.optimizeFpsLimit = 60

        try AtomicJSON.write(settings, to: url)
        let loaded = try AtomicJSON.read(Settings.self, from: url)

        #expect(loaded == settings)
        #expect(loaded.v == Settings.currentVersion)
    }

    @Test func swapStateRoundtrip() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("swap-state.json")
        let state = SwapState(
            displays: [
                SwapState.DisplayEntry(displayID: 1, originalURL: "file:///orig1.heic"),
                SwapState.DisplayEntry(displayID: 2, originalURL: "file:///orig2.jpg")
            ],
            takenAt: Date(timeIntervalSince1970: 1_700_000_000)
        )

        try AtomicJSON.write(state, to: url)
        let loaded = try AtomicJSON.read(SwapState.self, from: url)

        #expect(loaded == state)
    }

    @Test func settingsDefaultsAreVersioned() {
        let settings = Settings()
        #expect(settings.v == 2)
        #expect(settings.muted == true)
        #expect(settings.stopAfterSeconds == 30)
        #expect(settings.language == "en")
    }
}

@Suite struct AtomicJSONSafetyTests {
    @Test func writeCreatesParentDirectory() throws {
        let dir = makeTempDir().appendingPathComponent("nested/deeper")
        let url = dir.appendingPathComponent("library.json")

        try AtomicJSON.write(sampleLibrary(), to: url)

        #expect(FileManager.default.fileExists(atPath: url.path))
    }

    @Test func atomicReplaceLeavesNewestContent() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")

        try AtomicJSON.write(Library(v: 2, items: [], folders: []), to: url)
        let newest = sampleLibrary()
        try AtomicJSON.write(newest, to: url)

        #expect(try AtomicJSON.read(Library.self, from: url) == newest)
    }

    @Test func writeLeavesNoTemporaryFiles() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")

        for _ in 0..<20 {
            try AtomicJSON.write(sampleLibrary(), to: url)
        }

        let entries = try FileManager.default.contentsOfDirectory(atPath: dir.path)
        #expect(entries == ["library.json"], "leftover files: \(entries)")
    }

    @Test func readRejectsCorruptTail() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")
        var data = try AtomicJSON.encode(sampleLibrary())
        data.append(contentsOf: Data("GROKEN-TAIL".utf8))
        try data.write(to: url)

        #expect(throws: (any Error).self) {
            try AtomicJSON.read(Library.self, from: url)
        }
    }

    @Test func readRejectsTruncatedFile() throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")
        let data = try AtomicJSON.encode(sampleLibrary())
        try data.prefix(data.count / 2).write(to: url)

        #expect(throws: (any Error).self) {
            try AtomicJSON.read(Library.self, from: url)
        }
    }

    @Test func readerNeverObservesPartialFileUnderContention() async throws {
        let dir = makeTempDir()
        let url = dir.appendingPathComponent("library.json")

        let small = Library(v: 2, items: [], folders: [])
        let big = Library(
            v: 2,
            items: (0..<200).map { index in
                LibraryItem(
                    kind: index.isMultiple(of: 2) ? .video : .image,
                    path: "/tmp/file-\(index)",
                    checksum: "cs-\(index)",
                    addedAt: Date(timeIntervalSince1970: 1_700_000_000)
                )
            }
        )

        try AtomicJSON.write(small, to: url)

        let failures = await withTaskGroup(of: String?.self) { group -> [String] in
            group.addTask {
                for _ in 0..<100 {
                    try? AtomicJSON.write(big, to: url)
                    try? AtomicJSON.write(small, to: url)
                }
                return nil
            }
            for _ in 0..<4 {
                group.addTask {
                    for _ in 0..<200 {
                        do {
                            let loaded = try AtomicJSON.read(Library.self, from: url)
                            if loaded.v != 2 { return "unexpected version \(loaded.v)" }
                        } catch {
                            return "partial read: \(error)"
                        }
                    }
                    return nil
                }
            }
            var collected: [String] = []
            for await result in group {
                if let result { collected.append(result) }
            }
            return collected
        }

        #expect(failures.isEmpty, "reader observed partial file: \(failures)")
    }
}
