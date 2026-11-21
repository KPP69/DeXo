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
├── AGENTS.md
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
- 路径解析（2026-11-20 修正）：`data_dir()` 与 `data_dir_for_exe()` 返回 `Result<PathBuf, PathError>`，`current_exe()` 失败或 exe 路径无有效父目录时明确报错，经启动 stage 机制弹原生对话框后退出；禁止静默回退到当前工作目录或系统目录（AGENTS.md 第 6.3、21.3 节）。
- 已知缺口（不阻塞 Phase 1）：尚未设置 Tauri `appDirectoriesOverride`。当前代码不调用 Tauri 目录 API、未引入插件，只有主窗口 WebView 数据目录被钉到 `data/webview/`；第一次使用 Tauri Directory API（app_config_dir / app_data_dir / app_local_data_dir / app_cache_dir / app_log_dir）或引入相关插件时，必须先按 per-directory 方式把这些目录映射到 `data/` 下并做实机验证（AGENTS.md 第 6.5、21.4 节）。

验收标准：

- 项目可以正常编译；Tauri 桌面窗口可以启动。
- `data/` 目录自动创建；数据库可以初始化；日志正常输出。
- 不依赖外部数据库。
- `cargo check`、`cargo test`、`cargo fmt --check`、`cargo clippy` 通过；`npm run build` 通过。

## Phase 2：基础模型

任务：定义所有共享数据结构（Account、Exchange、FundingInfo、OrderBook、Balance、Position、OrderRequest、Order、Fill、Trade、RiskResult）、统一订单状态（见 DEVELOPMENT_GUIDE.md 第 7 节状态集）、错误类型、交易所 Adapter Trait、策略 Trait、风控接口。金额、价格、数量一律 rust_decimal；每个订单携带 account_id、strategy_id、exchange、symbol、client_order_id。本阶段引入 `rust_decimal` 与 `async-trait` 依赖（Phase 1 未提前引入未使用的依赖）。

验收：数据结构编译通过；单元测试通过；不同交易所可以使用统一模型。

实现记录（2026-11-21）：

- `models/` 拆分为 account / market / order / risk / error 五个子模块：全部金额、价格、数量、费率为 `Decimal`；时间戳统一 Unix 毫秒；`Account` 不含任何凭证字段（凭证与账户配置分离，Phase 3 存储）。
- `Exchange` 带 `kind`：`Standalone` 或 `Namespace`（子 DEX / 市场命名空间，`parent` 指向承载其传输的交易所）。Entropy 按产品层面的独立交易所条目建模（id `entropy`），kind 为 Namespace、parent 为 `hyperliquid`，经 Hyperliquid Adapter 接入，不建独立 Adapter。
- 订单状态机：`OrderStatus` 14 个状态与受控转移表 `can_transition_to`（含终态判定）；超时只能进入 UNKNOWN 并经 RECONCILING 核对，禁止从 OPEN 直接转 FAILED 或重提。
- 错误类型：`ExchangeError`（含 `outcome_is_ambiguous` 标记超时/网络/未知三类结果不确定错误）、`DataError`（含 StaleData）、`StrategyError`。
- `ExchangeAdapter`（exchanges/traits.rs）：实例由账户中心按账户绑定，方法不传 account_id / 凭证；含 place_order / get_order / cancel_order / get_fills；不支持的能力必须返回 Unsupported。测试内含 Mock Adapter 验证统一模型全流程。
- `Strategy`（strategies/traits.rs）+ `DataAccess`（data/mod.rs）：策略只经数据中心读数、只产出 OrderRequest；`RiskCheck`（risk/mod.rs）：四层顺序检查接口，RiskResult 拒绝必须带层级与原因。
- 本地验证：harness 单元测试 26 个全过（其中 17 个为本阶段新增）；fmt / cargo xwin check / clippy -D warnings 全过；npm run build 通过。Windows CI 结果见 PR #1 的 Actions 运行记录。

## Phase 3：账户中心

任务：账户加载、账户分组、多账号管理、账户启停、凭证存储接口（默认操作系统凭证存储，便携模式为主密码加密凭证库，明文存储必须经用户确认）。

验收：可以加载多个账户；不同账户严格隔离；日志与前端不输出敏感信息；提供账户删除与凭证清除。

实现记录（2026-11-21）：

- `AccountManager`（accounts/account_manager.rs）：从 `data/accounts/<id>.json` 加载账户配置（仅非敏感字段）；严格校验——id 限 [A-Za-z0-9_-] 且同时是文件名、重复 id 拒绝、损坏文件使整次加载失败并指明文件；支持按交易所分组、启停（持久化）、新增与删除；写入采用临时文件 + rename 防截断。启动流程已接入：账户加载为独立启动阶段，失败明确报错退出。
- `Account` 模型启用 `deny_unknown_fields`：账户文件里出现凭证类未知字段直接加载失败，杜绝明文凭证混入普通配置（有测试覆盖）。
- `CredentialStore` 接口（accounts/credential_store.rs）：put/get/delete + backend_name；`Credentials` 的 Debug 输出脱敏（只显示字段名）。本阶段交付接口与内存后端（测试/开发用，进程退出即消失）；操作系统凭证存储与主密码加密库两个真实后端随账户 UI 接线时实现，在此之前不落盘任何真实凭证——这是安全默认，不是缺口冒充。
- 本地验证：harness 单元测试 37 个全过（本阶段新增 11 个）；fmt / cargo xwin clippy -D warnings 全过。Windows CI 结果见 PR #1 的 Actions 运行记录。

## Phase 4：第一个交易所（Hyperliquid）

任务：按官方 API 接入（接入前核实订单类型、签名、限频；只接主网，不接测试网）：账户余额、持仓查询、资金费率、Order Book、订单提交、订单查询、订单取消、成交记录，全部映射到统一数据结构，不支持的能力返回 Unsupported。

验收：数据读取正常；统一模型映射正常；主网小额限价单下单→查单→撤单闭环正常（须用户指定并显式启用）；异常情况可以正确处理。

实现记录（2026-11-21，第一步：只读 + 写路径就绪）：

- 签名方案按产品负责人批准采用 hypersdk 0.3.1（精确锁定，default-features 关、仅 http；评估记录见本节与 Cargo.toml 注释）。主钱包永不进程序：查询用主账户地址，签名只用 API wallet（agent）私钥，approveAgent 由用户在 Hyperliquid 前端自行完成。
- `HyperliquidAdapter`（exchanges/hyperliquid.rs）实现完整 ExchangeAdapter：get_funding（metaAndAssetCtxs，下一次资金费时间为整点边界推算）、get_orderbook（SDK 无 HTTP l2Book，直连 /info 并分类错误）、get_balance / get_positions（clearinghouseState；余额 total 取 cross margin account_value、available 取 withdrawable）、get_fills（userFillsByTime）、get_order（orderStatus 按 cloid 查，未找到返回 ORDER_NOT_FOUND）、place_order / cancel_order（cancel 先查单确认资产与状态，终态直接返回，撤单后回查确认，查不到则 CancelPending）。
- 写路径双重门：signer 存在且 `writes_enabled` 显式开启（默认关）才允许发单；市价单 = IOC + 保护价（OrderRequest.price 必填）；价格按市场 tick 取整、数量按 szDecimals 向零截断；client_order_id 必须是 cloid 格式（0x + 32 hex），提供 `new_client_order_id()` 生成；nonce 为毫秒时间戳 + 原子单调递增。
- HIP-3：构造参数 `dex`（如 "io"）切换命名空间，perp_dexes/perps_from 解析市场，产品 exchange_id 独立（如 entropy）。
- 错误分类：hypersdk 的 /info 错误是 anyhow 纯文本，按内容分类（429/Too Many Requests → RateLimited，实测命中并已修正分类器）；写操作失败默认 Unknown（结果不确定，必须先查单）。
- 验证：本地单元测试 43 个全过（含 cloid、funding/orderbook JSON 解析、状态映射、写门默认关）；主网只读实测通过（零地址 + 公开行情：BTC 资金费率、BTC 订单簿 20 档、余额、持仓 0，均经适配器真实返回）。下单闭环未验证——须用户提供 API wallet 并指定小额单后单独进行。

## Phase 5：数据中心

任务：统一行情与资金费率接口、账户与持仓数据、数据缓存、数据时间戳、过期数据保护。

验收：策略可以通过统一接口获取数据；数据过期能够被识别并阻止新订单；策略不直接依赖交易所具体实现。

实现记录（2026-11-21）：`DataCenter`（data/mod.rs）实现 DataAccess，按账户持有 Arc<dyn ExchangeAdapter>，策略只见统一接口、不见交易所实现。四类数据各有带时间戳的 TTL 缓存（data/cache.rs 的 TtlCache）与独立新鲜度上限（FreshnessLimits 默认：funding 30s、order book 5s、balance 15s、positions 15s）。读取语义统一：新鲜缓存直接返回；过期则向适配器刷新并回填；刷新失败且只有过期数据时返回 DataError::StaleData（绝不静默把过期数据喂给策略），无缓存时返回适配器错误（Backend）；未知交易所/账户返回 NotFound。时钟可注入，本地测试 6 个覆盖缓存命中不重复请求、过期刷新、过期+刷新失败 → StaleData、无缓存失败 → Backend、未知对象 → NotFound。WebSocket 实时推送未做，随行情订阅需求在后续阶段接入（缓存层结构已为其预留）。

## Phase 6：交易中心

任务：SQLite 订单、成交、持仓记录与交易历史；数据库事务；启动状态核对（本地记录与交易所未完成订单、成交、实际持仓对比，差异生成异常记录）。

数据库迁移必须执行严格规则（AGENTS.md 第 11.4 节）：仅对真正的全新数据库执行初始 Schema 创建并写入版本；已有业务表但缺少 `dexo_meta` 或有效版本记录时必须拒绝自动初始化；不得用 `INSERT OR IGNORE` 掩盖版本异常（Phase 1 的 database.rs 目前只有 `dexo_meta` 一张表，其 INSERT OR IGNORE 写法必须在本阶段按此规则改造）；不得静默重建或覆盖用户数据库；迁移按明确版本顺序、尽量在事务中执行，失败保留可诊断信息，并考虑备份与恢复策略。

验收：交易数据可持久化；程序重启后可以读取；不存在不合理的状态覆盖。

实现记录（2026-11-21）：Schema 升至 v2。database.rs 已按第 11.4 节严格规则重写：只有真正空库才在单事务内建全量 Schema 并写入版本；已有表但缺 dexo_meta、dexo_meta 缺版本行或版本不可解析、版本高于本程序，一律拒绝并给出可诊断错误；版本为 2 但业务表缺失时报告损坏、绝不静默重建；v1 → v2 迁移在事务内建表并用普通 UPDATE 推进版本（已删除原 INSERT OR IGNORE 写法），旧数据保留。新增四张表：orders（以 client_order_id 为主键）、fills（fill_key 去重，重复记录显式返回未插入）、position_snapshots（只追加快照，最新一条为本地视图）、anomalies（异常记录）。订单写入受状态机守卫：upsert 只允许同状态刷新或 can_transition_to 允许的迁移，非法迁移（如 Open 直达 Cancelled、终态再变更）被拒绝且不改动原记录。reconciliation.rs 实现启动核对：本地未完成订单逐笔按 cloid 查交易所，消失 → LocalOrderMissingRemotely 异常；状态/已成交量不一致 → OrderStateMismatch 异常且仅在合法迁移时向权威记录收敛；持仓与最新快照比对，不一致 → PositionMismatch 并追加远程快照收敛，无本地记录 → PositionNotRecorded；本地非零持仓远程消失 → PositionMismatch。异常全部落库可查；除「订单不存在」外的交易所错误使核对整体报错，不静默半途而废。本地测试增至 63 个（迁移与拒绝路径 8 个、订单守卫、成交去重、快照最新值、核对三类异常与零异常路径）。

## Phase 7：执行层

任务：单腿下单、订单状态管理、订单查询、超时处理（用 client_order_id 查询核实，禁止盲目重提）、订单恢复、账户并发控制（按账户/交易对的合理粒度，锁范围最小化）。

验收：单腿交易闭环正常；部分成交可处理；网络超时不会盲目重复下单；重启后可以恢复未完成订单。

实现记录（2026-11-21）：执行层落地于 execution/ 三个模块。`OrderManager`（order_manager.rs）是唯一能下单/撤单/收敛订单的路径：提交先落库（Created），经 RiskCheck 接口（Phase 8 提供实现，本层只依赖接口；上下文含 Kill Switch 状态，由本层的原子开关承载、Phase 8 驱动）后依次 RiskChecked → Submitting 再发往交易所；同 client_order_id 重复提交直接返回已存订单，永不二次发单（幂等）。结果不确定（超时、网络错误、限频、Unknown）一律转 Unknown 并按 cloid 查单收敛——查到则按权威状态收敛，查不到保持 Unknown 交恢复流程，绝不重提；只有交易所明确拒绝（InvalidRequest、Remote 错误等）才转 Failed。远端状态收敛（apply_remote_order）只走状态机合法迁移，Cancelled 经 CancelPending 中转，收敛不了的拒绝并保留原记录。撤单超时同样转 CancelPending 后查单确认。并发控制按（账户，交易对）粒度异步锁，不同交易对并行；数据库互斥锁从不跨 await 持有。`recovery.rs` 实现启动恢复：逐笔查询本地未完成订单并收敛，统计 converged / still_open / resolved_terminal；交易所查无此单时落异常记录，Unknown 单额外转 Reconciling 标记待复核；其他查询错误使恢复整体报错，不带病启动。`ExecutionEngine`（engine.rs）是策略与 UI 的统一入口（submit/cancel/query/recover）。本地测试增至 71 个：脚本化模拟 Adapter 覆盖下单成功、超时后查单命中（断言只发单一次）、超时后查无此单保持 Unknown、明确拒绝转 Failed、风控拒绝不触达交易所、重复提交幂等、撤单经 CancelPending 收敛、恢复收敛 Filled + 缺失订单落异常。真实账户下单仍未启用（须用户显式启用 + Phase 4 闭环验收）。

## Phase 8：风控中心

任务：全局风控（全局资金限制、总敞口、最大日亏损、全局熔断）、账户风控（单账户最大仓位与资金使用、杠杆限制、余额检查）、策略风控（策略资金与持仓限制、策略最大亏损、启停限制）、订单风控（下单数量、价格偏差、滑点、最小下单量、数据有效性）、全局 Kill Switch（只禁止新开仓，保留撤单/减仓/平仓，记录原因，可手动解除）。

验收：未通过风控的订单不能执行；风控拒绝原因可查询；Kill Switch 能阻止新开仓且不触发强制平仓。

实现记录（2026-11-21）：`RiskCenter`（risk/mod.rs）实现 RiskCheck，按全局 → 账户 → 策略 → 订单顺序执行四层，第一层拒绝即终止并记录。检查保持同步纯函数契约：中心对一份状态快照（余额、持仓、当日盈亏、参考价及其时间戳）评估，快照由应用层从数据中心刷新（update_state）；查不到的数据按「不可验证」处理而不是按零风险放行——开仓单没有余额记录直接拒绝，没有参考价或参考价过期（默认 30 秒）在订单层拒绝。全局层：Kill Switch（开启后只拒开仓，reduce-only/撤单/平仓不受影响，不触发强平；激活记录原因与时间，只能手动解除）、当日亏损熔断（达到上限后停开仓）、全账户总敞口上限。账户层：余额可用额 × 杠杆上限的保证金可行性、账户总敞口上限、单笔上限、已有持仓杠杆超限时拒绝加仓。策略层：停用策略禁开仓（减仓放行）、策略单笔上限、策略当日亏损上限。订单层：数量为正、限价单价格有效、最小下单额（默认 10，与 Hyperliquid 场内最小额一致）、限价偏离参考价上限（默认 10%）、数据新鲜度。敞口增量按 (|q+d| − |q|) × 参考价 计算，纯减仓增量为零、永不被敞口/亏损类规则拦截；reduce-only 订单增量恒为零。每次拒绝写入有界日志（最近 200 条，含时间、账户、策略、client_order_id、层级、原因），recent_rejections() 可查——满足「拒绝原因可查询」。本地测试增至 83 个（四层各有专项用例 + 中心级 Kill Switch/层级优先/放行用例）。执行层 OrderManager 的 Kill Switch 标志后续与 RiskCenter 的开关在应用接线时统一由中心驱动。

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

按修正后的清单处理（2026-11-20 核查并经用户确认）：Entropy 不是独立交易所，而是 Hyperliquid 的 HIP-3 子 DEX（命名空间 io，市场如 io:ANTH、io:SNDK），经 Hyperliquid Adapter 的 dex namespace 接入，不开发独立 entropy Adapter（`exchanges/entropy.rs` 仅记录该决策）；Arcus 为候选——永续合约仍在分批 beta 开放（现货已开，交易需 Ed25519 API key），开发前必须核验 API 开放状态，不得假设随时可用；Bulk 为独立候选 Adapter——Solana 永续 DEX，主网 API 已上线（docs.bulk.trade），但采用 Solana 钱包签名认证而非 API key 模式，认证与签名须单独设计；Lighter、AsterDex 为候选，接入前各自核实。每个对象单独开发、单独测试；接入前核实官方 API，不得复制未经核实的 API 实现，资料不足先报告缺失信息。

## Phase 13：正式打包

任务：Windows Release 构建、exe 启动测试、便携目录测试、数据库恢复测试、多账户压力测试、异常退出测试、凭证安全检查。

验收：解压即可运行；用户数据保存在程序目录；移动文件夹后可以继续使用；不需要单独安装数据库；程序异常退出后可以恢复；不存在真实凭证泄漏。

## 测试总则

- 必须建立模拟交易所 Adapter，覆盖：下单成功、下单失败、网络超时、部分成交、完全成交、订单取消、重复订单、双腿不平衡、回滚失败、程序中断、重启恢复、多账户并发。
- 没有通过模拟异常测试的交易功能不得进入真实交易阶段。
- 真实交易必须由用户明确启用；未启用前只用模拟 Adapter，不连真实交易所；开发过程中严禁用真实账户下单。
