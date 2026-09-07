import { lazy, Suspense } from "react";
import { Route, Routes, useLocation } from "react-router-dom";
import { Sidebar } from "./components/Sidebar";
import { Topbar } from "./components/Topbar";
import { ExeHandlerDialog } from "./components/ExeHandlerDialog";

const Dashboard = lazy(() => import("./pages/Dashboard").then(({ Dashboard: page }) => ({ default: page })));
const Play = lazy(() => import("./pages/Play").then(({ Play: page }) => ({ default: page })));
const Apps = lazy(() => import("./pages/Apps").then(({ Apps: page }) => ({ default: page })));
const ThisPc = lazy(() => import("./pages/ThisPc").then(({ ThisPc: page }) => ({ default: page })));
const MoveIn = lazy(() => import("./pages/MoveIn").then(({ MoveIn: page }) => ({ default: page })));
const Updates = lazy(() => import("./pages/Updates").then(({ Updates: page }) => ({ default: page })));

const crumbFor: Record<string, string> = {
  "/": "Home",
  "/play": "Play",
  "/apps": "Apps",
  "/this-pc": "This PC",
  "/move-in": "Move In",
  "/updates": "Updates",
};

export function App() {
  const location = useLocation();
  const crumb = crumbFor[location.pathname] ?? "Home";

  return (
    <>
      <div className="bg-glow" />
      <div className="app-shell">
        <Sidebar />
        <main className="scroll-area main-content" style={{ flex: 1, padding: "0 24px 24px", overflowY: "auto" }}>
          <Topbar crumb={crumb} />
          <Suspense fallback={<div className="glass dashboard-card card-copy">Loading Hub page…</div>}>
            <Routes>
              <Route path="/" element={<Dashboard />} />
              <Route path="/play" element={<Play />} />
              <Route path="/apps" element={<Apps />} />
              <Route path="/this-pc" element={<ThisPc />} />
              <Route path="/move-in" element={<MoveIn />} />
              <Route path="/updates" element={<Updates />} />
            </Routes>
          </Suspense>
        </main>
      </div>
      <ExeHandlerDialog />
    </>
  );
}
