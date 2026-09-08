import AVFoundation
import AVKit
import Network
import Tauri
import UIKit
import WebKit

struct PathArgs: Decodable {
  let path: String
}

struct PlayArgs: Decodable {
  let url: String
  let title: String?
}

struct UrlArgs: Decodable {
  let url: String
}

// MARK: - Lifecycle bridge (Swift -> Rust)

/// `lidhra_native_event` is a plain C function exported by the app's Rust
/// static library (see `lib.rs`); the linker resolves it at build time.
/// kind 1 = app state (0 suspending, 1 foreground, 2 entered background),
/// kind 2 = network (0 Wi-Fi or wired, 1 cellular or expensive, 2 offline),
/// kind 3 = Low Power Mode (0 off, 1 on).
@_silgen_name("lidhra_native_event")
private func lidhraNativeEvent(_ kind: Int32, _ value: Int32)

/// Feeds the on-device torrent engine what it cannot observe itself: when iOS
/// is about to freeze the process, when the app is back, which network path
/// is active, and whether Low Power Mode is on.
///
/// Background policy: on `didEnterBackground` the app asks for the extra
/// seconds iOS grants (`beginBackgroundTask`) so in-flight pieces finish. When
/// that time is up (or the app is terminated) the engine is told to pause and
/// save; on `willEnterForeground` it resumes. There is no background mode and
/// none is claimed: a P2P download only runs while Lidhra is open.
final class LifecycleBridge {
  private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
  private var observers: [NSObjectProtocol] = []
  private let pathMonitor = NWPathMonitor()
  private let pathQueue = DispatchQueue(label: "dev.peterdsp.lidhra.network-path")
  private var started = false

  func start() {
    guard !started else { return }
    started = true
    let center = NotificationCenter.default
    observers.append(
      center.addObserver(forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main) {
        [weak self] _ in self?.didEnterBackground()
      })
    observers.append(
      center.addObserver(forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main) {
        [weak self] _ in self?.willEnterForeground()
      })
    observers.append(
      center.addObserver(forName: UIApplication.willTerminateNotification, object: nil, queue: .main) { _ in
        // Last chance to pause cleanly and flush the session file.
        lidhraNativeEvent(1, 0)
      })
    observers.append(
      center.addObserver(forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main) { _ in
        LifecycleBridge.reportPowerState()
      })
    LifecycleBridge.reportPowerState()

    pathMonitor.pathUpdateHandler = { path in
      let value: Int32
      if path.status != .satisfied {
        value = 2
      } else if path.isExpensive || path.usesInterfaceType(.cellular) {
        value = 1
      } else {
        value = 0
      }
      lidhraNativeEvent(2, value)
    }
    pathMonitor.start(queue: pathQueue)
  }

  deinit {
    observers.forEach(NotificationCenter.default.removeObserver)
    pathMonitor.cancel()
  }

  private static func reportPowerState() {
    lidhraNativeEvent(3, ProcessInfo.processInfo.isLowPowerModeEnabled ? 1 : 0)
  }

  private func didEnterBackground() {
    endBackgroundTask()
    backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "dev.peterdsp.lidhra.p2p") {
      [weak self] in
      // Background time is up: the engine pauses and saves before the freeze.
      // Never hold a torrent when the app is already back on screen.
      if UIApplication.shared.applicationState != .active {
        lidhraNativeEvent(1, 0)
      }
      self?.endBackgroundTask()
    }
    lidhraNativeEvent(1, 2)
  }

  private func willEnterForeground() {
    endBackgroundTask()
    lidhraNativeEvent(1, 1)
  }

  private func endBackgroundTask() {
    guard backgroundTask != .invalid else { return }
    UIApplication.shared.endBackgroundTask(backgroundTask)
    backgroundTask = .invalid
  }
}

// MARK: - Plugin

/// Lidhra's native bridge. Every command is invoked from the app's Rust
/// commands (never straight from JS), and every UI presentation happens on
/// the main thread from the webview's root view controller.
class NativePlugin: Plugin {
  // UIDocumentInteractionController must stay alive while its menu is shown.
  private var docController: UIDocumentInteractionController?
  private let lifecycle = LifecycleBridge()

  override func load(webview: WKWebView) {
    super.load(webview: webview)
    lifecycle.start()
  }

  private func onMain(_ block: @escaping () -> Void) {
    if Thread.isMainThread { block() } else { DispatchQueue.main.async(execute: block) }
  }

  /// Anchor for popovers on iPad: bottom-centre of the window.
  private func anchor(in view: UIView) -> CGRect {
    return CGRect(x: view.bounds.midX, y: view.bounds.maxY - 120, width: 1, height: 1)
  }

  /// System share sheet: AirDrop, Messages, Mail, Save to Files, "Copy to VLC", …
  @objc public func share(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PathArgs.self)
    guard FileManager.default.fileExists(atPath: args.path) else {
      invoke.reject("file not found")
      return
    }
    let fileUrl = URL(fileURLWithPath: args.path)
    onMain {
      guard let vc = self.manager.viewController else {
        invoke.reject("no view controller")
        return
      }
      let sheet = UIActivityViewController(activityItems: [fileUrl], applicationActivities: nil)
      if let pop = sheet.popoverPresentationController {
        pop.sourceView = vc.view
        pop.sourceRect = self.anchor(in: vc.view)
        pop.permittedArrowDirections = []
      }
      vc.present(sheet, animated: true)
      invoke.resolve()
    }
  }

  /// "Open in…" menu: only the apps that can open this file type (players, editors).
  @objc public func openIn(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PathArgs.self)
    guard FileManager.default.fileExists(atPath: args.path) else {
      invoke.reject("file not found")
      return
    }
    let fileUrl = URL(fileURLWithPath: args.path)
    onMain {
      guard let vc = self.manager.viewController else {
        invoke.reject("no view controller")
        return
      }
      let dic = UIDocumentInteractionController(url: fileUrl)
      self.docController = dic
      let shown = dic.presentOpenInMenu(from: self.anchor(in: vc.view), in: vc.view, animated: true)
      if shown {
        invoke.resolve()
      } else {
        self.docController = nil
        invoke.reject("no installed app can open this file")
      }
    }
  }

  /// Native full-screen player (AirPlay, Picture in Picture, scrubbing for free).
  @objc public func play(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PlayArgs.self)
    let url: URL?
    if args.url.hasPrefix("/") {
      url = URL(fileURLWithPath: args.url)
    } else {
      url = URL(string: args.url)
    }
    guard let mediaUrl = url else {
      invoke.reject("invalid media url")
      return
    }
    onMain {
      guard let vc = self.manager.viewController else {
        invoke.reject("no view controller")
        return
      }
      try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback)
      try? AVAudioSession.sharedInstance().setActive(true)
      let item = AVPlayerItem(url: mediaUrl)
      if let title = args.title, !title.isEmpty {
        let meta = AVMutableMetadataItem()
        meta.identifier = .commonIdentifierTitle
        meta.value = title as NSString
        meta.extendedLanguageTag = "und"
        item.externalMetadata = [meta]
      }
      let player = AVPlayer(playerItem: item)
      let controller = AVPlayerViewController()
      controller.player = player
      controller.allowsPictureInPicturePlayback = true
      controller.modalPresentationStyle = .fullScreen
      vc.present(controller, animated: true) {
        player.play()
      }
      invoke.resolve()
    }
  }

  /// `canOpenURL` probe. Schemes must be listed under LSApplicationQueriesSchemes.
  @objc public func canOpen(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(UrlArgs.self)
    guard let url = URL(string: args.url) else {
      invoke.resolve(["ok": false])
      return
    }
    onMain {
      invoke.resolve(["ok": UIApplication.shared.canOpenURL(url)])
    }
  }
}

@_cdecl("init_plugin_lidhra_native")
func initPlugin() -> Plugin {
  return NativePlugin()
}
