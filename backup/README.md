# Matrix Protocol Helper

A tiny, secure desktop helper that registers the `matrix://` custom URL scheme and safely hands off **install** requests to the `matrix` CLI.

This application acts as the bridge between your web browser and your local Matrix CLI, enabling a seamless one-click installation experience while prioritizing security.

## Key Features

- **Protocol Registration**: Registers the **`matrix://`** protocol on macOS, Windows, and Linux.
- **User Consent First**: Always prompts the user with a native **Yes/No confirmation dialog** before executing any command.
- **CLI Detection**: Checks if the **Matrix CLI** is installed. If it's missing, the app provides instructions (`pipx install matrix-cli`) and opens the official PyPI page.
- **Secure Execution**: Executes commands safely using a direct process spawn (**no shell**), which prevents shell injection vulnerabilities.
- **Live Feedback**: Streams the `matrix install` process output to a small **log window** and shows a clear **success or failure** status.

---

## The Deep Link Contract

The helper only responds to a strictly defined URL structure:
`matrix://install?entity=<entity>&alias=<alias>[&hub=<hub_url>]`

- `entity` (**required**): The fully-qualified or short name of the component (e.g., `mcp_server:hello@1.0.0`).
- `alias` (**required**): A sanitized alias for local installation. Must match `[A-Za-z0-9_-]{1,64}`.
- `hub` (*optional*): An http/https base URL to override the default Matrix Hub. This sets the `MATRIX_HUB_BASE` environment variable for the CLI.

**No other parameters are supported.** This minimal API surface enhances security.

---

## Build and Run (From Source)

### Prerequisites
- **Node.js 18+** and a package manager (pnpm, yarn, or npm).
- **Rust** stable toolchain (install via [rustup](https://rustup.rs/)).
- **Tauri CLI prerequisites** for your specific OS (see the [Tauri documentation](https://tauri.app/v1/guides/getting-started/prerequisites)).

### Development
```bash
# Install dependencies (pnpm is recommended)
pnpm install

# Run the app in development mode with hot-reloading
pnpm tauri dev
```

### Production Build
```bash
# Build and bundle the application for your platform
pnpm tauri build
```
The final installers and application bundles will be located in `src-tauri/target/release/bundle/`.

---

## Security Model

- **No Shell Execution**: Arguments are passed directly to the `matrix` binary, making it safe from shell injection.
- **Strict Validation**: All URL parameters are strictly validated for allowed characters and length.
- **Least Privilege**: The helper's functionality is limited to the `install` action only.
- **Mandatory User Consent**: The entire workflow is gated by a web modal followed by a native OS confirmation dialog.

---

## License

This project is licensed under the Apache License, Version 2.0.
