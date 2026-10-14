import Foundation

public struct EngineStatus: Equatable, Sendable {
    public var id: String?
    public var path: String?
    public var playing: Bool
    public var paused: Bool
    public var muted: Bool
    public var lowPower: Bool
    public var ruleAction: RuleAction
    public var index: Int
    public var itemCount: Int

    public init(
        id: String? = nil,
        path: String? = nil,
        playing: Bool = false,
        paused: Bool = false,
        muted: Bool = false,
        lowPower: Bool = false,
        ruleAction: RuleAction = .keepRunning,
        index: Int = -1,
        itemCount: Int = 0
    ) {
        self.id = id
        self.path = path
        self.playing = playing
        self.paused = paused
        self.muted = muted
        self.lowPower = lowPower
        self.ruleAction = ruleAction
        self.index = index
        self.itemCount = itemCount
    }
}

extension RuleAction {
    public var ipcName: String {
        switch self {
        case .keepRunning:
            return "keep-running"
        case .pause:
            return "pause"
        case .stop:
            return "stop"
        }
    }
}
