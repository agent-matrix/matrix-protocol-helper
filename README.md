# Matrix Protocol Helper

*A tiny, secure desktop utility that enables one-click installations from your browser to the Matrix CLI.*

[![Build Status](https://github.com/agent-matrix/matrix-protocol-helper/actions/workflows/release.yml/badge.svg)](https://github.com/agent-matrix/matrix-protocol-helper/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Latest Release](https://img.shields.io/github/v/release/agent-matrix/matrix-protocol-helper)](https://github.com/agent-matrix/matrix-protocol-helper/releases/latest)

---

The Matrix Protocol Helper is the essential bridge between your web browser and your local Matrix CLI. It registers the `matrix://` custom URL scheme, allowing you to install agents and servers from the Hub with a single click—while prioritizing security through mandatory user consent.


---

## What it does (in plain words)

The Matrix Protocol Helper is the bridge between your web browser and your local Matrix CLI.

* It registers the special link type **`matrix://`** on your computer.
* When you click an **Install** button on the Matrix Hub website, the app:

  1. asks for your **permission**,
  2. runs the safe **install command** for you, and
  3. shows clear **progress** and **results**.
* If the Matrix CLI isn’t installed yet, it will **guide you** to install it.

---

## Safety at a glance

* **You stay in control:** Every install requires your explicit confirmation in a native dialog.
* **No risky commands:** The app prevents command-injection by sending parameters **directly** to the Matrix CLI (no shell).
* **Minimal permissions:** It only handles `matrix://install` links and nothing else.
* **Transparent:** Live logs show exactly what happened (success or failure).

---

## System requirements

* **Windows** 10/11 (64-bit)
* **macOS** 12+ (Intel or Apple Silicon)
* **Linux**: a recent 64-bit distribution

  * Debian/Ubuntu (use **`.deb`**)
  * Fedora/RHEL/openSUSE (use **`.rpm`**)

> If your computer is very old or uses a different CPU, see **Build from Source** below.

---

## Download

Get the correct installer for your operating system from the **Latest Release** page:

**➡ [https://github.com/agent-matrix/matrix-protocol-helper/releases/latest](https://github.com/agent-matrix/matrix-protocol-helper/releases/latest)**

On that page, download one of:

* **Windows**: `MatrixProtocolHelper-x64.msi`
* **macOS (Universal)**: `MatrixProtocolHelper.dmg`
* **Linux (Debian/Ubuntu)**: `matrix-protocol-helper_*.deb`
* **Linux (Fedora/RHEL/openSUSE)**: `matrix-protocol-helper-*.rpm`

---

## Install instructions (step by step)

### Windows (MSI)

1. **Download** the `.msi` file.
2. **Double-click** it and follow the prompts: **Next → Install → Finish**.
3. If Windows asks for permission, choose **Yes**.
4. Done — the app runs quietly in the background and registers `matrix://` links.

**Uninstall:**
**Settings → Apps → Installed apps**, search **Matrix Protocol Helper**, click **Uninstall**.

**Verify the download (recommended):**

```powershell
Get-FileHash .\MatrixProtocolHelper-x64.msi -Algorithm SHA256
```

Compare the value with the SHA-256 checksum on the release page.

---

### macOS (DMG)

1. **Download** the `.dmg` file and **open** it.
2. **Drag & drop** **Matrix Protocol Helper** into **Applications**.
3. Open **Applications**, **right-click** the app → **Open** (first run to approve).
4. Confirm any macOS prompts. The app registers `matrix://` links.

**Uninstall:**
Drag **Matrix Protocol Helper** from **Applications** to **Trash**.

**If macOS warns that the app is from an unidentified developer:**
Right-click → **Open** → **Open** to confirm you trust it.

**Verify the download (recommended):**

```bash
shasum -a 256 ~/Downloads/MatrixProtocolHelper.dmg
```

Compare with the SHA-256 shown on the release page.

---

### Linux — Debian & Ubuntu (`.deb`)

**Option A (GUI):** Double-click the `.deb` and install with your Software Center.

**Option B (Terminal):**

```bash
sudo apt install ./matrix-protocol-helper_*.deb
# If apt suggests fixing dependencies:
sudo apt -f install
```

**Uninstall:**

```bash
sudo apt remove matrix-protocol-helper
```

**Verify the download (recommended):**

```bash
sha256sum matrix-protocol-helper_*.deb
```

Compare with the SHA-256 on the release page.

---

### Linux — Fedora, RHEL, openSUSE (`.rpm`)

**Option A (GUI):** Double-click the `.rpm` and install with your Software app.

**Option B (Terminal):**

```bash
# Fedora / RHEL (dnf)
sudo dnf install ./matrix-protocol-helper-*.rpm

# openSUSE (zypper)
sudo zypper install ./matrix-protocol-helper-*.rpm
```

**Uninstall:**

```bash
sudo dnf remove matrix-protocol-helper
# or
sudo zypper remove matrix-protocol-helper
```

**Verify the download (recommended):**

```bash
sha256sum matrix-protocol-helper-*.rpm
```

Compare with the SHA-256 on the release page.

---

## First-time test (about 2 minutes)

1. **Open the app** (Windows: Start menu; macOS: Applications; Linux: app launcher).
   The app runs in the background.
2. On the Matrix Hub website, click an **Install** button (a `matrix://install?...` link).
   Your browser may ask to open **Matrix Protocol Helper** → choose **Allow**.
3. A **confirmation** window appears, showing exactly what will be installed.
   Click **Yes** to continue.
4. Watch the **live log** until it says **Success** or shows an error.

> If nothing happens when you click an install link, see **Troubleshooting**.

---

## Everyday use

* You don’t need to open the app manually.
  Just click **Install from Hub** links — the app appears when needed.
* You will **always** see a clear confirmation before anything runs.
* The app only activates for `matrix://install` links.

---

## Using the Matrix CLI (optional)

If you prefer the command line or are troubleshooting, you can install with the Matrix CLI directly.

**Install the CLI (recommended via pipx):**

```bash
pipx install matrix-cli
```

> If `pipx` isn’t found, install Python (from python.org), then:
>
> ```bash
> python -m pip install --user pipx
> pipx ensurepath
> ```

**Example install command:**

```bash
matrix install mcp_server:hello-sse-server@0.1.0 --alias hello
```

---

## Troubleshooting

**Clicking an install link doesn’t open the app**

* Ensure the Helper is installed and running (reopen it from Start/Applications).
* Your browser may prompt each time — choose **Allow** and optionally **Remember**.
* Try another browser, or temporarily disable extensions that block custom links.

**“Matrix CLI not found”**

* The Helper will offer guidance to install it.
  Manual install:

  ```bash
  pipx install matrix-cli
  ```

**Corporate/school computer**

* You might need **administrator** approval to install apps or register custom links.
* If your network uses a proxy, the Helper uses your system’s proxy settings.

**Still stuck?**

* Open an issue on the project’s GitHub and include the **log output** (no personal data).

---

## Security model

* **No Shell Execution:** Arguments are passed directly to the `matrix` binary, eliminating shell-injection risk.
* **Strict Validation:** URL parameters are validated for allowed characters and length; only `matrix://install` is processed.
* **Least Privilege:** The Helper’s capabilities are limited to the *install* action.
* **Mandatory User Consent:** No action is taken without explicit approval in a native OS dialog.
* **Integrity checks:** We publish **SHA-256** checksums for every release (verify with the commands above).
* **Code signing:** Where supported by the OS, installers are signed; see release notes for details.

---

## Privacy

* The app **does not collect personal data**.
* Logs are stored **locally** and only show installation details.
* No background network services are installed; the app respects system proxy settings.

---

## Accessibility & international use

* Works with platform accessibility features (keyboard navigation, high-contrast, screen readers).
* Simple, plain language suitable for non-technical users.
* This README is Markdown and can be translated easily (aim for **WCAG 2.1 AA** documentation guidance).

---

## Build from source (advanced / other Linux)

If your distribution isn’t covered by `.deb`/`.rpm`, or you prefer building locally:

### Prerequisites

* [Node.js](https://nodejs.org/) **v18+** with `npm`
* [Rust](https://rustup.rs/) (stable toolchain)
* Tauri CLI prerequisites for your OS

### Build steps

```bash
# 1) Clone the repository
git clone https://github.com/agent-matrix/matrix-protocol-helper.git
cd matrix-protocol-helper

# 2) Install dependencies and bootstrap the app
make install

# 3) Run in development mode (hot reload)
make dev

# 4) Create production bundles
make build
```

Artifacts are created under:
`src-tauri/target/release/bundle/`

> For a list of available make targets, run:
>
> ```bash
> make help
> ```

---

## Contributing

Bug reports, feature requests, and pull requests are welcome.
Please open an issue to discuss significant changes.

---

## License

**Apache License 2.0** — see the [LICENSE](https://www.apache.org/licenses/LICENSE-2.0) for details.

---

### Quick checklist 

* [ ] Downloaded the installer for my OS
* [ ] Installed it (Next → Install → Finish / drag to Applications / install package)
* [ ] Clicked an **Install from Hub** link and chose **Allow**
* [ ] Read the confirmation and clicked **Yes**
* [ ] Saw **Success** in the log window

You’re all set — enjoy one-click installs with confidence!
