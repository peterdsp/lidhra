# App Store Connect (iOS)

How the Lidhra iOS app gets to App Store Connect, mirroring klipa's setup.

## One-time prerequisite: the app record

App Store Connect has no API to *create* an app record, so this is done once in
the web UI: https://appstoreconnect.apple.com/apps -> **+** -> **New App**

- Platform: **iOS**
- Name: **Lidhra**
- Primary Language: **English (U.S.)**
- Bundle ID: **dev.peterdsp.lidhra** (already registered)
- SKU: **lidhra-ios**, User Access: **Full Access**

Until this record exists, uploads fail with
`Cannot determine the Apple ID from Bundle ID 'dev.peterdsp.lidhra'`.

## Automatic upload

Two ways, both using the App Store Connect **API key** (key id `B3BD3SK79A`,
issuer `94b49788-ea57-4033-af26-dc5d362f185e`) so no Transporter clicking:

**Local, one command:**
```sh
./scripts/upload-appstore-ios.sh
```
Builds the signed IPA (with the `appstore` feature, no self-updater), then
`xcrun altool --validate-app` + `--upload-app`. The build shows up under
App Store Connect -> Lidhra -> TestFlight minutes later.

**CI on a version tag** (`.github/workflows/ios-appstore.yml`): builds + uploads
on every `v*` tag, gated so it skips until the signing secrets are set.

## Secrets (repo -> Settings -> Secrets and variables -> Actions)

| Secret | What | Status |
| --- | --- | --- |
| `ASC_KEY_ID` / `ASC_ISSUER_ID` / `ASC_API_KEY_P8_BASE64` | App Store Connect API key | **set** |
| `IOS_DIST_CERT_P12_BASE64` / `IOS_DIST_CERT_P12_PASSWORD` | .p12 of the "Apple Distribution" identity | needed for CI |
| `IOS_PROVISION_PROFILE_BASE64` | base64 of the App Store `.mobileprovision` | needed for CI |
| `CI_KEYCHAIN_PASSWORD` | throwaway keychain password | needed for CI |
| `TEAMID` | `YTS4KJBX3P` | needed for CI |

Export the cert + profile for the CI secrets:
```sh
# distribution cert as .p12 (Keychain Access -> export "Apple Distribution" incl. private key)
base64 -i AppleDistribution.p12 | pbcopy   # -> IOS_DIST_CERT_P12_BASE64
# the App Store profile the local build already created:
base64 -i ~/Library/MobileDevice/Provisioning\ Profiles/*.mobileprovision   # -> IOS_PROVISION_PROFILE_BASE64
```
The local script needs none of these secrets (it uses your Keychain directly).
