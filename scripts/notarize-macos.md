# macOS Signing & Notarization Guide

For production macOS builds, you must sign and notarize your application for it to run on other machines without security warnings.

## 1. Prerequisites

- An active **Apple Developer Program** membership.
- **Xcode** and the Xcode Command Line Tools installed.
- A **Developer ID Application** certificate and a **Developer ID Installer** certificate installed in your Keychain.

## 2. Environment Variables for CI/CD

You will need to configure the following secrets in your GitHub repository (`Settings -> Secrets and variables -> Actions`):

- `APPLE_ID`: Your Apple ID email address.
- `APPLE_API_KEY_ID`: The Key ID for your App Store Connect API Key.
- `APPLE_API_ISSUER`: The Issuer ID for your App Store Connect API Key.
- `APPLE_API_KEY_B64`: The private key file (`.p8`) for your API key, encoded in Base64.
  - Generate it with: `base64 -i AuthKey_XXXXXXXXXX.p8`
- `APPLE_CERTIFICATE_B64`: Your Developer ID Application certificate (`.p12`), encoded in Base64.
- `APPLE_CERTIFICATE_PASSWORD`: The password for your `.p12` certificate file.

## 3. Tauri Configuration

In `src-tauri/tauri.conf.json`, configure the bundle identifier and your developer certificate name:

```json
"tauri": {
  "bundle": {
    "identifier": "io.matrixhub.clihelper"
  },
  "macOS": {
    "signingIdentity": "Developer ID Application: Your Name (TEAMID)"
  }
}
```

## 4. GitHub Actions Workflow

The provided `release.yml` workflow includes steps for signing and notarization. Ensure your secrets are correctly configured for it to work.

See the official [Tauri Signing & Notarization Guide](https://tauri.app/v1/guides/distribution/sign-and-notarize) for more details.
