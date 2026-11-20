# DeXo 实现规格

本文件记录各阶段的具体实现规格与验收标准，与 DEVELOPMENT_GUIDE.md 配套。阶段完成状态以实际测试结果为准，未验证的项目不得标记为完成。

## 目录结构

```
DeXo/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── error_report.rs        # 启动失败的原生错误反馈（Windows 对话框）
│       ├── exchanges/             # mod.rs, traits.rs, hyperliquid.rs, entropy.rs,
│       │                          # arcus.rs, bulk.rs, lighter.rs, aster.rs
│       ├── accounts/              # mod.rs, account_manager.rs, credential_store.rs
│       ├── strategies/            # mod.rs, traits.rs, funding_arbitrage/
│       ├── data/                  # mod.rs, market.rs, funding.rs, account.rs,
│       │                          # positions.rs, cache.rs
│       ├── risk/                  # mod.rs, global.rs, account.rs, strategy.rs, order.rs
│       ├── trading/               # mod.rs, database.rs, orders.rs, fills.rs,
│       │                          # positions.rs, reconciliation.rs
│       ├── execution/             # mod.rs, engine.rs, order_manager.rs,
│       │                          # arbitrage_executor.rs, recovery.rs
│       ├── settings/              # mod.rs, paths.rs, logging.rs
│       ├── models/                # mod.rs（Phase 2 定义统一模型）
│       └── state/                 # mod.rs（Tauri 共享状态）
├── src/                           # React 前端：components/, pages/, hooks/, services/, App.tsx
├── public/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── README.md
├── DEVELOPMENT_GUIDE.md
├── IMPLEMENTATION_SPEC.md
└── .gitignore
```

## Phase 1：项目初始化（当前阶段）

实现内容：

- 分支：`main` 为唯一长期主分支（不建长期 `dev` 分支）；本阶段从 `main` 创建 `feature/project-bootstrap`，PR 目标为 `main`，合并前须通过 GitHub Actions 构建与实机测试，不创建 Release。
- CI：`.github/workflows/ci.yml` 在 Windows runner 上执行 npm ci、图标生成、前端构建、cargo fmt --check、cargo clippy（-D warnings）、cargo test、Tauri Release 构建（--no-bundle），并将 `DeXo.exe` 作为测试包上传到 Actions Artifacts（保留 14 天）。
- Tauri 2 工程：Rust 后端锁定 `tauri =2.12.6`、`tauri-build =2.7.5`；前端 React 19 + TypeScript + Vite，`@tauri-apps/api` / `@tauri-apps/cli` 2.12.5，版本精确锁定并提交 lockfile（`Cargo.lock`、`package-lock.json`）。
- 便携目录：`settings::paths::data_dir()` 按 DEVELOPMENT_GUIDE.md 第 10 节实现；`DataPaths::from_root` 给出全部子路径；`ensure_data_dirs` 启动时创建 `accounts/`、`strategies/`、`database/`、`logs/`、`cache/`、`credentials/`、`webview/` 并做可写探针，失败返回 `PathError` 并明确报错退出，不回退系统目录。
- 主窗口在 Rust 中用 `WebviewWindowBuilder` 创建，Windows 上以 `data_directory(<exe>/data/webview/)` 传绝对路径，保证 WebView 数据也留在便携目录内；`tauri.conf.json` 不声明窗口。
- SQLite：`trading::database::initialise` 在 `data/database/dexo.db` 建库，启用 WAL 与 foreign keys，创建 `dexo_meta` 元数据表并写入 `schema_version = 1`（`INSERT OR IGNORE`，重开不覆盖已有数据）。
- 日志：tracing 同时输出到控制台与 `data/logs/dexo.log.<日期>`（按日滚动，`tracing-appender`），级别可用 `RUST_LOG` 覆盖，默认 `info`。
- 启动失败反馈：数据目录或数据库初始化失败时，Windows 弹原生错误对话框（`error_report`，windows-sys MessageBoxW，仅 Win32_Foundation 与 Win32_UI_WindowsAndMessaging），注明失败阶段，不含任何敏感信息，随后退出。
- 前端骨架：左侧导航七个中心页面（总览、交易中心、策略中心、账户中心、数据中心、风控中心、系统设置），总览页通过 `app_status` Command 展示后端版本、数据目录、数据库路径与就绪状态；其余页面为骨架占位并注明实现阶段。
- Tauri 权限：capabilities 仅授予 `core:default`，不引入任何插件。
- 图标：仓库只保存 `src-tauri/icons/icon.svg`，二进制图标（ico/png/icns）由 `npx tauri icon src-tauri/icons/icon.svg` 生成，不提交二进制文件。

验收标准：

- 项目可以正常编译；Tauri 桌面窗口可以启动。
- `data/` 目录自动创建；数据库可以初始化；日志正常输出。
- 不依赖外部数据库。
- `cargo check`、`cargo test`、`cargo fmt --check`、`cargo clippy` 通过；`npm run build` 通过。

## Phase 2：基础模型

任务：定义所有共享数据结构（Account、Exchange、FundingInfo、OrderBook、Balance、Position、OrderRequest、Order、Fill、Trade、RiskResult）、统一订单状态（见 DEVELOPMENT_GUIDE.md 第 7 节状态集）、错误类型、交易所 Adapter Trait、策略 Trait、风控接口。金额、价格、数量一律 rust_decimal；每个订单携带 account_id、strategy_id、exchange、symbol、client_order_id。本阶段引入 `rust_decimal` 与 `async-trait` 依赖（Phase 1 未提前引入未使用的依赖）。

验收：数据结构编译通过；单元测试通过；不同交易所可以使用统一模型。

## Phase 3：账户中心

任务：账户加载、账户分组、多账号管理、账户启停、凭证存储接口（默认操作系统凭证存储，便携模式为主密码加密凭证库，明文存储必须经用户确认）。

验收：可以加载多个账户；不同账户严格隔离；日志与前端不输出敏感信息；提供账户删除与凭证清除。

## Phase 4：第一个交易所（Hyperliquid）

任务：按官方 API 接入（接入前核实订单类型、签名、限频、测试环境）：账户余额、持仓查询、资金费率、Order Book、订单提交、订单查询、订单取消、成交记录，全部映射到统一数据结构，不支持的能力返回 Unsupported。

验收：数据读取正常；统一模型映射正常；测试环境下订单闭环正常；异常情况可以正确处理。

## Phase 5：数据中心

任务：统一行情与资金费率接口、账户与持仓数据、数据缓存、数据时间戳、过期数据保护。

验收：策略可以通过统一接口获取数据；数据过期能够被识别并阻止新订单；策略不直接依赖交易所具体实现。

## Phase 6：交易中心

任务：SQLite 订单、成交、持仓记录与交易历史；数据库事务；启动状态核对（本地记录与交易所未完成订单、成交、实际持仓对比，差异生成异常记录）。

验收：交易数据可持久化；程序重启后可以读取；不存在不合理的状态覆盖。

## Phase 7：执行层

任务：单腿下单、订单状态管理、订单查询、超时处理（用 client_order_id 查询核实，禁止盲目重提）、订单恢复、账户并发控制（按账户/交易对的合理粒度，锁范围最小化）。

验收：单腿交易闭环正常；部分成交可处理；网络超时不会盲目重复下单；重启后可以恢复未完成订单。

## Phase 8：风控中心

任务：全局风控（全局资金限制、总敞口、最大日亏损、全局熔断）、账户风控（单账户最大仓位与资金使用、杠杆限制、余额检查）、策略风控（策略资金与持仓限制、策略最大亏损、启停限制）、订单风控（下单数量、价格偏差、滑点、最小下单量、数据有效性）、全局 Kill Switch（只禁止新开仓，保留撤单/减仓/平仓，记录原因，可手动解除）。

验收：未通过风控的订单不能执行；风控拒绝原因可查询；Kill Switch 能阻止新开仓且不触发强制平仓。

## Phase 9：策略中心

任务：策略接口实现、第一个资金费率套利策略、策略启停、策略参数管理、策略运行状态。

验收：策略只通过数据中心读取数据；策略只产生订单请求；所有订单都经过风控与执行层。

## Phase 10：双腿套利执行

任务：双腿下单、部分成交处理、双腿数量核对（以实际成交数量为准）、reduce-only 回滚、UNKNOWN 状态恢复、异常处理（回滚失败进入异常状态、报警、必要时停止相关策略开仓）。

验收：各类失败场景通过模拟测试；不会把超时误认为订单失败；回滚数量以实际成交数量为依据；无法恢复时停止相关策略开仓并报警；不承诺双腿原子成交。

## Phase 11：前端

任务：完成主要页面（总览、交易中心、策略中心、账户中心、数据中心、风控中心、系统设置）、账户管理、策略管理、交易记录、持仓展示、风控展示、系统设置；状态以后端为准，前端不接触私钥、不直连交易所、不绕过风控。

验收：前后端通信正常；账户和策略状态实时更新；用户操作能够正确反馈。

## Phase 12：其他交易所

依次接入 Entropy、Arcus、Bulk、Lighter、AsterDex，每个交易所单独开发、单独测试；接入前核实官方 API，不得复制未经核实的 API 实现，资料不足先报告缺失信息。

## Phase 13：正式打包

任务：Windows Release 构建、exe 启动测试、便携目录测试、数据库恢复测试、多账户压力测试、异常退出测试、凭证安全检查。

验收：解压即可运行；用户数据保存在程序目录；移动文件夹后可以继续使用；不需要单独安装数据库；程序异常退出后可以恢复；不存在真实凭证泄漏。

## 测试总则

- 必须建立模拟交易所 Adapter，覆盖：下单成功、下单失败、网络超时、部分成交、完全成交、订单取消、重复订单、双腿不平衡、回滚失败、程序中断、重启恢复、多账户并发。
- 没有通过模拟异常测试的交易功能不得进入真实交易阶段。
- 真实交易必须由用户明确启用；默认测试模式；开发过程中严禁用真实账户下单。
