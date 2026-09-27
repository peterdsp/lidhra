# lidhra-cast

Cast a direct video URL to a TV that cannot run a Lidhra app. On a TV, Lidhra is a library
plus player; for sets without an app (Vizio SmartCast, Roku, older Panasonic, and the many
Samsung / LG / Sony TVs that expose a DLNA renderer), Lidhra hands the TV a stream URL that
a debrid provider already resolved, and the TV plays it with its own player.

## Supported devices

| Kind   | Protocol                                                  | Typical devices                                                   |
|--------|-----------------------------------------------------------|-------------------------------------------------------------------|
| `dlna` | SSDP + UPnP AV `AVTransport:1` (SOAP over HTTP)           | Samsung, LG, Sony, Panasonic, Philips TVs; AV receivers; Kodi     |
| `roku` | SSDP `roku:ecp` + External Control Protocol (HTTP, 8060)  | Roku players and Roku TVs (TCL, Hisense, ...), via "Play on Roku" |

Discovery sends SSDP `M-SEARCH` for `urn:schemas-upnp-org:device:MediaRenderer:1` and
`roku:ecp` (each twice, since UDP is lossy), then reads each DLNA device description
(friendly name, model, AVTransport control URL) or Roku `query/device-info`.

Playing:

- **DLNA**: `SetAVTransportURI` (URL plus DIDL-Lite metadata with title, MIME type and
  `object.item.videoItem`) then `Play`. A renderer that refuses the new URI gets a `Stop` and
  one retry; a `Play` fault is retried once after a second (some TVs are still loading).
  `stop` sends `Stop`. SOAP faults surface as `CastError::Soap { code, description }`.
- **Roku**: `POST /input/15985?t=v&u=<url>&videoName=<title>&videoFormat=<fmt>` (the built-in
  Play on Roku channel), falling back to `launch/15985` with the same query. `videoFormat`
  (`mp4`, `mkv`, `hls`, `dash`) comes from the MIME type or the URL extension and is left out
  when unknown. `stop` sends the `Back` key, which leaves the player without also closing
  whatever channel the viewer was in (`Home` would).

## Limitations

- **Same LAN only.** SSDP is link-local multicast; the TV and the Lidhra host must share a
  network (and multicast must not be filtered, which some guest / mesh Wi-Fi setups do).
- **The TV fetches the URL itself.** It must be reachable from the TV and must not need auth
  headers or cookies. Debrid direct links are plain HTTPS with the token in the URL, so they
  usually work. Links bound to the requesting IP only work if the TV shares that public IP.
- **Codecs are the TV's.** A TV plays what its own player supports; MKV with DTS audio, for
  example, fails on many sets. Failures show up as a SOAP fault or as the TV's own error.
- **Roku "Control by mobile apps"** must be enabled (Settings > System > Advanced system
  settings); otherwise ECP answers HTTP 403.
- **Not covered:** Chromecast (Cast v2 is protobuf over TLS with its own receiver apps) and
  AirPlay are intentionally out of scope for this crate.
- Discovery only follows a `LOCATION` on the host that sent the SSDP answer, so a spoofed
  reply cannot redirect it to another machine or to localhost.

## API

```rust
pub enum Kind { Dlna, Roku }              // serde: "dlna" / "roku"

pub struct Device {
    pub id: String,                       // SSDP USN uuid, stable per device
    pub name: String,
    pub kind: Kind,
    pub location: String,                 // DLNA description URL, or Roku base URL
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub control_url: Option<String>,      // DLNA AVTransport control URL (re-read if missing)
}

pub struct Media { pub url: String, pub title: String, pub mime: Option<String> }
impl Media { pub fn new(url, title) -> Media }

pub async fn discover(timeout: Duration) -> Result<Vec<Device>>;
pub async fn play(device: &Device, media: &Media) -> Result<()>;
pub async fn stop(device: &Device) -> Result<()>;

pub enum CastError { Io, Http, Status { status, body }, Soap { action, code, description },
                     NoAvTransport, InvalidDevice, InvalidMedia }
```

`Device`, `Kind` and `Media` are `Serialize` / `Deserialize`, so a UI can list devices, keep
the JSON, and send one back to `play` without rediscovering. `discover` listens for
`timeout`, then waits for the per-device lookups (a few seconds at most each).

## Try it

```sh
cargo run -p lidhra-cast --example cast -- discover
cargo run -p lidhra-cast --example cast -- play 0 "https://example.com/movie.mp4" "Movie"
cargo run -p lidhra-cast --example cast -- stop 0
```
