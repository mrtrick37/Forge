import { invoke } from "@tauri-apps/api/core";
import { inTauriShell } from "./tauriEnv";

// Real backend data, read through the Tauri shell's bridge commands (see
// src-tauri/src/main.rs, which calls straight into the kyth-shared Rust
// crate — src/kyth-shared-rs — no subprocess). Every read here returns
// null rather than throwing when the data isn't available — running in a
// plain browser (npm run dev), no Tauri build, or (very commonly on a dev
// machine) no on-disk state yet because kyth-probe / Guardian have never
// run. Callers render an honest empty state on null; there are no fixtures
// left to fall back to.
//
// Two conventions the sections rely on:
//   - Reads may run on mount, but the Tauri commands for update status,
//     update summaries, and channel fallback offload blocking probes before
//     they reach this webview. Switching tabs therefore never blocks the UI.
//   - The mutating wrappers at the bottom throw instead of returning null,
//     so useSectionAction can report the failure rather than leaving a
//     button that appears to have done something.

export interface GuardianHistoryItem {
  timestamp: number;
  title: string;
  detail: string;
  status: "ok" | "warn" | "error";
  recipeId: string | null;
  action: string;
  verified: boolean | null;
}

export interface GuardianSnapshot {
  pendingCount: number;
  pending: GuardianPendingItem[];
  history: GuardianHistoryItem[];
}

// Mirrors main.rs's GuardianSnapshotResponse shape exactly.
interface GuardianBridgeHistoryItem {
  timestamp: number;
  recipe_id: string | null;
  title: string;
  detail: string;
  action: string;
  verified: boolean | null;
}
interface GuardianBridgePendingItem {
  recipe_id: string;
  title: string;
  detail: string;
  risk: string;
}
interface GuardianBridgeResponse {
  pending_count: number;
  pending: GuardianBridgePendingItem[];
  history: GuardianBridgeHistoryItem[];
}

function statusFor(item: GuardianBridgeHistoryItem): GuardianHistoryItem["status"] {
  if (item.action === "skipped") return "warn";
  if (item.verified === false) return "error";
  if (item.verified === true) return "ok";
  return "warn"; // recommended, not yet actioned
}

export async function fetchGuardianSnapshot(): Promise<GuardianSnapshot | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<GuardianBridgeResponse>("guardian_snapshot");
    return {
      pendingCount: raw.pending_count,
      pending: raw.pending.map((item) => ({
        recipeId: item.recipe_id,
        title: item.title,
        detail: item.detail,
        risk: item.risk,
      })),
      history: raw.history.map((item) => ({
        timestamp: item.timestamp,
        recipeId: item.recipe_id,
        title: item.title,
        detail: item.detail,
        action: item.action,
        verified: item.verified,
        status: statusFor(item),
      })),
    };
  } catch {
    return null;
  }
}
interface GuardianActionLaunch { job: string; state: "running"; detail: string; }
function guardianJob(launch: GuardianActionLaunch): string { if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Guardian action did not start."); return launch.job; }
export async function runGuardianCheck(investigate = false): Promise<string> {
  return guardianJob(await invoke<GuardianActionLaunch>("guardian_check", { investigate }));
}
export async function waitGuardianCheck(job: string): Promise<string> {
  for (let i = 0; i < 180; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await invoke<InstallStatus>("guardian_check_status", { job });
    if (state.state === "complete") return state.detail;
    if (state.state === "failed" || state.state === "unknown") throw new Error(state.detail);
  }
  throw new Error("Guardian is still running; refresh the page in a moment.");
}
export async function runGuardianControl(action: string): Promise<string> {
  if (!confirmUserAction(`Change Guardian setting: ${action}?`)) return "Cancelled.";
  const job = guardianJob(await invoke<GuardianActionLaunch>("guardian_control", { action }));
  return await waitGuardianCheck(job);
}

/** Shared confirmation boundary for actions that can change system state.
 * Tests and non-browser renders remain usable; the Tauri webview always has
 * the native browser confirm dialog. Never pass secret values in message. */
export function confirmUserAction(message: string): boolean {
  if (typeof window === "undefined" || typeof window.confirm !== "function") return true;
  return window.confirm(message);
}

type PrivilegedPayload = Record<string, string | boolean | number>;
interface PrivilegedActionLaunch { job: string; state: "running"; detail: string; }

function privilegedActionPrompt(operation: string, payload: PrivilegedPayload): string {
  switch (operation) {
    case "bitlocker_unlock":
      return `Unlock ${payload.device ?? "this BitLocker volume"}? The recovery key will be sent only to the local privileged service.`;
    case "kernel_switch":
      return `Stage the ${payload.flavor ?? "selected"} kernel? This changes the next boot deployment.`;
    case "secureboot_enroll":
      return "Enroll the KythOS Secure Boot key? This changes firmware trust configuration.";
    case "nvidia_install":
      return "Install the NVIDIA driver? This stages a system image change.";
    case "firmware_update":
      return "Apply firmware updates? The device may reboot during this operation.";
    case "network_share_add":
      return `Add network share ${typeof payload.name === "string" ? payload.name : ""}? Its credentials are sent only to the local privileged helper and saved in a protected root-owned file.`;
    case "network_share_remove":
      return `Remove network share ${typeof payload.name === "string" ? payload.name : ""}? This deletes its systemd mount unit and protected credentials.`;
    default:
      return `Run privileged operation ${operation}?`;
  }
}

export async function runPrivilegedAction(operation: string, payload: PrivilegedPayload = {}): Promise<string> {
  if (!confirmUserAction(privilegedActionPrompt(operation, payload))) return "Cancelled.";
  const launch = await invoke<PrivilegedActionLaunch>("privileged_action", { operation, payload });
  if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Privileged operation did not start.");
  const job = launch.job;
  for (let i = 0; i < 1800; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await invoke<InstallStatus>("privileged_action_status", { job });
    if (state.state === "complete") return state.detail;
    if (state.state === "failed" || state.state === "unknown") throw new Error(state.detail);
  }
  throw new Error("Privileged operation is still running; check the system status shortly.");
}

// Mirrors kyth_shared.system.bootc_policy.branch_display_name() — small
// enough to duplicate as a presentation-only mapping here rather than
// round-trip through the bridge for display text.
const CHANNEL_DISPLAY: Record<string, string> = {
  latest: "Stable (latest)",
  testing: "Testing",
  "latest-cachy": "Stable + CachyOS kernel",
  "testing-cachy": "Testing + CachyOS kernel",
};

interface ProbeBridgeResponse<T = unknown> {
  key: string;
  data: T | null;
  error: string | null;
}

/** Generic disk-backed probe section read — see main.rs's probe_backend
 * command / kyth_shared::system::probe::read_section. Every probe_backend
 * caller below is this same call with a different key and a typed
 * reshape; this is just the shared plumbing. */
async function fetchProbeSection<T>(key: string): Promise<T | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<ProbeBridgeResponse<T>>("probe_backend", { section: key });
    return raw.data ?? null;
  } catch {
    return null;
  }
}

export async function fetchUpdateChannel(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<string | null>("current_update_channel");
    if (!raw) return null;
    return CHANNEL_DISPLAY[raw] ?? raw;
  } catch {
    return null;
  }
}

interface HardwareBridgeResponse {
  gpu_line: string | null;
}

// Strips a raw `lspci -nn` line down to a display-sized name — best-effort
// only (lspci's format varies enough by vendor that a fully robust parse
// isn't realistic); falls back to the raw line untouched if the shape
// doesn't match what's stripped here, so nothing goes missing, just less
// tidy. Example input:
//   "03:00.0 VGA compatible controller [0300]: Advanced Micro Devices,
//    Inc. [AMD/ATI] Navi 31 [Radeon RX 7900 XT/7900 XTX] [1002:744c] (rev c8)"
function cleanGpuName(raw: string): string {
  return raw
    .replace(/^\S+\s+.*?\[[0-9a-f]{4}\]:\s*/i, "") // bus address + controller class + hex class code
    .replace(/\s*\[[0-9a-f]{4}:[0-9a-f]{4}\]\s*$/i, "") // trailing vendor:device PCI id
    .replace(/\s*\(rev [0-9a-f]+\)\s*$/i, "") // trailing revision
    .trim();
}

export async function fetchGpuName(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<HardwareBridgeResponse>("hardware_snapshot");
    if (!raw.gpu_line) return null;
    return cleanGpuName(raw.gpu_line) || raw.gpu_line;
  } catch {
    return null;
  }
}

interface StorageBridgeResponse {
  free_bytes: number | null;
  total_bytes: number | null;
}

function formatGiB(bytes: number): string {
  return `${Math.round(bytes / 1024 ** 3)} GB`;
}

export async function fetchStorageFree(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<StorageBridgeResponse>("storage_snapshot");
    if (raw.free_bytes == null) return null;
    return formatGiB(raw.free_bytes);
  } catch {
    return null;
  }
}

// Shape of one `status.booted` / `status.rollback` entry in `bootc status
// --format=json`'s own output — see kyth_shared.system.bootc_query's
// fetch_status_data(), which is a bare parse of that command, no
// reshaping. Every field is optional: this is read straight off the
// disk-backed probe cache (see kyth_shared::system::probe::read_section),
// which only has this at all once kyth-probe.service has actually run on
// a real KythOS install —
// never on a plain dev checkout, which is the expected null case here.
export interface BootcDeployment {
  image?: string;
  version?: string;
  timestamp?: string;
  imageDigest?: string;
}

export interface BootcSnapshot {
  channel: string | null; // display name, e.g. "Testing"
  booted: BootcDeployment | null;
  rollback: BootcDeployment | null;
}

interface BootcStatusImage {
  image?: string | { image?: string; reference?: string; imageDigest?: string; digest?: string };
  reference?: string;
  version?: string;
  timestamp?: string;
  imageDigest?: string;
  digest?: string;
}

interface BootcStatusJsonEntry {
  image?: BootcStatusImage | string;
  version?: string;
  timestamp?: string;
  imageDigest?: string;
  digest?: string;
}

interface BootcStatusJson {
  status?: {
    booted?: BootcStatusJsonEntry;
    rollback?: BootcStatusJsonEntry;
  };
}

function deploymentFrom(entry: BootcStatusJsonEntry | undefined): BootcDeployment | null {
  const rawImage = entry?.image;
  if (!rawImage) return null;
  if (typeof rawImage === "string") {
    return {
      image: rawImage,
      version: entry.version,
      timestamp: entry.timestamp,
      imageDigest: entry.imageDigest ?? entry.digest,
    };
  }
  const nestedImage = typeof rawImage.image === "object" ? rawImage.image : null;
  const imageRef = typeof rawImage.image === "string"
    ? rawImage.image
    : nestedImage?.image ?? nestedImage?.reference ?? rawImage.reference;
  return {
    image: imageRef,
    version: entry.version ?? rawImage.version,
    timestamp: entry.timestamp ?? rawImage.timestamp,
    imageDigest: entry.imageDigest ?? rawImage.imageDigest ?? nestedImage?.imageDigest ?? rawImage.digest ?? nestedImage?.digest,
  };
}

export async function fetchBootcSnapshot(): Promise<BootcSnapshot | null> {
  if (!inTauriShell()) return null;
  try {
    const [statusRaw, channelRaw] = await Promise.all([
      invoke<ProbeBridgeResponse>("probe_backend", { section: "bootc-status-data" }),
      invoke<ProbeBridgeResponse<string>>("probe_backend", { section: "bootc-branch" }),
    ]);
    const data = statusRaw.data as unknown as BootcStatusJson | null;
    if (!data) return null;
    return {
      channel: channelRaw.data ? (CHANNEL_DISPLAY[channelRaw.data] ?? channelRaw.data) : null,
      booted: deploymentFrom(data.status?.booted),
      rollback: deploymentFrom(data.status?.rollback),
    };
  } catch {
    return null;
  }
}

// kernel-flavor and nvidia-detect are both plain scalars already in
// DISK_TTL — no new backend needed, just fetchProbeSection with the right
// key and type.
export async function fetchKernelFlavor(): Promise<string | null> {
  return fetchProbeSection<string>("kernel-flavor");
}

export async function fetchNvidiaDetected(): Promise<boolean | null> {
  return fetchProbeSection<boolean>("nvidia-detect");
}

// Mirrors kyth_shared.system.probe's "network-summary" JSON-safe
// projection exactly (see probe.py's _collect_network_identity) — covers
// VPN, Network Shares, and Cloud Storage sections from one probe read.
export interface NetworkSummary {
  vpnConnected: boolean;
  vpnName: string;
  smbMounts: number;
  cloudProviders: string[];
  detail: string;
}

interface NetworkSummaryRaw {
  vpn_connected: boolean;
  vpn_name: string;
  smb_mounts: number;
  cloud_providers: string[];
  detail: string;
}

export async function fetchNetworkSummary(): Promise<NetworkSummary | null> {
  const raw = await fetchProbeSection<NetworkSummaryRaw>("network-summary");
  if (!raw) return null;
  return {
    vpnConnected: raw.vpn_connected,
    vpnName: raw.vpn_name,
    smbMounts: raw.smb_mounts,
    cloudProviders: raw.cloud_providers,
    detail: raw.detail,
  };
}

// Mirrors kyth_shared.system.controllers.detect_controllers()'s dict shape
// exactly — read from the disk-backed "controllers-detect" probe section
// (see probe.py's DISK_TTL), same as every fetchProbeSection call.
export interface ControllerInfo {
  usbControllers: { name: string; kind: string }[];
  inputNodeCount: number;
  driverLoaded: { xone: boolean; xpadneo: boolean; hidPlaystation: boolean };
}

interface ControllersDetectRaw {
  usb_controllers: [string, string][];
  input_nodes: string[];
  xone_loaded: boolean;
  xpadneo_loaded: boolean;
  hid_ps_loaded: boolean;
}

export async function fetchControllers(): Promise<ControllerInfo | null> {
  const raw = await fetchProbeSection<ControllersDetectRaw>("controllers-detect");
  if (!raw) return null;
  return {
    usbControllers: raw.usb_controllers.map(([name, kind]) => ({ name, kind })),
    inputNodeCount: raw.input_nodes.length,
    driverLoaded: { xone: raw.xone_loaded, xpadneo: raw.xpadneo_loaded, hidPlaystation: raw.hid_ps_loaded },
  };
}

// flatpak-apps and flatpak-updates are separate probe collectors with
// separate TTLs (see probe.py) — genuinely independent, so each stays
// nullable rather than collapsing a missing one to 0 (which would read as
// "zero updates" instead of "unknown").
export interface AppStoreSnapshot {
  installedCount: number | null;
  updatesAvailable: number | null;
}

export async function fetchAppStoreSnapshot(): Promise<AppStoreSnapshot | null> {
  const [apps, updates] = await Promise.all([
    fetchProbeSection<string[]>("flatpak-apps"),
    fetchProbeSection<number>("flatpak-updates"),
  ]);
  if (apps == null && updates == null) return null;
  return { installedCount: apps?.length ?? null, updatesAvailable: updates ?? null };
}

// Mirrors kyth_shared.system.probe's "hardware-summary" JSON-safe
// projection (see probe.py's _collect_hardware_view — deliberately not the
// raw HardwareView dataclass, which isn't JSON-serializable).
export interface HardwareSnapshot {
  gpuName: string | null;
  hasNvidia: boolean | null;
  isHybrid: boolean | null;
  capabilities: string[];
}

interface HardwareSummaryRaw {
  has_nvidia: boolean;
  is_hybrid: boolean;
  capabilities: string[];
}

export async function fetchHardwareSnapshot(): Promise<HardwareSnapshot | null> {
  const [gpuName, summary] = await Promise.all([
    fetchGpuName(),
    fetchProbeSection<HardwareSummaryRaw>("hardware-summary"),
  ]);
  if (gpuName == null && summary == null) return null;
  return {
    gpuName,
    hasNvidia: summary?.has_nvidia ?? null,
    isHybrid: summary?.is_hybrid ?? null,
    capabilities: summary?.capabilities ?? [],
  };
}

// Mirrors main.rs's GuardianPendingResponse — the same
// pending_recommendations() list Hub's own mission bar/sidebar badge reads,
// now with a title (via RECIPES) and risk level attached for display.
export interface GuardianPendingItem {
  recipeId: string;
  title: string;
  detail: string;
  risk: string;
}

/** "3h ago" / "2d ago" style relative time — Guardian history stores raw
 * unix-seconds timestamps, formatting is a frontend presentation concern. */
export function relativeTime(unixSeconds: number): string {
  const diffMs = Date.now() - unixSeconds * 1000;
  const minutes = Math.round(diffMs / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

// Channels reuses bootc-branch (already cached for Update) — same data,
// different framing: ChannelSection shows the switcher state vs. Update's
// deployment view.
export async function fetchChannelRaw(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string | null>("current_update_channel"); } catch { return null; }
}

// display-detect (capabilities/profiles) was collected but never readable
// via disk cache until the DISK_TTL fix above — now it's a normal
// fetchProbeSection like hardware-summary.
export interface DisplayDetect {
  capabilities: string[];
  profiles: string[];
}
export async function fetchDisplayDetect(): Promise<DisplayDetect | null> {
  return fetchProbeSection<DisplayDetect>("display-detect");
}

// ntfs-drives — other-system NTFS/BitLocker partitions from lsblk, via
// probe_cached("ntfs-drives") in kyth_welcome/services/hardware/drives.py
// (also written to the shared probe-cache.json so Hub can read it).
export interface NtfsDrive {
  dev: string;
  name: string;
  size: string;
  label: string;
  mount: string;
  is_bitlocker: boolean;
}
export async function fetchNtfsDrives(): Promise<NtfsDrive[] | null> {
  return fetchProbeSection<NtfsDrive[]>("ntfs-drives");
}

// audit-cache — 46-140 perf audit (gaming + scheduler + memory tunables)
// plus systemd-analyze line. Written by kyth_shared.perf_audit via
// update_sections({"audit-cache": data}); large, loosely-typed by design.
export type AuditCache = Record<string, unknown> & { ts?: number; systemd_analyze?: string; master?: string };
export async function fetchAuditCache(): Promise<AuditCache | null> {
  const raw = await fetchProbeSection<AuditCache>("audit-cache");
  if (!raw || typeof raw !== "object") return null;
  return raw;
}

// secureboot-state — the cheap disk-cached Secure Boot scalar. Read on
// mount; CompatibilitySection escalates to live mokutil (fetchMokStatus)
// only when the user asks, because mokutil is slow enough to stall a tab
// switch. The "firmware-cache" section is deliberately not wrapped —
// fetchFirmwareUpdatesCount is the readable form of the same thing.
export async function fetchSecurebootState(): Promise<string | null> {
  return fetchProbeSection<string>("secureboot-state");
}

// Just recipes — live `just --list` via Tauri (port of page_just.py).
// `params` is non-empty when the recipe takes arguments. The Hub only offers
// buttons for no-argument recipes until it has a safe, user-friendly form for
// choosing those arguments.
export interface JustRecipe { name: string; params: string; comment: string }
export interface HubActionLaunch { job: string; state: "running"; detail: string; }
export async function fetchJustList(): Promise<JustRecipe[] | null> {
  if (!inTauriShell()) return null;
  try {
    const raw = await invoke<JustRecipe[]>("just_list");
    return raw ?? null;
  } catch { return null; }
}
// Recipes run as captured background jobs. KDE's graphical askpass helper is
// used by the Rust shell for sudo, so there is no terminal window to find or
// explain to a new user.
async function waitJustJob(job: string): Promise<string> {
  for (let i = 0; i < 1800; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await invoke<InstallStatus>("hub_action_status", { job });
    if (state.state === "complete") return state.detail;
    if (state.state === "failed" || state.state === "unknown") throw new Error(state.detail);
  }
  throw new Error("This action is still running; check the status here again in a moment.");
}

async function waitHubActionLaunch(launch: HubActionLaunch): Promise<string> {
  if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Recipe did not start.");
  return await waitJustJob(launch.job);
}

export interface UpdateActionLaunch { job: string; state: "running"; detail: string; }

async function waitUpdateJob(job: string): Promise<string> {
  // Upgrade downloads can legitimately take an hour on a slow connection.
  for (let i = 0; i < 7200; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await invoke<InstallStatus>("update_job_status", { job });
    if (state.state === "complete") return state.detail;
    if (state.state === "failed" || state.state === "unknown") throw new Error(state.detail);
  }
  throw new Error("The update is still running; refresh the Updates page in a moment.");
}

async function waitUpdateLaunch(launch: UpdateActionLaunch): Promise<string> {
  if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Update action did not start.");
  return await waitUpdateJob(launch.job);
}

async function runHubAction(recipe: string): Promise<string> {
  if (!inTauriShell()) throw new Error("This action is only available in the Hub app.");
  return await waitHubActionLaunch(await invoke<HubActionLaunch>("run_hub_action", { action: recipe }));
}

export async function runHubRecipeAction(recipe: string): Promise<string> {
  return await runHubAction(recipe);
}

// Update card view-model — the Rust port of the Qt Update page's
// "what should this card say" logic. UpdatesSection feeds it the live
// update_status + collect_availability reads rather than recomputing the
// copy in TS. The sibling branch_display_name command stays unwrapped on
// purpose: CHANNEL_DISPLAY above is the one authority for channel labels.
export interface UpdateAvailabilityView { card_style: string; icon_text: string; icon_style: string; title: string; body: string; update_btn_visible: boolean; restart_btn_visible: boolean; }
export async function fetchUpdateAvailabilityView(args: { staged: boolean; check_state: string; flatpak_count: number; check_ts: string; check_ts_details: string; staged_ts?: string | null }): Promise<UpdateAvailabilityView | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<UpdateAvailabilityView>("update_availability_view", args); } catch { return null; }
}

// Mok verify — live mokutil Secure Boot + enrollment (N40)
export interface MokStatus { sb_state: string; enrolled: string; }
export async function fetchMokStatus(): Promise<MokStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<MokStatus>("mok_status"); } catch { return null; }
}

// Fonts ready — live fc-list check (N35)
export interface FontsReady { ready: boolean; detail: string; }
export async function fetchFontsReady(): Promise<FontsReady | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<FontsReady>("fonts_ready"); } catch { return null; }
}

// Mesa version — live glxinfo/rpm check (N41)
export async function fetchMesaVersion(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string>("mesa_version"); } catch { return null; }
}
export async function fetchMesaOverlayDryRun(): Promise<{ ok: boolean; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ ok: boolean; detail: string }>("mesa_overlay_dry_run"); } catch { return null; }
}

// SMB — Aurora autodiscover parity (N33)
export async function fetchSmbBrowse(host?: string | null): Promise<{ ok: boolean; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ ok: boolean; detail: string }>("smb_browse", { host: host ?? null }); } catch { return null; }
}
export async function mountSmbShare(share: string): Promise<string> {
  if (!inTauriShell()) throw new Error("Share mounting is available from the installed Kyth Hub.");
  return await invoke<string>("smb_mount", { share });
}
export interface ConfiguredNetworkShare {
  name: string;
  server: string;
  share_path: string;
  mount_point: string;
  username: string;
  domain: string;
  auto_mount: boolean;
}
export interface NetworkShareInput extends ConfiguredNetworkShare {
  password: string;
  mount_now: boolean;
}
interface SmbActionResult { state: "complete"; detail: string; }
export async function fetchConfiguredNetworkShares(): Promise<ConfiguredNetworkShare[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<ConfiguredNetworkShare[]>("smb_configured_shares"); } catch { return null; }
}
export async function addNetworkShare(share: NetworkShareInput): Promise<string> {
  const detail = await runPrivilegedAction("network_share_add", { ...share });
  if (detail === "Cancelled.") return detail;
  const saved = await invoke<SmbActionResult>("smb_save_configured_share", { share: {
    name: share.name, server: share.server, share_path: share.share_path,
    mount_point: share.mount_point, username: share.username, domain: share.domain,
    auto_mount: share.auto_mount,
  } });
  if (saved.state !== "complete") throw new Error(saved.detail || "Network share configuration was not saved.");
  return detail;
}
export async function removeNetworkShare(share: Pick<ConfiguredNetworkShare, "name" | "mount_point">): Promise<string> {
  const detail = await runPrivilegedAction("network_share_remove", { ...share });
  if (detail === "Cancelled.") return detail;
  const removed = await invoke<SmbActionResult>("smb_remove_configured_share", { name: share.name });
  if (removed.state !== "complete") throw new Error(removed.detail || "Network share configuration was not removed.");
  return detail;
}

// Memory pressure + snapshot count (Diagnostics/Repair)
export async function fetchMemoryPressure(): Promise<{ status: string; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ status: string; detail: string }>("memory_pressure"); } catch { return null; }
}
export async function fetchSnapshotCount(): Promise<number | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<number>("snapshot_count"); } catch { return null; }
}
export interface SnapshotRow {
  id: string;
  timestamp: string;
  type: string;
  description: string;
  healthy?: boolean | null;
}
export async function fetchSnapshotTimeline(limit = 20): Promise<SnapshotRow[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<SnapshotRow[]>("snapshot_timeline", { limit }); } catch { return null; }
}

export async function fetchGamingSliceAvailable(): Promise<boolean | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<boolean>("is_gaming_slice_available"); } catch { return null; }
}

// Cloud OAuth + Printing (N36/N34)
export async function fetchCloudOauthStatus(): Promise<{ ok: boolean; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ ok: boolean; detail: string }>("cloud_oauth_status"); } catch { return null; }
}
export interface CloudSyncRemote {
  name: string;
  service: string;
  folder: string;
  last_sync: number | null;
  last_ok: boolean | null;
}
export async function fetchCloudSyncRemotes(): Promise<CloudSyncRemote[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<CloudSyncRemote[]>("cloud_sync_remotes"); } catch { return null; }
}
async function waitHubJob(job: string, limit = 7200): Promise<string> {
  for (let i = 0; i < limit; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await invoke<InstallStatus>("job_status", { job }).catch(() => null);
    if (!state || state.state === "running") continue;
    if (state.state === "complete") return state.detail;
    throw new Error(state.detail);
  }
  throw new Error("This action is still running; check back in a moment.");
}
export async function runCloudSync(remote: string): Promise<string> {
  if (!inTauriShell()) throw new Error("Cloud sync is available from the installed Kyth Hub.");
  if (!confirmUserAction(`Sync ${remote} to its saved local folder?`)) return "Cancelled.";
  return await waitHubJob(await invoke<string>("cloud_sync_now", { remote }));
}
export async function openBackupApp(): Promise<string> {
  if (!inTauriShell()) throw new Error("Backup is available from the installed Kyth Hub.");
  return await invoke<string>("open_backup_app");
}
export async function openCloudStorageApp(): Promise<string> {
  if (!inTauriShell()) throw new Error("The full Cloud Storage workflow is available from the installed Kyth Hub.");
  return await invoke<string>("open_cloud_storage_app");
}
export async function openMoveFilesApp(): Promise<string> {
  if (!inTauriShell()) throw new Error("The full migration workflow is available from the installed Kyth Hub.");
  return await invoke<string>("open_move_files_app");
}
export interface MigrationReadiness { bookmarks: string; drives: string; files: string; onedrive: string; pwa: string; parity: string; }
export async function fetchMigrationReadiness(): Promise<MigrationReadiness | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<MigrationReadiness>("migration_readiness"); } catch { return null; }
}
export async function openNetworkSharesApp(): Promise<string> {
  if (!inTauriShell()) throw new Error("The full Network Shares workflow is available from the installed Kyth Hub.");
  return await invoke<string>("open_network_shares_app");
}
export async function fetchPrinterDiscover(): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("ipp_discover"); } catch { return null; }
}

// Btrfs + drivers (Repair/Hardware)
export async function fetchBtrfsHealth(): Promise<{ status: string; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ status: string; detail: string }>("btrfs_health"); } catch { return null; }
}
export async function fetchPciByClass(deviceClass: string): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("pci_devices_by_class", { class: deviceClass }); } catch { return null; }
}

// Controllers live detect (lsusb + lsmod)
export interface ControllersLive { usb_controllers: [string,string][]; input_nodes: string[]; xone_dongle: boolean; xone_loaded: boolean; xpadneo_loaded: boolean; hid_ps_loaded: boolean; dualsense_found: boolean; }
export async function fetchControllersLive(): Promise<ControllersLive | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<ControllersLive>("controllers_detect"); } catch { return null; }
}

// Hardware view summary — canonical ProbeService cached view (30s)
export interface HardwareViewSummary { has_nvidia: boolean; is_hybrid: boolean; capabilities: string[]; }
export async function fetchHardwareViewSummary(): Promise<HardwareViewSummary | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<HardwareViewSummary>("hardware_view_summary"); } catch { return null; }
}

// Network identity live (VPN/SMB/cloud) — live nmcli + mounts, reshaped to
// the same NetworkSummary the cached "network-summary" probe read returns
// so the three Move In sections can swap one for the other. Mount reads the
// cache; a Refresh button reads this.
interface NetworkIdentityLive { vpn_connected: boolean; vpn_name: string; smb_mounts: number; cloud_providers: string[]; detail: string; }
async function fetchNetworkIdentityLive(): Promise<NetworkIdentityLive | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<NetworkIdentityLive>("network_identity"); } catch { return null; }
}

export async function fetchNetworkSummaryLive(): Promise<NetworkSummary | null> {
  const raw = await fetchNetworkIdentityLive();
  if (!raw) return null;
  return {
    vpnConnected: raw.vpn_connected,
    vpnName: raw.vpn_name,
    smbMounts: raw.smb_mounts,
    cloudProviders: raw.cloud_providers,
    detail: raw.detail,
  };
}

export async function openVpnApp(): Promise<string> {
  if (!inTauriShell()) throw new Error("Native VPN controls are available from the installed Kyth Hub.");
  return await invoke<string>("open_vpn_app");
}
export async function startVpnConnection(profile: { gateway: string; protocol: string; os_emulation: string; username: string; password: string }): Promise<string> {
  if (!inTauriShell()) throw new Error("VPN connections require the installed Kyth Hub.");
  return await invoke<string>("vpn_connect", profile);
}
export interface VpnConnectionStatus { id: string; state: "connecting" | "authentication_required" | "connected" | "disconnected" | "failed" | "unknown"; detail: string; }
export async function fetchVpnConnectionStatus(job: string): Promise<VpnConnectionStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<VpnConnectionStatus>("vpn_status", { job }); } catch { return null; }
}
export async function disconnectVpnConnection(job: string): Promise<string> {
  if (!inTauriShell()) throw new Error("VPN connections require the installed Kyth Hub.");
  return await invoke<string>("vpn_disconnect", { job });
}
export interface VpnSavedProfile { gateway: string; protocol: string; os: string; }
export async function fetchVpnSavedProfile(): Promise<VpnSavedProfile | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<VpnSavedProfile | null>("vpn_saved_profile"); } catch { return null; }
}

// Updates unified — bootc/flatpak/firmware summary
export async function fetchPendingUpdatesSummary(): Promise<Record<string,string> | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<Record<string,string>>("pending_updates_summary"); } catch { return null; }
}

// PipeWire quantum presets (N32)
export async function fetchAudioPresets(): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("available_audio_presets"); } catch { return null; }
}
export async function applyPipewireQuantum(preset: string, dryRun = false): Promise<{ ok: boolean; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ ok: boolean; detail: string }>("apply_pipewire_quantum", { preset, dryRun }); } catch { return null; }
}

// Deployment history — bootc timeline (Repair)
export interface DeploymentInfo { section: string; label: string; available: boolean; reference?: string | null; branch?: string | null; timestamp?: string | null; digest?: string | null; short_digest?: string | null; status_text: string; }
export async function fetchDeploymentHistory(): Promise<DeploymentInfo[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<DeploymentInfo[]>("deployment_history"); } catch { return null; }
}

// Recovery status — staged/rollback/quarantined single view (Repair)
export interface RecoveryStatus { has_staged: boolean; has_rollback: boolean; quarantined_digest: string; quarantine_detail: string; watcher_staged: boolean; clear_quarantine_cmd: string; banner: string; }
export async function fetchRecoveryStatus(): Promise<RecoveryStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<RecoveryStatus>("recovery_status"); } catch { return null; }
}

// Update status — TTL-bounded check_state (Updates)
export interface UpdateStatusLive { booted?: string | null; staged: boolean; rollback: boolean; remote_digest?: string | null; blocked_reason?: string | null; retry_cmd?: string | null; check_state: string; detail: string; }
export async function fetchUpdateStatus(): Promise<UpdateStatusLive | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<UpdateStatusLive>("update_status"); } catch { return null; }
}

export interface UpdateHealthLive { status: string; pending_digest: string; last_healthy_digest: string; failures: number; quarantined: number; detail: string; }
export async function fetchUpdateHealth(): Promise<UpdateHealthLive | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<UpdateHealthLive>("update_health"); } catch { return null; }
}

export interface UpdateWatcherStatus { available: boolean; enabled: boolean; active: boolean; }
export async function fetchUpdateWatcherStatus(): Promise<UpdateWatcherStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<UpdateWatcherStatus>("update_watcher_status"); } catch { return null; }
}
export async function setUpdateWatcherEnabled(enabled: boolean): Promise<string> {
  if (!inTauriShell()) throw new Error("The automatic update controls require the installed Hub.");
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("set_update_watcher_enabled", { enabled }));
}
export async function checkForUpdatesNow(): Promise<string> {
  if (!inTauriShell()) throw new Error("The automatic update controls require the installed Hub.");
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("check_for_updates_now"));
}
export async function deferUpdateWatcher(): Promise<string> {
  if (!inTauriShell()) throw new Error("The automatic update controls require the installed Hub.");
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("defer_update_watcher"));
}

// Process helpers — live session + ansi + disk bytes
export async function fetchIsLiveSession(): Promise<boolean | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<boolean>("is_live_session"); } catch { return null; }
}

// Firmware — fwupd counts (Hardware)
export async function fetchFirmwareUpdatesCount(): Promise<number | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<number>("firmware_updates_count"); } catch { return null; }
}

// Plasma HDR/VRR presets
export async function fetchPlasmaPresets(): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("plasma_presets"); } catch { return null; }
}

// Update availability collect (Hub-side 45s deadline, issue #164)
export interface AvailabilityStatusLive { state: string; detail: string; flatpak_count: number; flatpak_detail: string; staged: boolean; manifest_raw: string; blocked_reason: string; }
export async function fetchCollectAvailability(branch?: string | null, useCached = true): Promise<AvailabilityStatusLive | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<AvailabilityStatusLive>("collect_availability", { branch: branch ?? null, useCached }); } catch { return null; }
}

// Drives — live `lsblk -J` blockdevices (Move In's "Rescan drives"). The
// cached fetchNtfsDrives above is what the section reads on mount; this is
// the escalation when the user has just plugged something in. Typed to the
// lsblk column set get_ntfs_devices() asks for, not `any`.
export interface NtfsDevice {
  name?: string;
  fstype?: string | null;
  label?: string | null;
  uuid?: string | null;
  mountpoint?: string | null;
  children?: NtfsDevice[];
}
export async function fetchNtfsDevices(): Promise<NtfsDevice[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<NtfsDevice[]>("ntfs_devices"); } catch { return null; }
}

// Boot runtime + desktop stack + updater (final reads)
export interface BootRuntimeCheck { name: string; passed: boolean; detail: string; }
export interface TelemetrySession {
  game_name: string;
  started_at: number | null;
  duration_s: number | null;
  avg_fps: number | null;
  p1_low_fps: number | null;
  stutter_count: number;
  scheduler: string;
  avg_latency_ms: number | null;
  p99_latency_ms: number | null;
}

export async function fetchTelemetryRecent(limit = 7): Promise<TelemetrySession[] | null> {
  if (!inTauriShell()) return null;
  try {
    const rows = await invoke<TelemetrySession[]>("telemetry_recent", { limit });
    return rows;
  } catch {
    return null;
  }
}

export interface CompatibilityGame {
  name: string;
  anticheat: string;
  status: "native" | "proton" | "tweaks" | "blocked";
  note: string;
  checked: string;
  source: string;
  source_url: string;
}

export async function fetchCompatibilityGames(): Promise<CompatibilityGame[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<CompatibilityGame[]>("compatibility_games"); } catch { return null; }
}


export interface LauncherEntry { id: string; label: string; installed: boolean; library_count: number | null; path: string; }
export async function fetchGamingLibrary(): Promise<LauncherEntry[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<LauncherEntry[]>("gaming_library"); } catch { return null; }
}
export interface StarterPack { name: string; desc: string; apps: { id: string; label: string; selected: boolean; description: string }[]; }
export async function fetchStarterPacks(): Promise<StarterPack[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<StarterPack[]>("starter_packs"); } catch { return null; }
}

export async function fetchBootRuntimeChecks(): Promise<BootRuntimeCheck[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<BootRuntimeCheck[]>("boot_runtime_checks"); } catch { return null; }
}

// Current user's display name for the dashboard greeting. Empty string
// means "no name available" — callers greet without a name rather than
// substituting a placeholder person.
export async function fetchUserName(): Promise<string | null> {
  if (!inTauriShell()) return null;
  try {
    const name = await invoke<string>("current_user_name");
    return name.trim() ? name : null;
  } catch { return null; }
}

// Phase 2 mutating (Updates + Repair/Diagnostics)
export async function invokeBootcUpgrade(): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  if (!confirmUserAction("Download and stage the next system update? It will require a reboot to apply.")) return "Cancelled.";
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("bootc_upgrade"));
}
export async function invokeBootcRollback(): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  if (!confirmUserAction("Roll back to the previous system deployment? This changes the next boot target.")) return "Cancelled.";
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("bootc_rollback"));
}
export async function invokeApplyStaged(): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  if (!confirmUserAction("Restart now to apply the staged system update?")) return "Cancelled.";
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("apply_staged"));
}
export async function invokeBootcSwitchBranch(branch: string): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  if (!confirmUserAction(`Switch the system update channel to ${branch}? This stages a new deployment.`)) return "Cancelled.";
  return await waitUpdateLaunch(await invoke<UpdateActionLaunch>("bootc_switch_branch", { branch }));
}
export async function invokeGuardianExecute(recipeId: string): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  if (!confirmUserAction(`Run Guardian fix ${recipeId}? It may change system configuration.`)) return "Cancelled.";
  return await invoke<string>("guardian_execute_recipe", { recipeId });
}
export async function dismissGuardianRecommendation(recipeId: string): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  return await invoke<string>("guardian_dismiss", { recipeId });
}

// Plasma HDR/VRR presets — apply_plasma_preset is the mutating half of the
// pair fetchPlasmaPresets lists (same shape as the PipeWire pair above).
export async function applyPlasmaPreset(preset: string, dryRun = false): Promise<{ ok: boolean; detail: string } | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<{ ok: boolean; detail: string }>("apply_plasma_preset", { preset, dryRun }); } catch { return null; }
}

// Driver/desktop introspection (Hardware, Desktop & displays).
export async function fetchLoadedKernelModules(): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("loaded_kernel_modules"); } catch { return null; }
}
export interface DesktopStackCheck { name: string; passed: boolean; detail: string; advisory: boolean; }
export async function fetchDesktopStackChecks(): Promise<DesktopStackCheck[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<DesktopStackCheck[]>("desktop_stack_checks"); } catch { return null; }
}
export async function fetchUpdaterAvailable(): Promise<boolean | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<boolean>("updater_available"); } catch { return null; }
}

// "Windows app -> Flatpak" chooser backing the App Store search box.
export interface FamiliarApp { windows_name: string; description: string; flatpak_id: string }
export async function fetchFamiliarApps(): Promise<FamiliarApp[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<FamiliarApp[]>("familiar_apps"); } catch { return null; }
}

export interface AppStreamApp { id: string; name: string; summary: string; icon_url: string }
export interface AppImageEntry { name: string; path: string; executable: boolean }
export interface InstallStatus { id: string; state: "running" | "complete" | "failed" | "unknown"; detail: string }
export async function searchAppStream(query: string): Promise<AppStreamApp[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<AppStreamApp[]>("appstream_search", { query }); } catch { return null; }
}
export async function fetchAppImages(): Promise<AppImageEntry[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<AppImageEntry[]>("appimage_list"); } catch { return null; }
}

export interface InstalledFlatpak { id: string; name: string; version: string; branch: string; arch: string; scope: "user" | "system"; icon_url: string }
interface InstallActionLaunch { job: string; state: "running"; detail: string; }
export async function fetchInstalledFlatpaks(): Promise<InstalledFlatpak[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<InstalledFlatpak[]>("installed_flatpaks"); } catch { return null; }
}
export async function makeAppImageExecutable(path: string): Promise<string> {
  return await invoke<string>("make_appimage_executable", { path });
}
export async function importAppImage(path: string): Promise<string> {
  return await invoke<string>("import_appimage", { path });
}
export async function uninstallFlatpak(id: string): Promise<string> {
  if (!confirmUserAction(`Uninstall ${id}? This removes the application from this system.`)) return "Cancelled.";
  const launch = await invoke<InstallActionLaunch>("uninstall_flatpak", { appId: id });
  if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Uninstall did not start.");
  const job = launch.job;
  for (let i = 0; i < 120; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 500));
    const state = await fetchInstallStatus(job);
    if (!state || state.state === "running") continue;
    if (state.state === "complete") return state.detail;
    throw new Error(state.detail);
  }
  throw new Error("Uninstall is still running; refresh Flatpak in a moment.");
}
export async function launchAppImage(path: string): Promise<string> { return await invoke<string>("launch_appimage", { path }); }
export async function installFlatpak(appId: string): Promise<string> {
  const launch = await invoke<InstallActionLaunch>("install_flatpak", { appId });
  if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Install did not start.");
  return launch.job;
}
export async function fetchInstallStatus(id: string): Promise<InstallStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<InstallStatus>("install_status", { job: id }); } catch { return null; }
}

// ---------------------------------------------------------------------
// Security tab: Kali distrobox lifecycle + host-side (Flatpak) tools grid.
// Kali create/export/remove run as background jobs (security_job_status),
// same running/complete/failed shape as installFlatpak/uninstallFlatpak
// above — polled longer since a "kali-linux-everything" pull can run many
// minutes. Reported status text, not a live percentage; see
// kyth-shared-rs's security_container module doc for why.
// ---------------------------------------------------------------------

export async function fetchKaliStatus(): Promise<boolean | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<boolean>("kali_status"); } catch { return null; }
}

export interface SecHostTool { flatpak: string; name: string; desc: string; installed: boolean }
export async function fetchSecHostTools(): Promise<SecHostTool[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<SecHostTool[]>("sec_host_tools"); } catch { return null; }
}

async function pollSecurityJob(job: string, maxIterations: number): Promise<string> {
  for (let i = 0; i < maxIterations; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 3000));
    const state = await invoke<InstallStatus>("security_job_status", { job }).catch(() => null);
    if (!state || state.state === "running") continue;
    if (state.state === "complete") return state.detail;
    throw new Error(state.detail);
  }
  throw new Error("Still running; check back in a moment.");
}
interface SecurityActionLaunch { job: string; state: "running"; detail: string; }
function securityJob(launch: SecurityActionLaunch): string { if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Security action did not start."); return launch.job; }

export async function createKaliBox(tier: "headless" | "default" | "everything"): Promise<string> {
  if (!confirmUserAction(`Create the Kali box (${tier} tools)? This pulls a container image and installs packages — it may take several minutes, longer for "everything".`)) return "Cancelled.";
  const job = securityJob(await invoke<SecurityActionLaunch>("kali_create", { tier }));
  return await pollSecurityJob(job, 600); // up to 30 minutes
}
export async function exportKaliApps(): Promise<string> {
  const job = securityJob(await invoke<SecurityActionLaunch>("kali_export", {}));
  return await pollSecurityJob(job, 100); // up to 5 minutes
}
export async function removeKaliBox(): Promise<string> {
  if (!confirmUserAction("Remove the Kali distrobox container? Files in your home directory are not affected.")) return "Cancelled.";
  const job = securityJob(await invoke<SecurityActionLaunch>("kali_remove", {}));
  return await pollSecurityJob(job, 60); // up to 3 minutes
}
export async function enterKaliTerminal(): Promise<string> { return await invoke<string>("kali_enter_terminal"); }

export async function installSecHostTool(flatpakId: string): Promise<string> {
  const job = securityJob(await invoke<SecurityActionLaunch>("sec_host_tool_install", { flatpakId }));
  return await pollSecurityJob(job, 240); // up to 12 minutes
}
export async function uninstallSecHostTool(flatpakId: string): Promise<string> {
  if (!confirmUserAction("Remove this tool?")) return "Cancelled.";
  const job = securityJob(await invoke<SecurityActionLaunch>("sec_host_tool_uninstall", { flatpakId }));
  return await pollSecurityJob(job, 60);
}
export async function launchSecHostTool(flatpakId: string): Promise<string> { return await invoke<string>("sec_host_tool_launch", { flatpakId }); }

// ---------------------------------------------------------------------
// Gaming tab: the install/launch/uninstall tool grid, the two one-shot
// Flatpak permission fixes (Discord screen share, OBS PipeWire capture),
// and the first-failure playbook / Fix My Game folder shortcuts. Mirrors
// page_gaming_tools_grid.py / page_gaming_fixes.py.
// ---------------------------------------------------------------------

export interface GamingTool { flatpak: string; name: string; desc: string; installed: boolean }
export async function fetchGamingTools(): Promise<GamingTool[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<GamingTool[]>("gaming_tools"); } catch { return null; }
}

async function pollGamingJob(job: string, maxIterations: number): Promise<string> {
  for (let i = 0; i < maxIterations; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 3000));
    const state = await invoke<InstallStatus>("gaming_job_status", { job }).catch(() => null);
    if (!state || state.state === "running") continue;
    if (state.state === "complete") return state.detail;
    throw new Error(state.detail);
  }
  throw new Error("Still running; check back in a moment.");
}
interface GamingActionLaunch { job: string; state: "running"; detail: string; }
function gamingJob(launch: GamingActionLaunch): string { if (launch.state !== "running" || !launch.job) throw new Error(launch.detail || "Gaming action did not start."); return launch.job; }

export async function installGamingTool(flatpakId: string): Promise<string> {
  const job = gamingJob(await invoke<GamingActionLaunch>("gaming_tool_install", { flatpakId }));
  return await pollGamingJob(job, 240); // up to 12 minutes
}
export async function uninstallGamingTool(flatpakId: string): Promise<string> {
  if (!confirmUserAction("Remove this tool?")) return "Cancelled.";
  const job = gamingJob(await invoke<GamingActionLaunch>("gaming_tool_uninstall", { flatpakId }));
  return await pollGamingJob(job, 60);
}
export async function launchGamingTool(flatpakId: string): Promise<string> { return await invoke<string>("gaming_tool_launch", { flatpakId }); }

export async function fixDiscordScreenshare(): Promise<string> { return await invoke<string>("fix_discord_screenshare"); }
export async function fixObsPipewire(): Promise<string> { return await invoke<string>("fix_obs_pipewire"); }
export async function openGameFolder(key: "compatdata" | "shadercache"): Promise<string> { return await invoke<string>("open_game_folder", { key }); }

// ---------------------------------------------------------------------
// Overlays / sched-ext / per-game profile builder — page_gaming_tools_perf.py.
// ---------------------------------------------------------------------

export interface GamingPerfStatus { mangohud_installed: boolean; gamescope_installed: boolean; vkbasalt_installed: boolean }
export async function fetchGamingPerfStatus(): Promise<GamingPerfStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<GamingPerfStatus>("gaming_perf_status"); } catch { return null; }
}

export interface ScxStatus { active: boolean; configured: string }
export async function fetchScxStatus(): Promise<ScxStatus | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<ScxStatus>("scx_status"); } catch { return null; }
}
export async function setScxScheduler(scheduler: "rusty" | "stop"): Promise<string> {
  const job = await invoke<string>("scx_set_scheduler", { scheduler });
  for (let i = 0; i < 20; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 1500));
    const state = await invoke<InstallStatus>("gaming_job_status", { job }).catch(() => null);
    if (!state || state.state === "running") continue;
    if (state.state === "complete") return state.detail;
    throw new Error(state.detail);
  }
  throw new Error("Still running; check back in a moment.");
}

export interface GameProfile { profile: string; hdr: boolean }
export async function fetchPerGameProfile(appid: string): Promise<GameProfile | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<GameProfile>("per_game_profile", { appid }); } catch { return null; }
}
export async function savePerGameProfile(appid: string, profile: string, hdr: boolean): Promise<string> {
  return await invoke<string>("save_per_game_profile", { appid, profile, hdr });
}
export interface ProtonDbResult { app_id: string; tier: string; detail: string }
export interface AntiCheatEntry { game: string; status: string; detail: string }
export async function fetchProtonDbMany(appIds: string[]): Promise<ProtonDbResult[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<ProtonDbResult[]>("protondb_lookup_many", { appIds }); } catch { return null; }
}
export async function fetchAntiCheatTable(): Promise<AntiCheatEntry[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<AntiCheatEntry[]>("anti_cheat_table"); } catch { return null; }
}

/** Feedback's send path — opens a prefilled kyth-os/kyth issue via
 * xdg-open. Throws like the other mutating wrappers so useSectionAction
 * can surface the failure. */
export async function invokeOpenFeedbackIssue(title: string, body: string): Promise<string> {
  if (!inTauriShell()) throw new Error("not in Tauri");
  return await invoke<string>("open_feedback_issue", { title, body });
}

// Work Setup parity: fixed Microsoft 365 web apps, PST discovery/import, and
// a timed sleep-inhibited focus session. The catalog is intentionally fixed;
// no arbitrary URL or command is accepted from the webview.
export async function openM365App(name: string): Promise<string> {
  if (!inTauriShell()) throw new Error("Microsoft 365 shortcuts are available from the installed Kyth Hub.");
  return await invoke<string>("open_m365_app", { name });
}
export async function createM365Shortcuts(): Promise<string> {
  if (!inTauriShell()) throw new Error("Microsoft 365 shortcuts are available from the installed Kyth Hub.");
  return await invoke<string>("create_m365_shortcuts");
}
export async function fetchPstFiles(): Promise<string[] | null> {
  if (!inTauriShell()) return null;
  try { return await invoke<string[]>("pst_files"); } catch { return null; }
}
export async function convertPst(path: string): Promise<string> {
  if (!inTauriShell()) throw new Error("Outlook import is available from the installed Kyth Hub.");
  const job = await invoke<string>("convert_pst", { path });
  return await waitHubJob(job, 3600);
}
export async function startFocusSession(minutes: number): Promise<string> {
  if (!inTauriShell()) throw new Error("Focus sessions are available from the installed Kyth Hub.");
  return await invoke<string>("focus_start", { minutes });
}
export async function stopFocusSession(id: string): Promise<string> {
  if (!inTauriShell()) throw new Error("Focus sessions are available from the installed Kyth Hub.");
  return await invoke<string>("focus_stop", { id });
}

// Downloaded executable / RPM MIME-handler workflow.  The native launcher
// supplies the path; these wrappers are deliberately narrow so the webview
// never receives a generic process or filesystem bridge.
export interface ExeHandlerCompatibility {
  level: "likely" | "unknown" | "unsupported";
  summary: string;
  detail: string;
}
export interface ExeHandlerInspection {
  path: string;
  basename: string;
  is_rpm: boolean;
  app_name: string | null;
  suggestion: string;
  flatpak_id: string | null;
  search_term: string;
  compatibility: ExeHandlerCompatibility | null;
  sha256_prefix: string | null;
  auto_bottles: boolean;
}
export interface ExeHandlerJob { job: string; state: "running" | "complete" | "failed" | "unknown"; detail: string; }

export async function takePendingExeHandler(): Promise<string | null> {
  if (!inTauriShell()) return null;
  return await invoke<string | null>("take_pending_exe_handler");
}
export async function inspectExeHandler(path: string): Promise<ExeHandlerInspection> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  return await invoke<ExeHandlerInspection>("exe_handler_inspect", { path });
}
export async function setExeHandlerAutoBottles(enabled: boolean): Promise<void> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  await invoke("exe_handler_set_auto_bottles", { enabled });
}
export async function openExeHandlerFlathub(searchTerm: string): Promise<void> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  await invoke("exe_handler_open_flathub", { searchTerm });
}
export async function isExeHandlerFlatpakInstalled(appId: string): Promise<boolean> {
  if (!inTauriShell()) return false;
  return await invoke<boolean>("exe_handler_flatpak_installed", { appId });
}
export async function launchExeHandlerFlatpak(appId: string): Promise<void> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  await invoke("exe_handler_launch_flatpak", { appId });
}
export async function startExeHandlerFlatpakInstall(appId: string): Promise<ExeHandlerJob> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  return await invoke<ExeHandlerJob>("install_flatpak", { appId });
}
export async function startExeHandlerBottles(path: string, allowUnsupported: boolean): Promise<ExeHandlerJob> {
  if (!inTauriShell()) throw new Error("Installer help is available from the installed Kyth Hub.");
  return await invoke<ExeHandlerJob>("exe_handler_start_bottles", { path, allowUnsupported });
}
