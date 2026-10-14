# Lakasir Printer Bridge

A small Rust HTTP bridge for sending text print jobs to printers installed in the operating system. The same source builds on Windows, macOS, and Linux.

## Build and run

Install the Rust stable toolchain, then run:

```sh
cargo run --release
```

The server listens on `http://127.0.0.1:5463` by default. Set `HTTP_PORT` to use another port. Printer selection and paper width are stored in the operating system's user configuration directory.

On Windows, printer discovery and printing use PowerShell's `Get-Printer` and `Out-Printer`. On macOS and Linux, install CUPS; discovery uses `lpstat`, and printing uses `lp` (or `lpr` if `lp` is unavailable). Printers must already be installed and configured with the operating system.

## HTTP API

- `GET /printers` lists installed printers as `{ "printers": [{ "name": "..." }] }`.
- `GET /settings` returns the configured `printerName` and `paperWidth`.
- `POST /settings` saves those settings:

  ```json
  { "printerName": "Receipt Printer", "paperWidth": 48 }
  ```

- `POST /print` prints `text` followed by each entry in `items`:

  ```json
  { "text": "Order #123", "items": ["Item 1", "Total: $10.00"] }
  ```

The print endpoint currently supports text and item lines, wrapping at the configured paper width. Image fields from the earlier Electron bridge are rejected with an error rather than silently ignored.

The API is bound to loopback and permits cross-origin browser requests. It does not expose the printer to other machines on the network.

## Multi-platform release builds

GitHub Actions builds a release executable on Windows, macOS, and Linux for pushes and pull requests. Download the corresponding `printer-bridge-<platform>` artifact from the workflow run. To build locally for the current platform, use `cargo build --release`.
