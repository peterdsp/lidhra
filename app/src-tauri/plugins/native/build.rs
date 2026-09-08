// The plugin exposes a Rust API only (the app's own commands call into it), so
// no JS-facing commands and no permissions are generated. On iOS the build
// links the Swift package in `ios/`.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
