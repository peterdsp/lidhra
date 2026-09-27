// Lidhra for HarmonyOS: an ArkWeb shell over the shared web UI (ui/index.html),
// bundled into resources/rawfile by `scripts/tv/build.sh harmonyos` together with
// the lidhra-bridge wasm module. The page runs in TV mode (tv-boot.js) and talks to
// the debrid provider directly, so nothing else runs natively.
import { webview } from '@kit.ArkWeb';
import { common } from '@kit.AbilityKit';

// Exposed to the page as `window.LidhraHarmony`; the TV UI calls exit() when Back
// is pressed on its first screen.
class LidhraHarmony {
  private ctx: common.UIAbilityContext;

  constructor(ctx: common.UIAbilityContext) {
    this.ctx = ctx;
  }

  exit(): void {
    this.ctx.terminateSelf();
  }
}

@Entry
@Component
struct Index {
  controller: webview.WebviewController = new webview.WebviewController();
  private native: LidhraHarmony = new LidhraHarmony(getContext(this) as common.UIAbilityContext);

  // The web UI owns Back (screens, player); it exits the app from the first screen.
  onBackPress(): boolean {
    this.controller.runJavaScript('typeof tvBack === "function" && tvBack()');
    return true;
  }

  build() {
    Column() {
      Web({ src: $rawfile('index.html'), controller: this.controller })
        .javaScriptAccess(true)
        .domStorageAccess(true)
        .fileAccess(true)
        .mediaPlayGestureAccess(false)
        .javaScriptProxy({
          object: this.native,
          name: 'LidhraHarmony',
          methodList: ['exit'],
          controller: this.controller
        })
        .width('100%')
        .height('100%')
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#07110e')
  }
}
