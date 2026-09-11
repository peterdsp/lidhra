import AVFoundation
import AVKit
import Network
import Tauri
import UIKit
import WebKit

/// An optional presentation anchor, in the webview's own CSS-pixel space (which
/// equals its UIKit points at the default zoom). Sent by the web UI as the
/// bounding box of the control that started a share / Open In, so the popover
/// points at that control instead of a fixed corner.
struct AnchorRect: Decodable {
  let x: CGFloat
  let y: CGFloat
  let w: CGFloat
  let h: CGFloat
}

struct PathArgs: Decodable {
  let path: String
  let anchor: AnchorRect?
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
/// none is claimed: a P2P download only runs while Lidhra is open. Opening the
/// device (an iPhone Duo geometry change) does not change this: geometry is
/// handled by `GeometryBridge` and never touches the engine.
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

// MARK: - Reserved-region provider (iPhone Duo hinge / active division)

/// The active reserved regions of the current scene: on iPhone Duo, the strip
/// occluded by the fold when the display is divided. Each region is reported in
/// the scene's coordinate space (the same space the webview frame is reported
/// in) as `["x","y","w","h"]` points plus a `"kind"` tag, so the web UI can
/// convert them to CSS pixels and lay the two work areas out around the fold.
///
/// On every currently shippable SDK there is no such API, so this returns an
/// empty list and the web UI falls back to an ordinary responsive layout, never
/// a guessed hinge. The real implementation is isolated behind the
/// `LIDHRA_DUO` compilation condition (see docs/adaptive-layout.md): an
/// availability check alone cannot make an unknown symbol compile against the
/// iOS 26 SDK this project builds with today, so the Duo geometry query is
/// enabled only in an Xcode 27.1 build that defines `LIDHRA_DUO`. Enabling it
/// changes nothing for any other configuration.
enum DuoRegions {
  static func active(in view: UIView) -> [[String: Any]] {
    #if LIDHRA_DUO
    // INTEGRATION POINT (Xcode 27.1 + iPhone Duo SDK):
    // Query the scene's active division / reserved regions from the arrangement
    // geometry, map each rectangle into `view.window` (scene) coordinates, and
    // return one dictionary per region:
    //   ["x": r.minX, "y": r.minY, "w": r.width, "h": r.height, "kind": "hinge"]
    // Convert with the confirmed API only; do not fabricate a symbol here.
    #warning("LIDHRA_DUO: implement the reserved-region query against the confirmed iPhone Duo SDK before shipping Duo support.")
    return []
    #else
    _ = view
    return []
    #endif
  }
}

// MARK: - Geometry bridge (Swift -> web)

/// Pushes the scene geometry the shared web UI needs for its adaptive layout:
/// the scene bounds, the webview's own frame within the scene, each safe-area
/// inset, the keyboard overlap, and any active reserved regions. Delivered as a
/// small versioned JSON snapshot through `window.__lidhraGeometry`, coalesced to
/// the main run loop, each carrying a strictly increasing revision so the web
/// side can drop a stale or replayed event.
///
/// This only describes geometry; it never touches transfers, playback, or the
/// engine. Observers are torn down in `stop()` / `deinit`.
final class GeometryBridge {
  private weak var webview: WKWebView?
  private var observers: [NSObjectProtocol] = []
  private var boundsObservation: NSKeyValueObservation?
  private var revision: Int = 0
  private var keyboardInset: CGFloat = 0
  private var scheduled = false

  func start(webview: WKWebView) {
    self.webview = webview
    let center = NotificationCenter.default
    // Rotation, foreground, and keyboard are the geometry-affecting events an
    // ordinary iOS build can observe. On iPhone Duo the open/close/fold and
    // Split-View resizes also change the webview's bounds, which the KVO
    // observation below catches.
    observers.append(center.addObserver(forName: UIDevice.orientationDidChangeNotification, object: nil, queue: .main) {
      [weak self] _ in self?.schedule()
    })
    observers.append(center.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) {
      [weak self] _ in self?.schedule()
    })
    observers.append(center.addObserver(forName: UIResponder.keyboardWillChangeFrameNotification, object: nil, queue: .main) {
      [weak self] note in self?.keyboardChanged(note)
    })
    observers.append(center.addObserver(forName: UIResponder.keyboardWillHideNotification, object: nil, queue: .main) {
      [weak self] _ in self?.keyboardInset = 0; self?.schedule()
    })
    boundsObservation = webview.observe(\.bounds, options: [.new]) { [weak self] _, _ in
      self?.schedule()
    }
    schedule()
  }

  func stop() {
    observers.forEach(NotificationCenter.default.removeObserver)
    observers.removeAll()
    boundsObservation?.invalidate()
    boundsObservation = nil
  }

  deinit { stop() }

  private func keyboardChanged(_ note: Notification) {
    guard let webview = webview,
      let frame = (note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? NSValue)?.cgRectValue
    else { return }
    // How much of the webview the keyboard covers, in the webview's own space.
    let kb = webview.convert(frame, from: webview.window)
    keyboardInset = max(0, webview.bounds.maxY - kb.minY)
    schedule()
  }

  /// Coalesce a burst of events into one emit on the next main-loop hop.
  private func schedule() {
    guard !scheduled else { return }
    scheduled = true
    DispatchQueue.main.async { [weak self] in
      self?.scheduled = false
      self?.emit()
    }
  }

  private func emit() {
    guard let webview = webview, let window = webview.window else { return }
    let scene = window.windowScene?.coordinateSpace.bounds ?? window.bounds
    let frame = webview.convert(webview.bounds, to: window)
    let safe = webview.safeAreaInsets
    revision += 1

    let payload: [String: Any] = [
      "v": 1,
      "rev": revision,
      "coordinateSpace": "scene",
      "scene": ["w": scene.width, "h": scene.height],
      "webview": ["x": frame.minX, "y": frame.minY, "w": frame.width, "h": frame.height],
      "safeArea": ["top": safe.top, "right": safe.right, "bottom": safe.bottom, "left": safe.left],
      "keyboard": keyboardInset,
      "regions": DuoRegions.active(in: webview),
    ]

    guard
      let data = try? JSONSerialization.data(withJSONObject: payload, options: []),
      let json = String(data: data, encoding: .utf8)
    else { return }
    // The web entry point is a no-op on any page that does not define it, so
    // this is safe even during an early load.
    webview.evaluateJavaScript("window.__lidhraGeometry && window.__lidhraGeometry(\(json))", completionHandler: nil)
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
  private let geometry = GeometryBridge()
  private weak var webview: WKWebView?

  override func load(webview: WKWebView) {
    super.load(webview: webview)
    self.webview = webview
    lifecycle.start()
    geometry.start(webview: webview)
  }

  private func onMain(_ block: @escaping () -> Void) {
    if Thread.isMainThread { block() } else { DispatchQueue.main.async(execute: block) }
  }

  /// Popover anchor. Prefer the control the web UI reported (its box is in the
  /// webview's coordinate space); fall back to the bottom-centre of the view.
  /// Returns the view the rect is relative to and the rect itself.
  private func anchor(_ a: AnchorRect?, fallbackIn view: UIView) -> (UIView, CGRect, UIPopoverArrowDirection) {
    if let a = a, a.w >= 0, a.h >= 0, let wv = self.webview {
      let rect = CGRect(x: a.x, y: a.y, width: max(a.w, 1), height: max(a.h, 1))
      return (wv, rect, .any)
    }
    return (view, CGRect(x: view.bounds.midX, y: view.bounds.maxY - 120, width: 1, height: 1), [])
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
        let (view, rect, dir) = self.anchor(args.anchor, fallbackIn: vc.view)
        pop.sourceView = view
        pop.sourceRect = rect
        pop.permittedArrowDirections = dir
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
      let (view, rect, _) = self.anchor(args.anchor, fallbackIn: vc.view)
      let shown = dic.presentOpenInMenu(from: rect, in: view, animated: true)
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
