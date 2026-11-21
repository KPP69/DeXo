import { useState } from "react";
import Sidebar from "./components/Sidebar";
import PlaceholderPage from "./components/PlaceholderPage";
import OverviewPage from "./pages/OverviewPage";

const PAGES = [
  "总览",
  "交易中心",
  "策略中心",
  "账户中心",
  "数据中心",
  "风控中心",
  "系统设置",
] as const;

const PAGE_PHASES: Record<string, string> = {
  交易中心: "Phase 6 / Phase 11",
  策略中心: "Phase 9 / Phase 11",
  账户中心: "Phase 3 / Phase 11",
  数据中心: "Phase 5 / Phase 11",
  风控中心: "Phase 8 / Phase 11",
  系统设置: "Phase 11",
};

export default function App() {
  const [active, setActive] = useState<string>("总览");

  return (
    <div className="layout">
      <Sidebar pages={PAGES} active={active} onSelect={setActive} />
      <main className="content">
        {active === "总览" ? (
          <OverviewPage />
        ) : (
          <PlaceholderPage title={active} phase={PAGE_PHASES[active] ?? "后续阶段"} />
        )}
      </main>
    </div>
  );
}
