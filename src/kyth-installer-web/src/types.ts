export type InstallMode = "wipe" | "resize_ntfs" | "alongside" | "free_space" | "manual";
export type Phase = "prepare" | "storage" | "image" | "configure" | "secure_boot" | "complete";
export type Lifecycle = "idle" | "validated" | "partitioning" | "installing" | "done" | "failed";

export interface Disk { name: string; size_bytes?: number; model?: string; removable?: boolean; current?: boolean; }
export interface Partition { name: string; size_bytes?: number; fstype?: string; label?: string; mountpoint?: string; current?: boolean; in_use?: boolean; efi?: boolean; }
export interface FreeRegion { start_bytes: number; end_bytes?: number; size_bytes: number; }
export interface SourceStatus { available?: boolean; kind?: string; message?: string; }
export interface Config { source_image: string; is_live: boolean; source?: SourceStatus; }
export interface PendingOperation { index?: number; kind: string; params?: Record<string, unknown>; }
export interface TransactionReport { status?: string; phase?: Phase; lifecycle?: Lifecycle; message?: string; [key: string]: unknown; }
export interface RescueProbe { log_tail?: string; transaction?: TransactionReport; rescue_guidance?: { message?: string; bootable?: boolean; severity?: string }; [key: string]: unknown; }

export interface InstallRequest {
  disk: string; install_mode: InstallMode; target_partition: string; resize_partition: string;
  resize_gib: number; free_region_start: number; free_region_end: number; hostname: string;
  timezone: string; locale: string; keymap: string; username: string; password: string; mok_password: string; kernel: string;
  confirm_backup: boolean; confirm_erase: boolean; confirm_current: boolean;
}

export type InstallerEvent =
  | { type: "log"; text: string }
  | { type: "progress"; value: number }
  | { type: "stats"; [key: string]: unknown }
  | { type: "phase"; phase: Phase }
  | { type: "done"; mok_state?: string }
  | { type: "error"; message: string };
