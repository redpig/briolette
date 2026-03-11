import SwiftUI

@main
struct BrioletteApp: App {
    init() {
        // Wire the Swift FFI delegate into the KMP wallet bridge
        // before the Compose UI is created.
        installWalletBridge()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
