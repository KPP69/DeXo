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
8. **交易所中心** `src-tauri/src/exchanges/`：每个独立交易所一个 Adapter，统一 Trait 与统一数据模型。规划：Hyperliquid（首个接入；Entropy 是其 HIP-3 子 DEX，经 Hyperliquid Adapter 的 namespace 接入，不设独立 Adapter）、Arcus（候选，永续 API 开放状态待验证）、Bulk（候选，独立 Adapter，Solana 钱包认证）、Lighter、AsterDex（候选）。全部须逐一核实官方 API 后才接入。
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

## 安装方法

1. 打开仓库的 GitHub Actions 运行页，下载最新一次成功运行的测试包 `DeXo-windows-x64-test`（需要登录 GitHub；测试包保留 14 天）。
2. 解压得到 `DeXo.exe`，把它放进一个可写、内容为空的文件夹。
3. 双击 `DeXo.exe` 运行。Windows 11 自带 WebView2 Runtime，无需另装数据库或运行库。

当前测试包（Phase 3）：4.01 MB，sha256:7a631ffa8d8d7fcc2f3cdf4c39f36b838002e4e1f1c2546bce415e2894b48d4d（[Actions 运行 #8](https://github.com/KPP69/DeXo/actions/runs/44849666098)）。

## 使用说明

首次运行后，exe 同级会自动生成 `data/` 目录（含 `accounts/`、`strategies/`、`database/`、`logs/`、`cache/`、`credentials/`、`webview/` 与 `settings.json`），全部数据都在其中；移动整个文件夹即可换位置继续使用。

添加账户：在 `data/accounts/` 下新建 `<账户id>.json`，内容为一个 JSON 对象，只有以下 5 个字段：

```json
{"id":"my-hyperliquid-1","exchange":"hyperliquid","label":"主账户","enabled":true,"testnet":false}
```

- `id` 只能用字母、数字、`-`、`_`，且必须与文件名（去掉 `.json`）一致。
- `testnet` 表示该账户是否指向测试环境；Phase 4 只接主网：请写 `false`，适配器不连接测试网。
- 把 `enabled` 改为 `false` 即可停用该账户，改回 `true` 重新启用；删除文件即删除账户。
- **切勿在账户文件里写任何凭证字段**（如私钥、API Secret）：账户文件只存非敏感配置，出现未知字段会导致整次账户加载失败。凭证通过独立的凭证存储管理（见已知限制）。

## 已知限制

- 真实交易未启用；Phase 4 只接主网（不接 testnet），Phase 4 的下单功能将默认关闭，须账户级显式启用后才会发单；交易所尚未接入（Hyperliquid 为 Phase 4）。
- 凭证存储（CredentialStore）目前只有接口与内存后端，操作系统凭证存储与主密码加密库两个真实后端尚未实现；在此之前程序不会把任何真实凭证落盘。
- Tauri 的 `appDirectoriesOverride` 仍是已知缺口：主窗口 WebView 数据已固定到 `data/webview/`，但其余 Tauri 目录 API 尚未全部映射到 `data/`；首次使用 Tauri 目录 API 或插件时须补齐映射并做实机验证。
- Phase 6 的严格 SQLite 迁移规则已写入文档，代码待该阶段落实。
- 本地完整 Tauri 构建依赖 Windows CI；便携性以 GitHub Actions 测试包的实机运行结果为准，CI 成功不等于实机已验证。

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

分支规范：`main` 是唯一的长期主分支，不设长期 `dev` 分支。当前按产品负责人的决定，在现有分支 `feature/project-bootstrap` 与 PR #1 内按代迭代开发，每代推送后等 GitHub Actions 全绿并生成 Windows x64 测试包（Actions Artifacts）；PR 不合并，等产品完成并收到明确指令后再处理。不创建正式 Release，不发布正式版本。

详细规范见 [DEVELOPMENT_GUIDE.md](DEVELOPMENT_GUIDE.md)，实现规格见 [IMPLEMENTATION_SPEC.md](IMPLEMENTATION_SPEC.md)。

## 当前状态

- Phase 1（项目初始化）已完成：工程骨架、便携数据目录、SQLite 初始化、日志、基础启动与总览页状态展示。
- Phase 2（基础模型）已完成：统一数据模型、14 状态订单状态机、ExchangeAdapter / Strategy / DataAccess / RiskCheck 接口（本地单元测试 26 个全过，CI 运行 #6 全绿）。
- Phase 3（账户中心）已完成：账户配置加载与校验、按交易所分组、启停持久化、凭证存储接口与内存后端（本地单元测试 37 个全过，CI 运行 #8 全绿，测试包见「安装方法」）。
- Phase 4（Hyperliquid 接入）进行中：官方 API 已核验，签名方案采用 hypersdk 试评估；只接主网、只用 API wallet（主钱包私钥不进程序）；先只读、后由用户指定小额单显式验证下单闭环。

真实交易下单默认关闭，必须由用户明确启用；Phase 4 只接主网。
