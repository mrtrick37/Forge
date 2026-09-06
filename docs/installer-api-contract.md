# Kyth Installer API Contract

Status: **logical contract, frozen across the native Rust cutover**
Contract version: **1.0**  
Compatibility target: the native Rust installer daemon and React/Tauri client;
the Python implementation is retained only as a parity-test fixture.

This document describes the behavior that installer clients consume. It is not a
promise that every operation is safe on arbitrary hardware: disk mutation is
guarded by the native Rust validation, journal, and recovery layers.

## 1. Transport and authentication

The packaged transport is HTTP framing over a root-owned Unix socket
(`KYTH_INSTALLER_SOCKET`). The legacy HTTP-on-loopback transport
(`127.0.0.1:<PORT>`, with `PORT` supplied by installer configuration) remains
available only for local-development fixture compatibility and must not be
used as the packaged authority.

The native daemon serves the same HTTP framing over the explicitly configured
Unix socket (`KYTH_INSTALLER_SOCKET`). The socket is created with
mode `0600`, or `0660` when `KYTH_INSTALLER_SOCKET_GROUP` is configured. API
authentication remains the session token, and mutating socket requests also
require a valid Linux `SO_PEERCRED` result. The socket path and route set are
fixed by the launcher/native shell; there is no generic filesystem or command
bridge. In the live image, the root launcher writes the per-run session token
to `/run/kyth-installer/session-token` with mode `0600`, starts the
`kyth-installerd.service`, and removes the token after the shell exits.

The legacy HTTP root request must include the one-use `bootstrap_token` query
parameter. A successful request sets:

```http
Set-Cookie: bootstrap_auth=<session-token>; Path=/; HttpOnly; SameSite=Strict
```

The bootstrap token is consumed atomically and cannot be reused. Subsequent requests authenticate with either the `bootstrap_auth` cookie or `X-Kyth-Session-Token: <session-token>`. Unauthenticated requests receive HTTP 403.

Legacy loopback mutating requests additionally require `Host:
127.0.0.1:<PORT>` or `Host: localhost:<PORT>` as their DNS-rebinding defense.
The packaged Unix-socket transport preserves the same logical authorization
boundary with peer credentials and the session token instead of an HTTP Host
header.

JSON requests use `Content-Type: application/json` and UTF-8 bodies. Invalid JSON receives HTTP 400. JSON responses use `Content-Type: application/json`; errors are normally JSON objects with `ok: false` and either `message` or `error`.

## 2. HTTP routes

Unless noted otherwise, API routes require authentication. Paths and methods are exact.

### Read-only GET routes

| Route | Query | Response |
|---|---|---|
| `/api/config` | none | `{source_image, is_live, source}`. `source` is the source-image status object. |
| `/api/disks` | none | Array of disk descriptor objects from the disk discovery layer. |
| `/api/partitions` | `disk` (required in practice) | Array of partition descriptor objects; an absent disk returns `[]`. |
| `/api/free-space` | `disk` (required in practice) | Array of free-region descriptor objects; an absent disk returns `[]`. |
| `/api/disk/pending` | none | Array of staged journal operations, or `[]` when no journal exists. |
| `/api/disk/filesystems` | none | Array of filesystem option objects (`id` and display metadata). |
| `/api/timezones` | none | Array of timezone names. |
| `/api/locales` | none | Array of supported locale descriptors/names. |
| `/api/keymaps` | none | Array of supported keymap descriptors/names. |
| `/api/report` | none | Persisted support-safe transaction object, or `{}` if unavailable. |
| `/api/rescue/probe` | none | Rescue diagnostics object; see §6. |
| `/api/log` | none | Plain-text installer log stream. |
| `/api/stream` | `since` may be supplied by legacy clients but is currently ignored; reconnect is defined by `Last-Event-ID` | SSE stream; see §3. |

The root page and static JavaScript/CSS assets are transport details of the
legacy HTTP fixture. The packaged Tauri clients bundle their own assets, but
they still use the same authenticated session and logical route contract.

### Mutating POST routes

All request bodies are JSON. Successful responses generally return HTTP 200. Validation failures return HTTP 400; conflicts return HTTP 409; unexpected operation failures return HTTP 500.

| Route | Required/recognized body | Success shape and behavior |
|---|---|---|
| `/api/start` | Installation state plus confirmation flags: `disk`, `hostname`, `timezone`, `username`, `password`, `locale`, `keymap`, `kernel`, `install_mode`, optional `target_partition`, `resize_partition`, `resize_gib`, `free_region_start`, `free_region_end`, `confirm_backup`, `confirm_erase`, `confirm_current` | `{started: true}` after validation and worker launch. A running install is 409. |
| `/api/cancel` | `{}` | `{ok: true, message}` when cancellation is accepted. No active cancellable install is 409. Cancellation is cooperative. |
| `/api/reboot` | `{}` | `{ok: true}` after the privileged reboot command is accepted; command failure is `{ok:false,error}` with 500. |
| `/api/disk/new-table` | `disk`, optional `table_type` (`gpt` or `msdos`) | `{ok:true,pending}`; creates a staged `new_table` operation. |
| `/api/disk/create` | `disk`, `start_bytes`, `size_bytes`, optional `fs_type`, `label`, `mountpoint` | `{ok, pending, errors?}`; stages a create operation and validates the journal. |
| `/api/disk/delete` | `disk`, `partition` | `{ok:true,pending}`; refuses mounted/in-use partitions. |
| `/api/disk/resize` | `disk`, `partition`, `new_size_bytes` | `{ok:true,pending}`; current service only permits shrinking. |
| `/api/disk/format` | `disk`, `partition`, optional `fs_type`, `label` | `{ok:true,pending}`; stages a format operation. |
| `/api/disk/set-mountpoint` | `disk`, `partition`, `mountpoint` | `{ok:true,pending}`; mountpoint may be empty, `swap`, or an absolute path. |
| `/api/disk/pending/remove` | `disk`, `index` | `{ok:true,pending}`; removes one uncommitted journal operation. |
| `/api/disk/commit` | `disk` | `{ok:true,root_partition}` after journal validation and destructive commit. Validation failure includes `errors`; an irreversible partial failure includes `irreversible:true`. |
| `/api/disk/rollback` | `disk` | `{ok:true}` after restoring the journal snapshot, or `{ok:false,message}` on failure. |
| `/api/rescue/logs-to-usb` | optional `usb_mount` | `{ok:true,dest,copied}` after copying support-safe logs, or `{ok:false,message}`. If omitted, the service best-effort detects a mount under `/run/media`. |

Partition routes are rejected with HTTP 409 while installation holds the install lock. The backend normalizes device paths and verifies partition ownership; clients must not treat a successful staging response as a disk mutation.

The service has an internal `preview_plan` operation returning a dry-run `PlanReport`, but there is currently no `/api/...` route for it. The React migration must not assume a preview endpoint exists until one is deliberately added and versioned.

## 3. Server-sent events

`GET /api/stream` returns `text/event-stream`, disables intermediary caching, and sends keepalive comments (`:ka`) after a 15-second idle wait. Each event is:

```text
id: <zero-based event index>
data: {"type":"...", ...}

```

The event payload is JSON. Current event types are:

| Type | Fields | Meaning |
|---|---|---|
| `log` | `text` | Human-readable progress/log text. |
| `progress` | `value` (numeric) | Progress value for the progress bar. The producer owns the scale; current UI treats it as a percentage-like value. |
| `stats` | producer-defined fields | Optional install statistics. |
| `phase` | `phase` | One of `prepare`, `storage`, `image`, `configure`, `secure_boot`, `complete`. |
| `done` | optional producer-defined fields, including `mok_state` | Terminal successful result. |
| `error` | `message` | Terminal failure or cancellation. |

Events are ordered by append order within one server context. Event IDs are indexes into the in-memory event list, not globally durable IDs. A reconnect should send `Last-Event-ID: <last-seen-id>`; the server resumes at the following event. The `since` query parameter is compatibility-only, currently ignored by the server, and does not replace the header semantics.

The stream closes after `done` or `error`. A transport disconnect is not itself an install failure: reconnect, then query `/api/report` to determine durable transaction state. Events are not guaranteed to survive a server restart.

## 4. Installer lifecycle

Lifecycle values are:

```text
idle → validated → installing → done
  └→ partitioning → idle
any active state → failed
```

`partitioning` is a focused transaction state and returns to `idle` after a reversible successful partition commit. `done` and `failed` are terminal for the current transaction; submitting a new request resets the context and transaction ID.

Install phases are monotonic:

```text
prepare → storage → image → configure → secure_boot → complete
```

Every phase entry publishes a `phase` event. Progress events and phase events are independent: clients should not infer phase from a progress number, and should render the most recent phase label separately from the progress bar.

Lifecycle transitions, phase-order validation, cancellation decisions, durable
transaction-status ordering, the live power-supply probe, and all destructive
phase execution are owned by the typed Rust `kyth-installerd` and
`kyth-installer-exec` operations. The Python service implementation is not a
packaged fallback; it is retained only for source-level parity tests.

Cancellation sets a flag and publishes a log message. The worker stops at its next safe point. Once destructive storage/image/configuration/secure-boot work has begun, the error explicitly warns that disk changes may already have started; cancellation is never an implicit rollback.

## 5. Disk transaction and recovery

Partition editing is a staged journal. `new-table`, `create`, `delete`, `resize`, `format`, and `set-mountpoint` append operations; `pending/remove` removes one operation; `commit` validates and applies the complete journal; `rollback` restores the saved snapshot and clears staged operations.

The durable transaction report (`/api/report`) is schema version 1 and contains non-secret fields:

```json
{
  "schema_version": 1,
  "transaction_id": "...",
  "updated_at": "...",
  "status": "started|prepared|storage_complete|image_installed|configure_started|configure_complete|secure_boot_staged|complete|failed",
  "phase": "prepare|storage|image|configure|secure_boot|complete",
  "lifecycle": "idle|validated|partitioning|installing|done|failed",
  "install_mode": "...",
  "disk": "...",
  "target_partition": "...",
  "source": {"kind":"...", "digest":"...", "verified": false, "target_ref":"..."},
  "checks": [],
  "partition_steps": [],
  "message": "..."
}
```

`partition_steps` records destructive steps with `index`, `kind`, `status` (`started` or `completed`), and `target`. A durable `started` record means power loss may have interrupted that exact operation. The report is support-safe and is written atomically with restrictive permissions.

## 6. Rescue mode

`/api/rescue/probe` is read-only. Its response includes `log_tail`, `sgdisk_verify`, `efibootmgr`, `bootc_status`, `bootc_status_summary`, `transaction`, and `rescue_guidance`. The guidance maps durable transaction status to a severity, human message, and `bootable` boolean. Unknown or incomplete states must be treated as not bootable.

`/api/rescue/logs-to-usb` is the only rescue mutation. It copies available installer log, transaction, and failure-summary files into `kyth-installer-logs` on the selected USB mount. It must never copy request passwords or other secrets.

## 7. Secret handling

Passwords are accepted only in the POST body and must never be placed in URLs, query parameters, headers intended for general logging, process arguments, SSE payloads, installer logs, failure summaries, transaction reports, or persistent UI state. The current request fields are `password` (validated into `password_hash`) and `mok_password`; both are cleared from runtime state after use. Replacement clients must clear password inputs immediately after `/api/start` is accepted and must not persist them in local storage, crash reports, telemetry, or URLs.

Session tokens are credentials as well: do not log them, expose them to third-party content, or include them in analytics. Tauri/native transport should use an equivalent private credential channel.

## 8. Error and compatibility rules

Clients must handle:

- HTTP 400 as a request or validation error and display `message`/`errors`.
- HTTP 403 as an expired or missing session, requiring controlled re-bootstrap rather than retrying blindly.
- HTTP 409 as a state conflict (install already running, cancellation unavailable, or partition editing locked).
- HTTP 500 as an operation failure; query `/api/report` before offering retry or reboot.
- SSE `error` as a terminal worker result, followed by `/api/report` for recovery details.
- Network disconnects as unknown state until `/api/report` is read; never automatically restart installation.

The React/Tauri client should implement an adapter with the same logical methods (`getConfig`, disk discovery, journal operations, `start`, `cancel`, `reboot`, rescue probe/log export, event subscription). The adapter may map HTTP to a Unix socket or native command bridge later, but request names, response meanings, lifecycle values, event types, terminal behavior, and secret rules remain stable. Any incompatible change requires a new contract version and a compatibility adapter.
