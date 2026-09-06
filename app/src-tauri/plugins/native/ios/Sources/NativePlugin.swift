import AVFoundation
import AVKit
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

/// Lidhra's native bridge. Every command is invoked from the app's Rust
/// commands (never straight from JS), and every UI presentation happens on
/// the main thread from the webview's root view controller.
class NativePlugin: Plugin {
  // UIDocumentInteractionController must stay alive while its menu is shown.
  private var docController: UIDocumentInteractionController?

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
