import SwiftUI
import shared  // KMP shared framework

/// Main iOS content view wrapping the Compose Multiplatform UI.
///
/// The shared KMP module provides `MainViewControllerKt.MainViewController()`
/// which renders the full Compose UI (same screens as Android) inside a
/// UIKit UIViewController.
struct ContentView: View {
    var body: some View {
        ComposeView()
            .ignoresSafeArea(.all)
    }
}

/// Bridges the KMP Compose UIViewController into SwiftUI.
struct ComposeView: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        MainViewControllerKt.MainViewController()
    }

    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}

#Preview {
    ContentView()
}
