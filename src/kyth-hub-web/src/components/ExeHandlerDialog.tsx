import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import {
  fetchInstallStatus,
  inspectExeHandler,
  isExeHandlerFlatpakInstalled,
  launchExeHandlerFlatpak,
  openExeHandlerFlathub,
  setExeHandlerAutoBottles,
  startExeHandlerBottles,
  startExeHandlerFlatpakInstall,
  takePendingExeHandler,
  type ExeHandlerInspection,
  type ExeHandlerJob,
} from "../services/liveData";
import { inTauriShell } from "../services/tauriEnv";

/** Handles `kyth-exe-handler` launches forwarded from the native MIME
 * launcher. It intentionally has no browser fallback: files are opened only
 * by the installed Tauri shell, whose Rust commands validate the path. */
export function ExeHandlerDialog() {
  const [inspection, setInspection] = useState<ExeHandlerInspection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [autoBottles, setAutoBottles] = useState(false);
  const [flatpakInstalled, setFlatpakInstalled] = useState(false);
  const [job, setJob] = useState<ExeHandlerJob | null>(null);
  const startedAutomatically = useRef(false);

  useEffect(() => {
    if (!inTauriShell()) return;
    const open = async (path: string) => {
      setError(null); setJob(null); setFlatpakInstalled(false); startedAutomatically.current = false;
      try {
        const next = await inspectExeHandler(path);
        setInspection(next); setAutoBottles(next.auto_bottles);
        if (next.flatpak_id) setFlatpakInstalled(await isExeHandlerFlatpakInstalled(next.flatpak_id).catch(() => false));
      } catch (reason) { setInspection(null); setError(String(reason)); }
    };
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await listen<string>("exe-handler", (event) => void open(event.payload));
      const initial = await takePendingExeHandler();
      if (initial) await open(initial);
    })();
    return () => unlisten?.();
  }, []);

  const startBottles = async (allowUnsupported = false) => {
    if (!inspection) return;
    if (inspection.compatibility?.level === "unsupported" && !allowUnsupported) {
      if (!window.confirm(`${inspection.compatibility.detail}\n\nTry it anyway?`)) return;
      return startBottles(true);
    }
    try {
      setError(null);
      setJob(await startExeHandlerBottles(inspection.path, allowUnsupported));
    } catch (reason) { setError(String(reason)); }
  };

  useEffect(() => {
    if (!inspection || inspection.is_rpm || !inspection.auto_bottles || startedAutomatically.current || inspection.compatibility?.level === "unsupported") return;
    startedAutomatically.current = true;
    void startBottles();
  }, [inspection]); // Deliberately runs only for a new native MIME launch.

  useEffect(() => {
    if (!job || job.state !== "running") return;
    const timer = window.setInterval(async () => {
      const next = await fetchInstallStatus(job.job);
      if (next) setJob({ job: next.id, state: next.state, detail: next.detail });
    }, 750);
    return () => window.clearInterval(timer);
  }, [job]);

  if (!inspection && !error) return null;
  const unsupported = inspection?.compatibility?.level === "unsupported";
  return (
    <div role="dialog" aria-modal="true" aria-label="Installer help" style={{ position: "fixed", inset: 0, zIndex: 50, background: "rgba(8, 12, 20, .72)", display: "grid", placeItems: "center", padding: 24 }}>
      <section className="glass dashboard-card" style={{ width: "min(620px, 100%)", padding: 28 }}>
        <h2 style={{ marginTop: 0 }}>{inspection?.is_rpm ? "KythOS — Installer Help" : "KythOS — Windows Application"}</h2>
        {inspection && <>
          <p style={{ opacity: .72, overflowWrap: "anywhere" }}>{inspection.basename}</p>
          <h3>{inspection.app_name ?? "Windows Application"}</h3>
          <p style={{ whiteSpace: "pre-line", lineHeight: 1.5 }}>{inspection.suggestion}</p>
          {inspection.compatibility && <>
            <p><strong style={{ color: unsupported ? "#f48771" : inspection.compatibility.level === "likely" ? "#73c991" : "#d7ba7d" }}>{inspection.compatibility.summary.toUpperCase()}</strong> — {inspection.compatibility.detail}</p>
            <p style={{ opacity: .72, fontSize: ".9em" }}>Kyth runs Windows software in an isolated compatibility environment. Apps that need Windows drivers, kernel anti-cheat, device services, or Microsoft Store components generally will not work.</p>
            {inspection.sha256_prefix && <p style={{ opacity: .6, fontSize: ".82em" }}>SHA-256: {inspection.sha256_prefix}…</p>}
          </>}
          {job && <p role="status"><strong>{job.state === "failed" ? "Could not open installer:" : "Installer workflow:"}</strong> {job.detail}</p>}
          {error && <p role="alert" style={{ color: "#f48771" }}>{error}</p>}
          {!inspection.is_rpm && <label style={{ display: "block", margin: "16px 0" }}><input type="checkbox" checked={autoBottles} onChange={async (event) => { const enabled = event.target.checked; setAutoBottles(enabled); try { await setExeHandlerAutoBottles(enabled); } catch (reason) { setError(String(reason)); } }} /> Automatically prepare and run future compatible Windows installers</label>}
          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            {inspection.is_rpm && <button onClick={() => { window.location.hash = "/apps"; setInspection(null); }}>Open App Store</button>}
            {!inspection.is_rpm && <button onClick={() => void startBottles()} disabled={job?.state === "running"}>{unsupported ? "Try Anyway" : "Run Windows Installer"}</button>}
            {inspection.flatpak_id && <button onClick={() => {
              if (flatpakInstalled) void launchExeHandlerFlatpak(inspection.flatpak_id!).then(() => setInspection(null)).catch((reason) => setError(String(reason)));
              else void startExeHandlerFlatpakInstall(inspection.flatpak_id!).then(setJob).catch((reason) => setError(String(reason)));
            }}>{flatpakInstalled ? "Launch Linux Version" : "Install Linux Version"}</button>}
            <button onClick={() => void openExeHandlerFlathub(inspection.search_term).catch((reason) => setError(String(reason)))}>Search Flathub</button>
            <button onClick={() => setInspection(null)} disabled={job?.state === "running"}>Cancel</button>
          </div>
        </>}
        {error && !inspection && <><p role="alert" style={{ color: "#f48771" }}>{error}</p><button onClick={() => setError(null)}>Close</button></>}
      </section>
    </div>
  );
}
