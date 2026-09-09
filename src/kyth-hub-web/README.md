# kyth-hub-web

The Hub shell is Tauri/React. Its UI lives in `src/` and its native Rust
bridge and compatibility-named launcher are under `src-tauri/` and the shared
Rust crate. The launcher has no Python fallback; this project does not build
or ship a second UI toolkit.

React + TypeScript frontend for the Kyth Hub web/Tauri rewrite (see
`src-tauri/` for the Rust shell — its bridge commands call straight into
`../../kyth-shared-rs`, the Rust port of `kyth_shared`, no subprocess).

## Running it locally

Build tooling (Rust + the Tauri Linux prerequisites) lives in the
`kyth-ai-dev` dev container, not the base OS — enter it first:

```bash
distrobox enter kyth-ai-dev   # not `toolbox enter` — this box is distrobox-managed
cd ~/git/kyth/src/kyth-hub-web
npm run tauri:dev
```

This starts the Vite dev server and opens a real Tauri window with hot
reload — edit any `.tsx`/`.ts` and it updates live, no restart.

### Why `tauri:dev` sets `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`

WebKitGTK sandboxes its web/network subprocess via bubblewrap by default.
Creating that sandbox means creating a *nested* user namespace, and doing
that from inside an already-containerized shell (distrobox) reliably fails
— silently: the main GTK process stays alive, GTK itself initializes, but
the actual web content process never comes up and no window ever paints.
No crash, no error text, nothing in the log — just a process that sits
there until you kill it. `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`
routes around that for local dev, where the whole point is running nested
inside distrobox.

This is deliberately **not** set for `tauri:build` (the release path) or
anywhere in `src-tauri/`'s own code — an actual installed KythOS desktop
isn't nested inside a dev container, so the sandbox should work normally
there, and there's no reason to weaken it for a real install.
