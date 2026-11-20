# DeXo 开发总指南

本文件是 DeXo 项目的长期开发规范。所有开发必须遵循本指南；遇到技术细节不明确时，优先选择简单、稳定、可维护的方案，不擅自改变项目架构。

## 1. 项目目标

DeXo 是便携式桌面端多账号、多交易所策略交易工具：

- Rust + Tauri 2；支持多交易所、多账户、多策略同时运行。
- 统一行情、资金费率、账户与仓位数据；统一风控；统一订单执行。
- 支持双腿套利交易；支持交易记录与持仓恢复。
- 最终打包为 Windows 桌面应用：解压即运行，全部用户数据保存在 exe 同级 `data/` 目录，移动文件夹后可继续使用；不依赖服务器，不要求用户安装数据库或配置运行环境（Windows 仍需满足 Tauri/WebView2 系统要求）。

## 2. 技术栈与原则

- 后端：Rust、Tokio、Tauri 2、Serde、async-trait、SQLite、tracing。
- 前端：React、TypeScript、Vite、Tauri 2 API。
- 交易核心全部使用 Rust；前端只负责 UI 和用户交互，不允许把交易逻辑放到 JavaScript。
- 不引入微服务、不必要的事件总线、复杂工厂模式；不为抽象而抽象；优先 Rust 原生能力和成熟依赖。
- 数据库使用 SQLite，通过 Rust 后端访问，前端不得直接访问数据库。

## 3. GitHub 开发规范

- `main` 是唯一的长期主分支，不创建长期 `dev` 分支；不允许直接向 `main` 提交功能代码。
- 每个功能从最新 `main` 创建独立 `feature/*` 分支，在功能分支开发和测试；GitHub Actions 自动编译、测试并生成 Windows x64 测试包，上传到 Actions Artifacts 供实机测试；测试通过后通过 PR 合并到 `main`。
- 不创建正式 Release，不发布正式版本。
- 分支命名：`feature/project-bootstrap`、`feature/models`、`feature/accounts`、`feature/exchange-hyperliquid`、`feature/data-center`、`feature/trading`、`feature/risk`、`feature/execution`、`feature/strategies` 等。
- 提交信息使用清晰的英文描述，例如 `feat: initialize Tauri application`、`fix: handle partial order fills`、`test: add execution recovery tests`。
- 每个阶段完成后必须检查：`cargo check`、`cargo test`、`cargo fmt --check`、`cargo clippy`；前端阶段还需 `npm run build`。项目尚未配置相应脚本时先正确配置。
- 不一次性写完所有模块；每项功能独立开发、独立测试。

## 4. 总体架构：八个中心与一个执行层

1. **总览中心**（前端 UI）：账户总览、运行状态、策略状态、持仓与资金概览、交易统计、系统状态。
2. **账户中心** `src/accounts/`：加载与管理账号、启停、账户与交易所关联、凭证管理、账户状态。策略不允许直接读取账户文件。
3. **数据中心** `src/data/`：统一提供行情、Order Book、Funding Rate、Balance、Positions 与市场数据缓存。策略只能通过数据中心获取数据。市场数据必须包含时间戳；数据过期时策略不能使用过期数据执行新订单。
4. **策略中心** `src/strategies/`：只负责看数据 → 判断 → 产生订单请求。不允许直接调用交易所 API、直接读取私钥或账户 JSON、绕过风控、自行修改交易中心的持仓记录。策略通过统一接口读数据，向执行层提交标准化订单意图。
5. **风控中心** `src/risk/`：四层风控（全局、账户、策略、订单）与全局 Kill Switch。Kill Switch 触发后禁止新开仓、停止策略开仓，保留撤单、减仓与平仓能力，记录触发原因，用户可手动解除。禁止把停止新开仓与强制平仓设计成同一个操作。
6. **交易中心** `src/trading/`：订单、成交、持仓记录，交易历史，状态持久化，启动恢复与交易所状态核对。SQLite 是本地持久化基础；交易所实际持仓与成交是恢复时必须核对的数据来源，本地数据库不能被简单视为真实仓位。
7. **设置中心** `src/settings/`：系统设置、便携式数据目录、日志设置、用户配置、安全设置。
8. **交易所中心** `src/exchanges/`：每个交易所单独实现 Adapter，首批规划 Hyperliquid、Entropy、Arcus、Bulk、Lighter、AsterDex。统一接口、统一数据结构，交易所特殊结构不得泄漏到策略层。接入前逐一核实官方 API、订单类型、签名方式、限频规则、测试环境与真实交易能力，不得假设 API 已经可用。
9. **执行层** `src/execution/`：加载账户、启动策略、向策略提供数据中心接口、接收订单请求、调用风控、通过后调用交易所 Adapter、管理订单生命周期与双腿交易、处理失败与回滚、将结果写入交易中心、异常恢复与状态核对。执行层不负责策略判断，但必须负责完整的交易生命周期。

目录结构以仓库根的 `src-tauri/`（Rust 后端）+ `src/`（React 前端）为准，完整树见 IMPLEMENTATION_SPEC.md。允许增加必要文件，但不要无理由改变顶层架构。

## 5. 统一交易数据结构

所有交易所必须映射到统一模型，至少包括：Account、Exchange、FundingInfo、OrderBook、Balance、Position、OrderRequest、Order、Fill、Trade、RiskResult。

- 所有价格、数量、手续费和资金金额使用精确数值类型（rust_decimal），禁止使用 f64 作为资金、价格和订单数量的核心计算类型。
- 统一时间戳；统一交易方向、订单类型和订单状态。
- 所有订单必须携带 `account_id`、`strategy_id`、`exchange`、`symbol`、`client_order_id`；`client_order_id` 用于订单去重、超时查询和恢复。

## 6. 交易所 Adapter

统一 Trait（全部异步）：

- 市场数据：`get_funding`、`get_orderbook`
- 账户数据：`get_positions`、`get_balance`
- 订单操作：`place_order`、`get_order`、`cancel_order`
- 成交数据：`get_fills`

交易所模块负责 API 通信、身份认证、签名、数据解析、精度转换、错误映射与限频处理。不支持的能力必须明确返回 Unsupported 或相应错误，不能伪装成成功。下单接口必须支持客户端订单标识并尽可能使用交易所幂等机制；不支持幂等下单的交易所必须通过查询与恢复机制降低重复下单风险。

## 7. 双腿套利执行规范

双腿交易必须考虑：第一腿完全/部分成交、提交失败；第二腿完全/部分成交、提交失败；网络超时但订单实际已提交；交易所异常；程序中途崩溃；回滚订单失败。

订单状态至少包括：CREATED、RISK_CHECKED、SUBMITTING、UNKNOWN、OPEN、PARTIALLY_FILLED、FILLED、CANCEL_PENDING、CANCELLED、FAILED、ROLLBACK_PENDING、ROLLBACK_SUBMITTED、ROLLBACK_COMPLETED、RECONCILING。状态转换必须受控。

- 网络超时不能直接判定订单失败。UNKNOWN 状态下禁止盲目重复提交，必须用 `client_order_id` 查询订单、查询成交记录、核对当前持仓，确认真实状态后再决定下一步。
- 双腿不平衡时按实际成交数量计算敞口，不按原始计划数量回滚；优先用 reduce-only 订单减小实际敞口（reduce-only 的支持方式逐个 Adapter 实现并验证）；回滚失败进入异常状态，记录并通知用户，必要时停止相关策略的新开仓。
- 不承诺双腿原子成交：两笔订单无法保证真正的原子性。

## 8. 交易恢复机制

启动时：读取本地未完成订单 → 查询交易所未完成订单 → 查询成交记录 → 查询实际持仓 → 对比本地数据库 → 恢复订单与策略状态 → 差异生成异常记录。状态未核实前，禁止自动继续执行可能扩大风险的新开仓操作。

数据库写入必须可靠，关键交易状态变更采用 SQLite 事务。不能出现交易所已成交但本地没有任何可恢复记录的设计。

## 9. 账户与凭证安全

- 账户配置可用 JSON 文件（如 `{ "id": "hl1", "exchange": "hyperliquid", "label": "主账户", "enabled": true, "settings": { "testnet": true } }`），凭证与普通账户配置分开管理。
- 不允许硬编码私钥；不允许将明文私钥写入日志；不允许将真实凭证提交到 GitHub。
- 默认优先使用操作系统凭证存储；便携模式可采用用户主密码加密的本地凭证库；不允许未经用户确认自动切换到明文存储。
- 不允许向前端暴露私钥；禁止将签名密钥放入普通 Tauri 前端状态。
- 提供账户删除和凭证清除功能。
- `.gitignore` 必须排除：`data/`、`*.db`、`*.sqlite`、`.env`、`.env.*`、`secrets/`、`credentials/`、`*.key`、`*.pem`。

## 10. 便携式数据目录

数据根目录由 exe 所在目录决定：

```rust
pub fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}
```

所有数据路径基于 `data_dir()`。目录规划：`data/accounts/`、`data/strategies/`、`data/database/dexo.db`、`data/logs/`、`data/cache/`、`data/credentials/`、`data/settings.json`（另有 WebView 数据 `data/webview/`，保证不写 LocalAppData）。

必须实现：启动时创建缺失目录；检查数据目录是否可写；无权限时明确报错；不允许静默切换到 AppData；不允许将用户数据散落到 exe 文件夹之外；日志文件滚动管理；数据库支持安全关闭和重新启动。开发环境和正式打包环境必须分别验证数据目录。

## 11. 多账户并发

- 账户之间凭证严格隔离；订单始终关联正确账户；同一账户的仓位更新不能相互覆盖；同一账户同一交易对需要合理的并发控制；API 限频按交易所和账户管理；一个账户异常不应无故阻断其他账户；账户停用后不能再产生新的交易请求。
- 不为并发引入复杂架构，优先 Tokio、Mutex、Semaphore 等基础工具；锁的范围尽可能小，不能在持有数据库或账户锁时执行长时间网络请求。

## 12. 前端规范

- 简洁、专业的交易终端风格。主要页面：总览、交易中心、策略中心、账户中心、数据中心、风控中心、系统设置。
- 前端通过 Tauri Commands 与后端通信，后端向前端提供必要的状态更新。
- 前端不能直接请求交易所 API、不能接触账户私钥、不能绕过风控执行订单。交易状态以 Rust 后端为准。

## 13. 开发阶段

严格按阶段开发，各阶段任务与验收标准见 IMPLEMENTATION_SPEC.md：

Phase 1 项目初始化 → Phase 2 基础模型 → Phase 3 账户中心 → Phase 4 第一个交易所（Hyperliquid）→ Phase 5 数据中心 → Phase 6 交易中心 → Phase 7 执行层 → Phase 8 风控中心 → Phase 9 策略中心 → Phase 10 双腿套利执行 → Phase 11 前端 → Phase 12 其他交易所 → Phase 13 正式打包。

## 14. 测试要求

交易软件不能只测试正常流程。必须建立模拟交易所 Adapter，测试：下单成功、下单失败、网络超时、部分成交、完全成交、订单取消、重复订单、双腿不平衡、回滚失败、程序中断、重启恢复、多账户并发。没有通过模拟异常测试的交易功能不得进入真实交易阶段。

真实交易必须由用户明确启用，默认采用测试模式。严禁在开发过程中自动使用真实账户下单。

## 15. AI 开发纪律

1. 不要假设仓库中存在尚未检查的代码；开发前先检查仓库结构。
2. 不要擅自改变整体架构；不要重复创建已有模块。
3. 不要引入不必要的依赖；不要留下无用代码和未使用入口。
4. 不要用占位成功结果冒充真实实现；不要伪造交易所 API；不要把 TODO 当作完成。
5. 不要为了通过编译而删除必要功能。
6. 不要提交密钥和真实账户信息；不要绕过风控。
7. 不要未经测试就宣称功能完成。
8. 每个阶段必须更新开发文档；每个功能必须有相应测试。
9. 某个交易所 API 资料不足时，先调查官方文档并报告缺失信息，不要猜测接口。
10. 出现架构级冲突时，先说明问题和建议，不要自行大范围重构。
