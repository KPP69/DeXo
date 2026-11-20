import { useAppStatus } from "../hooks/useAppStatus";

/** Overview center: shows the backend startup status reported over Tauri IPC. */
export default function OverviewPage() {
  const { status, error } = useAppStatus();

  return (
    <section>
      <h1>总览</h1>
      {error !== null && <p className="error">后端状态读取失败：{error}</p>}
      {status === null && error === null && <p className="muted">正在读取后端状态…</p>}
      {status !== null && (
        <dl className="status-grid">
          <dt>应用版本</dt>
          <dd>{status.version}</dd>
          <dt>数据目录</dt>
          <dd>{status.dataDir}</dd>
          <dt>数据库路径</dt>
          <dd>{status.databasePath}</dd>
          <dt>数据库状态</dt>
          <dd>{status.databaseReady ? "已初始化" : "未就绪"}</dd>
        </dl>
      )}
    </section>
  );
}
