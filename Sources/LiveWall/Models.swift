import Foundation
import AppKit
import Carbon.HIToolbox

// MARK: - Fill modes

enum FillMode: String, Codable, CaseIterable, Identifiable {
    case fill, fit, stretch, center

    var id: String { rawValue }

    var title: String {
        switch self {
        case .fill: return "Fill"
        case .fit: return "Fit"
        case .stretch: return "Stretch"
        case .center: return "Center"
        }
    }

    var next: FillMode {
        let all = FillMode.allCases
        let i = all.firstIndex(of: self) ?? 0
        return all[(i + 1) % all.count]
    }
}

enum MediaKind: String, Codable {
    case video, image, gif
}

struct WallpaperItem: Codable, Identifiable, Equatable {
    var id: UUID
    var name: String
    var file: String          // имя файла внутри папки хранилища
    var kind: MediaKind
    var mode: FillMode
    // свои настройки воспроизведения у каждых обоев (nil — берутся значения по умолчанию)
    var volume: Double?
    var speed: Double?
    var muted: Bool?
    // папка и свой хоткей-переключатель
    var folderID: UUID?
    var hotkey: HotkeyBinding?
}

// MARK: - Folders

enum FolderIcon: Codable, Equatable {
    case symbol(name: String, palette: Int)
    case image(file: String)

    /// Базовый пак иконок (SF Symbols) — рисуются на цветном градиенте.
    static let basePack: [String] = [
        "folder.fill", "star.fill", "heart.fill", "sparkles", "moon.stars.fill", "sun.max.fill",
        "cloud.fill", "bolt.fill", "flame.fill", "drop.fill", "snowflake", "leaf.fill",
        "mountain.2.fill", "globe.americas.fill", "beach.umbrella.fill", "tent.fill", "sailboat.fill", "airplane",
        "car.fill", "bicycle", "house.fill", "building.2.fill", "pawprint.fill", "cat.fill",
        "dog.fill", "bird.fill", "fish.fill", "hare.fill", "tortoise.fill", "gamecontroller.fill",
        "music.note", "headphones", "film.fill", "camera.fill", "paintpalette.fill", "paintbrush.fill",
        "theatermasks.fill", "book.fill", "graduationcap.fill", "laptopcomputer", "desktopcomputer", "cup.and.saucer.fill",
        "crown.fill", "gift.fill", "trophy.fill", "wand.and.stars", "atom", "eye.fill",
        "face.smiling.fill", "flag.fill", "bookmark.fill", "tag.fill", "lock.fill", "briefcase.fill",
    ]
}

struct WallpaperFolder: Codable, Identifiable, Equatable {
    var id: UUID
    var name: String
    var icon: FolderIcon
    // закрытая папка: соль и PBKDF2-хеш пароля (base64)
    var passwordSalt: String?
    var passwordHash: String?

    var isProtected: Bool { passwordHash != nil && passwordSalt != nil }
}

/// Что сейчас записывается в рекордере хоткея.
enum HotkeyTarget: Equatable {
    case action(HotkeyAction)
    case wallpaper(UUID)
}

/// Загрузка обоев по ссылке (для списка с прогрессом).
struct LinkDownload: Identifiable, Equatable {
    let id: UUID
    var title: String
    var progress: Double
    var error: String?
}

// MARK: - Hotkeys

enum HotkeyAction: String, Codable, CaseIterable, Identifiable {
    case next, previous, random, togglePause, cycleMode, toggleMute, openLibrary

    var id: String { rawValue }

    var title: String {
        switch self {
        case .next: return "Next wallpaper"
        case .previous: return "Previous wallpaper"
        case .random: return "Random wallpaper"
        case .togglePause: return "Pause / resume video"
        case .cycleMode: return "Cycle fill mode"
        case .toggleMute: return "Mute / unmute"
        case .openLibrary: return "Open library"
        }
    }

    var hotkeyID: UInt32 {
        UInt32((HotkeyAction.allCases.firstIndex(of: self) ?? 0) + 1)
    }
}

struct HotkeyBinding: Codable, Equatable {
    var keyCode: UInt32
    var modifiers: UInt32   // Carbon-модификаторы (cmdKey, optionKey, ...)
}

// MARK: - Pause rules

struct AppRule: Codable, Identifiable, Equatable {
    var bundleID: String
    var name: String
    var frontmostOnly: Bool      // true — только когда приложение в фокусе, false — пока оно запущено

    var id: String { bundleID }
}

// MARK: - Color styles

struct ColorStyle: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var primary: String     // hex, например #7D4DFA
    var secondary: String

    static let builtIns: [ColorStyle] = [
        ColorStyle(id: "violet", name: "Violet", primary: "#7D4DFA", secondary: "#ED549E"),
        ColorStyle(id: "ocean", name: "Ocean", primary: "#3378FF", secondary: "#33D1F2"),
        ColorStyle(id: "sunset", name: "Sunset", primary: "#FF7A45", secondary: "#F5487F"),
        ColorStyle(id: "forest", name: "Forest", primary: "#29B877", secondary: "#1A99B3"),
        ColorStyle(id: "rose", name: "Rose", primary: "#E84A8A", secondary: "#FF9EC4"),
        ColorStyle(id: "gold", name: "Gold", primary: "#F2A81D", secondary: "#FF6B3D"),
        ColorStyle(id: "neon", name: "Neon", primary: "#00D9A0", secondary: "#7B5CFF"),
        ColorStyle(id: "graphite", name: "Graphite", primary: "#5E6470", secondary: "#A2A8B2"),
    ]

    var isBuiltIn: Bool { ColorStyle.builtIns.contains(where: { $0.id == id }) }
}

// MARK: - Settings

struct Settings: Codable, Equatable {
    // воспроизведение
    var muted = true
    var volume: Double = 0.5
    var speed: Double = 1.0

    // автосмена
    var autoRotate = false
    var rotateMinutes = 30
    var shuffle = false

    // умная пауза
    var pauseWhenAppFocused = false
    var pauseWhenMaximized = true
    var pauseWhenFullscreen = true
    var pauseOnBattery = false
    var pauseInLowPower = true
    var pauseWhenHot = true
    var appRules: [AppRule] = []

    // внешний вид
    var colorStyleID = "violet"
    var customStyles: [ColorStyle] = []
    var windowBlur = true
    var blurAmount: Double = 1.0
    var windowTransparency: Double = 0.55

    // система
    var syncSystemWallpaper = true
    var launchAtLogin = false

    // хоткеи
    var numberHotkeys = true
    var hotkeys: [String: HotkeyBinding] = Settings.defaultHotkeys

    init() {}

    enum CodingKeys: String, CodingKey {
        case muted, volume, speed
        case autoRotate, rotateMinutes, shuffle
        case pauseWhenAppFocused, pauseWhenMaximized, pauseWhenFullscreen
        case pauseOnBattery, pauseInLowPower, pauseWhenHot, appRules
        case colorStyleID, customStyles, windowBlur, blurAmount, windowTransparency
        case syncSystemWallpaper, launchAtLogin
        case numberHotkeys, hotkeys
    }

    // терпимый к старым/неполным сохранениям декодер: отсутствующие ключи берутся по умолчанию
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = Settings()
        muted = (try? c.decodeIfPresent(Bool.self, forKey: .muted)) ?? d.muted
        volume = (try? c.decodeIfPresent(Double.self, forKey: .volume)) ?? d.volume
        speed = (try? c.decodeIfPresent(Double.self, forKey: .speed)) ?? d.speed
        autoRotate = (try? c.decodeIfPresent(Bool.self, forKey: .autoRotate)) ?? d.autoRotate
        rotateMinutes = (try? c.decodeIfPresent(Int.self, forKey: .rotateMinutes)) ?? d.rotateMinutes
        shuffle = (try? c.decodeIfPresent(Bool.self, forKey: .shuffle)) ?? d.shuffle
        pauseWhenAppFocused = (try? c.decodeIfPresent(Bool.self, forKey: .pauseWhenAppFocused)) ?? d.pauseWhenAppFocused
        pauseWhenMaximized = (try? c.decodeIfPresent(Bool.self, forKey: .pauseWhenMaximized)) ?? d.pauseWhenMaximized
        pauseWhenFullscreen = (try? c.decodeIfPresent(Bool.self, forKey: .pauseWhenFullscreen)) ?? d.pauseWhenFullscreen
        pauseOnBattery = (try? c.decodeIfPresent(Bool.self, forKey: .pauseOnBattery)) ?? d.pauseOnBattery
        pauseInLowPower = (try? c.decodeIfPresent(Bool.self, forKey: .pauseInLowPower)) ?? d.pauseInLowPower
        pauseWhenHot = (try? c.decodeIfPresent(Bool.self, forKey: .pauseWhenHot)) ?? d.pauseWhenHot
        appRules = (try? c.decodeIfPresent([AppRule].self, forKey: .appRules)) ?? d.appRules
        colorStyleID = (try? c.decodeIfPresent(String.self, forKey: .colorStyleID)) ?? d.colorStyleID
        customStyles = (try? c.decodeIfPresent([ColorStyle].self, forKey: .customStyles)) ?? d.customStyles
        windowBlur = (try? c.decodeIfPresent(Bool.self, forKey: .windowBlur)) ?? d.windowBlur
        blurAmount = (try? c.decodeIfPresent(Double.self, forKey: .blurAmount)) ?? d.blurAmount
        windowTransparency = (try? c.decodeIfPresent(Double.self, forKey: .windowTransparency)) ?? d.windowTransparency
        syncSystemWallpaper = (try? c.decodeIfPresent(Bool.self, forKey: .syncSystemWallpaper)) ?? d.syncSystemWallpaper
        launchAtLogin = (try? c.decodeIfPresent(Bool.self, forKey: .launchAtLogin)) ?? d.launchAtLogin
        numberHotkeys = (try? c.decodeIfPresent(Bool.self, forKey: .numberHotkeys)) ?? d.numberHotkeys
        hotkeys = (try? c.decodeIfPresent([String: HotkeyBinding].self, forKey: .hotkeys)) ?? d.hotkeys
    }

    static var defaultHotkeys: [String: HotkeyBinding] {
        let m = UInt32(controlKey | optionKey | cmdKey)
        return [
            HotkeyAction.next.rawValue: HotkeyBinding(keyCode: UInt32(kVK_RightArrow), modifiers: m),
            HotkeyAction.previous.rawValue: HotkeyBinding(keyCode: UInt32(kVK_LeftArrow), modifiers: m),
            HotkeyAction.random.rawValue: HotkeyBinding(keyCode: UInt32(kVK_ANSI_R), modifiers: m),
            HotkeyAction.togglePause.rawValue: HotkeyBinding(keyCode: UInt32(kVK_Space), modifiers: m),
            HotkeyAction.cycleMode.rawValue: HotkeyBinding(keyCode: UInt32(kVK_ANSI_M), modifiers: m),
            HotkeyAction.toggleMute.rawValue: HotkeyBinding(keyCode: UInt32(kVK_ANSI_S), modifiers: m),
            HotkeyAction.openLibrary.rawValue: HotkeyBinding(keyCode: UInt32(kVK_ANSI_L), modifiers: m),
        ]
    }
}

// MARK: - Hotkey formatting

enum HotkeyFormat {
    static func carbonMods(_ f: NSEvent.ModifierFlags) -> UInt32 {
        var m = 0
        if f.contains(.control) { m |= controlKey }
        if f.contains(.option) { m |= optionKey }
        if f.contains(.shift) { m |= shiftKey }
        if f.contains(.command) { m |= cmdKey }
        return UInt32(m)
    }

    static func string(_ b: HotkeyBinding) -> String {
        var s = ""
        if b.modifiers & UInt32(controlKey) != 0 { s += "⌃" }
        if b.modifiers & UInt32(optionKey) != 0 { s += "⌥" }
        if b.modifiers & UInt32(shiftKey) != 0 { s += "⇧" }
        if b.modifiers & UInt32(cmdKey) != 0 { s += "⌘" }
        s += KeyNames.table[b.keyCode] ?? "Key\(b.keyCode)"
        return s
    }
}

enum KeyNames {
    static let table: [UInt32: String] = [
        0: "A", 11: "B", 8: "C", 2: "D", 14: "E", 3: "F", 5: "G", 4: "H", 34: "I", 38: "J",
        40: "K", 37: "L", 46: "M", 45: "N", 31: "O", 35: "P", 12: "Q", 15: "R", 1: "S", 17: "T",
        32: "U", 9: "V", 13: "W", 7: "X", 16: "Y", 6: "Z",
        29: "0", 18: "1", 19: "2", 20: "3", 21: "4", 23: "5", 22: "6", 26: "7", 28: "8", 25: "9",
        123: "←", 124: "→", 125: "↓", 126: "↑",
        49: "Space", 36: "↩", 48: "⇥", 51: "⌫", 53: "⎋",
        122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6",
        98: "F7", 100: "F8", 101: "F9", 109: "F10", 103: "F11", 111: "F12",
        27: "-", 24: "=", 33: "[", 30: "]", 41: ";", 39: "'", 43: ",", 47: ".", 44: "/", 42: "\\", 50: "`",
    ]
}
