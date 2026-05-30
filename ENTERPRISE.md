# MatrixHub Client — Enterprise & Distribution Guide

This document covers professional packaging, deployment, code signing, and the
auto-update mechanism for **MatrixHub Client**.

---

## 1. Windows installers

The bundle produces two Windows artifacts:

| Artifact | Use |
|---|---|
| `MatrixHub Client_<ver>_x64-setup.exe` (NSIS) | Friendly consumer installer (Start-menu folder "MatrixHub", per-user or per-machine). |
| `MatrixHub Client_<ver>_x64_en-US.msi` (WiX) | Enterprise deployment (Intune / SCCM / Group Policy). |

The MSI uses a **fixed `upgradeCode`** (`819AD1D8-034E-47EB-9D09-AE83FEA024CC`) so
version bumps install as clean **upgrades** rather than duplicate apps. Do **not**
change this GUID across releases.

### Silent / unattended deployment

```powershell
# MSI (recommended for fleets)
msiexec /i "MatrixHub Client_0.2.0_x64_en-US.msi" /qn /norestart

# Per-machine, with a log
msiexec /i "MatrixHub Client_0.2.0_x64_en-US.msi" ALLUSERS=1 /qn /l*v install.log

# NSIS silent
"MatrixHub Client_0.2.0_x64-setup.exe" /S

# Uninstall (MSI)
msiexec /x "{819AD1D8-034E-47EB-9D09-AE83FEA024CC}" /qn
```

WebView2 is installed automatically via the download bootstrapper. For fully
offline fleets, switch `bundle.windows.webviewInstallMode` to `offlineInstaller`.

---

## 2. Code signing (removes SmartScreen warnings)

Unsigned apps trigger SmartScreen / "unknown publisher" warnings. Sign the
`.exe`, `.msi`, and the app binary with an **OV** or (best) **EV** certificate.

The config already sets `digestAlgorithm: sha256` and a timestamp URL. Provide a
certificate to the build via one of:

- **`certificateThumbprint`** in `tauri.conf.json > bundle.windows` (cert installed
  on the signing machine/runner), or
- **Azure Key Vault** / cloud HSM signing (recommended for EV).

In CI, add the certificate as a secret and reference it from the signing step.
See <https://v2.tauri.app/distribute/sign/windows/>.

macOS notarization is documented in `scripts/notarize-macos.md`.

---

## 3. Auto-update (premium in-app updater)

MatrixHub Client ships a built-in updater (Tauri updater plugin):

- On launch it checks the update endpoint in the background.
- When a newer **signed** release exists, the user sees a **"New version
  available"** toast → an update card with the changelog → **Update now**, which
  downloads with a live progress bar and relaunches into the new version.
- A manual **Settings → Diagnostics → Check for updates** is also available.

### Configuration (`tauri.conf.json > plugins.updater`)

```jsonc
"endpoints": ["https://www.matrixhub.io/releases/{{target}}/{{arch}}/{{current_version}}"],
"pubkey":    "<minisign public key>",
"windows":   { "installMode": "passive" }
```

The server returns JSON like:

```json
{
  "version": "0.3.0",
  "notes": "What's new …",
  "pub_date": "2026-06-01T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<content of the .sig file>",
      "url": "https://www.matrixhub.io/downloads/MatrixHub-Client_0.3.0_x64-setup.nsis.zip"
    }
  }
}
```

### Signing keys (required — signatures cannot be disabled)

1. Generate a keypair (already done once for the bundled dev pubkey):
   ```bash
   npx tauri signer generate -w ~/.matrixhub/updater.key
   ```
2. Put the **public** key in `tauri.conf.json > plugins.updater.pubkey`.
   > ⚠️ The pubkey currently committed is a **development placeholder**. For
   > production, generate your own keypair and replace it.
3. Keep the **private** key secret. In CI set:
   - `TAURI_SIGNING_PRIVATE_KEY`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

### Producing updater artifacts at release time

Updater artifacts (`*.sig` + the update archive) are only emitted when
`createUpdaterArtifacts` is enabled. It is intentionally **off** in the committed
config so normal builds don't require the signing key. Enable it at release time:

```bash
npm run tauri build -- --config "{\"bundle\":{\"createUpdaterArtifacts\":true}}"
# with TAURI_SIGNING_PRIVATE_KEY[_PASSWORD] set in the environment
```

Then publish the generated `latest.json` (or per-target JSON) at the endpoint URL
above, alongside the signed installers.

---

## 4. Self-contained runtime (managed venv)

To avoid system-Python / PATH / Microsoft-Store-alias / pipx problems, the client
provisions and owns an **isolated Python virtual environment**:

- Location: `<app_data_dir>/runtime/.venv` (per-user, survives upgrades).
- `matrix-cli` is installed into it (`python -m venv` + `pip install matrix-cli`),
  and the app invokes `matrix` by **absolute path** from that venv — never relying
  on PATH. The real terminal also prepends the venv's `bin`/`Scripts` to PATH.
- A real system Python is only needed once, to *create* the venv; if it's missing
  the app installs it via winget/Homebrew (python.org fallback).

Supportability:

- **First-run setup wizard** provisions the runtime with a visible log.
- **Settings → Diagnostics → Reset Matrix CLI** deletes and recreates the venv.
- **Export logs** / **Open data folder**, plus a persistent `client.log` recording
  every command + output + exit code.
- A stable **Install ID** (Settings → About) to correlate support cases.

For a *zero-Python* install, the next step is to ship `matrix-cli` as a **bundled
sidecar binary** (PyInstaller) via `bundle.externalBin`.

---

## 5. Branding

The single public product name is **MatrixHub Client** (publisher: *MatrixHub*).
Internal crate/package name `matrix-protocol-helper` is never shown to users.
