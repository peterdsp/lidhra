//! Try casting from the command line (same LAN as the TV):
//!
//! ```text
//! cargo run -p lidhra-cast --example cast -- discover
//! cargo run -p lidhra-cast --example cast -- play <index> <url> [title]
//! cargo run -p lidhra-cast --example cast -- stop <index>
//! ```
//!
//! `<index>` is the position printed by `discover` (discovery runs again for each command).

use lidhra_cast::{discover, play, stop, Device, Media};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage: cast discover | cast play <index> <url> [title] | cast stop <index>";

async fn pick(index: Option<&String>) -> Result<Device, String> {
    let index: usize = index.and_then(|i| i.parse().ok()).ok_or(USAGE)?;
    let mut devices = discover(Duration::from_secs(3)).await.map_err(|e| e.to_string())?;
    if index >= devices.len() {
        return Err(format!("no device #{index} ({} found)", devices.len()));
    }
    Ok(devices.swap_remove(index))
}

async fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("discover") => {
            let devices = discover(Duration::from_secs(3)).await.map_err(|e| e.to_string())?;
            if devices.is_empty() {
                println!("no DLNA renderers or Roku devices answered");
            }
            for (i, d) in devices.iter().enumerate() {
                let model = [d.manufacturer.as_deref(), d.model.as_deref()].into_iter().flatten().collect::<Vec<_>>();
                println!("{i}: [{:?}] {} ({}) {}", d.kind, d.name, model.join(" "), d.location);
            }
            Ok(())
        }
        Some("play") => {
            let device = pick(args.get(1)).await?;
            let url = args.get(2).ok_or(USAGE)?;
            let title = args.get(3).cloned().unwrap_or_else(|| "Lidhra".to_string());
            play(&device, &Media::new(url.as_str(), title)).await.map_err(|e| e.to_string())?;
            println!("playing on {}", device.name);
            Ok(())
        }
        Some("stop") => {
            let device = pick(args.get(1)).await?;
            stop(&device).await.map_err(|e| e.to_string())?;
            println!("stopped {}", device.name);
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
