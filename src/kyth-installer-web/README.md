# KythOS installer web frontend

The primary installer client is the Tauri/React shell in this directory. It
uses the shared Kyth Hub visual system while keeping disk and boot operations
behind the authenticated, fixed-route backend. The native Slint client lives
in `src-tauri/src/native_main.rs` and `src-tauri/ui/installer.slint` as an
explicit recovery path selected with `KYTH_USE_NATIVE_INSTALLER=1`.

Both clients support the same initial request flow, authenticated installer
SSE stream, and fixed-route storage operations. The launcher starts the
root-owned Rust daemon and the unprivileged Tauri shell; the Slint client is a
native recovery fallback, not a second backend.

The React/TypeScript frontend consumes the frozen API in
[`docs/installer-api-contract.md`](../../docs/installer-api-contract.md).
The supported image contains only the native Rust installer backend. The
legacy Python WebUI remains in the repository as source-only compatibility
fixture material for parity tests.

For fixture-only local development, run the legacy Python installer service on
`127.0.0.1:8642`:

```bash
npm install
npm run dev
```

The package is embedded in the unprivileged `kyth-installer-shell` Tauri
window. The shell connects to the root-owned `kyth-installerd` Unix socket;
it has no disk, filesystem, or
generic command bridge. The packaged image uses the fixed Unix-socket
transport with typed native request/event commands; loopback is retained only
for local development fixtures.

For native local development, run the Rust daemon with the configured Unix
socket and use:

```bash
npm install
npm run tauri:dev
```
