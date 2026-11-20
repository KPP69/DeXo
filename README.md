# DeXo

便携式桌面端多账号、多交易所策略交易工具。

下载压缩包解压后得到一个文件夹，内含 `DeXo.exe` 与 `data/` 目录，双击 exe 即可运行。所有配置、账户、数据库、日志、策略状态和交易记录都保存在 exe 所在文件夹内，移动整个文件夹后可继续使用。不依赖服务器，不要求用户安装数据库或单独配置运行环境（Windows 仍需满足 Tauri/WebView2 的系统要求）。

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 后端 | Rust + Tokio + Serde + async-trait |
| 数据库 | SQLite（rusqlite，WAL 模式，后端独占访问） |
| 日志 | tracing + 文件滚动 |
| 前端 | React + TypeScript + Vite（仅 UI 与交互，不含交易逻辑） |
| 精确数值 | rust_decimal（价格、数量、金额禁止使用 f64 计算） |

## 架构

系统由八个中心和一个执行层组成，交易核心全部在 Rust 后端：

1. **总览中心**（前端 UI）：账户、策略、持仓、资金与系统状态总览。
2. **账户中心** `src-tauri/src/accounts/`：多账户管理、启停、凭证隔离。
3. **数据中心** `src-tauri/src/data/`：统一行情、资金费率、余额、仓位与缓存，数据带时间戳，过期数据不得驱动新订单。
4. **策略中心** `src-tauri/src/strategies/`：只看数据、做判断、产生标准化订单意图，不直接调用交易所、不读私钥、不绕过风控。
5. **风控中心** `src-tauri/src/risk/`：全局 / 账户 / 策略 / 订单四层风控与全局 Kill Switch（只禁止新开仓，不等同于强制平仓）。
6. **交易中心** `src-tauri/src/trading/`：订单、成交、持仓记录与启动核对；交易所是真实仓位的权威来源。
7. **设置中心** `src-tauri/src/settings/`：便携数据目录、日志与用户配置。
8. **交易所中心** `src-tauri/src/exchanges/`：每个交易所一个 Adapter，统一 Trait 与统一数据模型。首批规划：Hyperliquid、Entropy、Arcus、Bulk、Lighter、AsterDex（逐一核实官方 API 后接入）。
9. **执行层** `src-tauri/src/execution/`：连接各中心，负责订单完整生命周期、双腿套利执行、失败回滚与异常恢复。网络超时不得直接判失败，必须用 `client_order_id` 查询核实。

## 便携数据目录

```
data/
├── accounts/
├── strategies/
├── database/
│   └── dexo.db
├── logs/
├── cache/
├── credentials/
└── settings.json
```

数据目录由 exe 位置决定（`current_exe()`），不写 AppData 等系统目录；目录不可写时明确报错退出，不静默回退。

## 开发

```bash
npm install        # 安装前端依赖
npx tauri icon src-tauri/icons/icon.svg  # 首次检出后生成二进制图标（仓库只保存 icon.svg）
npm run tauri dev  # 开发运行
npm run build      # 前端构建（tsc + vite）

cd src-tauri
cargo check
cargo test
cargo fmt --check
cargo clippy
```

分支规范：`main` 是唯一的长期主分支，不设长期 `dev` 分支。每个功能从最新 `main` 创建独立 `feature/*` 分支开发，GitHub Actions 自动编译、测试并生成 Windows x64 测试包（Actions Artifacts），实机测试通过后通过 PR 合并到 `main`。不创建正式 Release，不发布正式版本。

详细规范见 [DEVELOPMENT_GUIDE.md](DEVELOPMENT_GUIDE.md)，实现规格见 [IMPLEMENTATION_SPEC.md](IMPLEMENTATION_SPEC.md)。

## 当前状态

Phase 1（项目初始化）已完成：工程骨架、便携数据目录、SQLite 初始化、日志、基础启动与总览页状态展示。真实交易、交易所接入与各中心业务逻辑按阶段计划逐步实现，默认测试模式，真实交易必须由用户明确启用。
