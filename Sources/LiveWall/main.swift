import AppKit

// Перекодирование из командной строки (для проверки): LiveWall --convert <вход> <выход.mov>
let arguments = CommandLine.arguments
if arguments.count >= 4 && arguments[1] == "--convert" {
    let source = URL(fileURLWithPath: arguments[2])
    let dest = URL(fileURLWithPath: arguments[3])
    Task.detached {
        do {
            let native = await VideoConverter.canPlayNatively(source)
            print("native: \(native)")
            try await VideoConverter.convert(source: source, dest: dest, fpsLimit: 30,
                                             target: CGSize(width: 1920, height: 1080),
                                             token: CancelToken()) { _ in }
            if let info = await VideoOptimizer.info(for: dest) {
                print("output: \(info.summary), \(String(format: "%.2f", info.duration)) s")
            }
        } catch {
            print("error: \(error.localizedDescription)")
            exit(1)
        }
        exit(0)
    }
    dispatchMain()
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
