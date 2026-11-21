# AGENTS.md

# DeXo — AI Agent Development Rules

本文件定义 DeXo 项目中 AI 编程代理必须遵循的项目规范、架构约束、开发流程、代码质量要求和交付标准。

适用对象包括但不限于：

- OpenAI Codex
- Claude Code
- Cursor
- GitHub Copilot
- Muse
- 其他能够读取项目文件、修改代码、运行命令或操作 Git 的 AI Agent

本文件是项目级开发约束。所有 AI Agent 在开始任何开发任务之前，必须先阅读本文件及相关项目文档。

---

# 1. Project Overview

## 1.1 Project Name

DeXo

## 1.2 Project Positioning

DeXo 是一款便携式桌面端多账号、多交易所策略交易工具。

项目目标是提供统一的交易管理环境，使用户能够通过一个桌面应用管理多个交易账户、交易所连接、交易策略和交易执行流程。

核心设计方向：

- Portable Desktop Application
- Multi-Account Management
- Multi-Exchange Integration
- Unified Exchange Adapter Architecture
- Strategy Execution
- Risk Management
- Local-First Data Storage
- Modular Architecture
- Extensible Trading Infrastructure

## 1.3 Core Principles

所有开发工作必须遵循以下原则：

1. Portable First：便携式运行是基础要求，不是可选功能。
2. Local First：用户数据优先保存在应用自身的数据目录中。
3. Safety First：交易执行、资金管理和账户操作必须优先考虑安全性。
4. Explicit over Implicit：关键行为必须明确，不允许静默回退或隐藏错误。
5. Modular Architecture：功能必须保持模块化，避免不同业务领域之间产生不必要的耦合。
6. Testable Design：重要业务逻辑必须能够独立测试。
7. Incremental Development：按阶段实现，不擅自扩大开发范围。
8. No False Completion：没有实际验证的功能不得标记为完成。

---

# 2. Technology Stack

项目主要技术栈如下。

## 2.1 Desktop Application

- Rust
- Tauri 2

## 2.2 Frontend

- React
- TypeScript
- Vite

## 2.3 Database

- SQLite
- rusqlite

当前 rusqlite 版本暂时保持 0.32.1。

除非有明确的兼容性、安全性或功能需求，否则不得擅自升级。

## 2.4 Build and CI

- GitHub Actions
- Windows Runner
- Rust Toolchain
- Node.js
- npm

具体依赖版本以项目实际配置文件为准。

不得为了追求版本统一而随意升级依赖。

---

# 3. Mandatory Documentation Reading

开始任务之前，必须先检查并阅读以下文档：

1. `AGENTS.md`
2. `README.md`
3. `DEVELOPMENT_GUIDE.md`
4. `IMPLEMENTATION_SPEC.md`

如果任务涉及特定模块，还必须阅读该模块对应的设计文档、接口定义和已有实现。

文档之间发生冲突时：

- 本文件负责 AI Agent 的开发行为和项目级约束。
- `IMPLEMENTATION_SPEC.md` 负责产品功能、技术架构和实现要求。
- `DEVELOPMENT_GUIDE.md` 负责开发流程、环境和操作说明。
- `README.md` 负责项目介绍、安装和基本使用说明。

如果发现文档互相矛盾：

1. 不得自行选择一个版本并忽略其他文档。
2. 明确指出冲突位置。
3. 判断是否影响当前任务。
4. 必要时先向用户确认。
5. 经确认后同步修订相关文档。

---

# 4. Git Branching Strategy

## 4.1 Permanent Branches

项目只使用一个长期维护分支：

`main`

不创建长期存在的 `dev` 分支。

不引入其他未经用户批准的长期分支。

## 4.2 Feature Branches

每个独立功能或明确的开发任务，从最新的 `main` 创建一个短期功能分支。

命名规则：

- `feature/<feature-name>`
- `fix/<bug-name>`
- `refactor/<module-name>`
- `docs/<document-name>`
- `test/<test-name>`

例如：

- `feature/exchange-adapter`
- `feature/account-management`
- `feature/strategy-engine`
- `fix/portable-data-path`
- `docs/api-documentation`

## 4.3 Existing Branches and Pull Requests

如果目标功能分支或对应 PR 已经存在：

1. 首先检查远端分支。
2. 检查对应 PR。
3. 确认当前分支和 PR 的关联关系。
4. 在现有分支继续开发。

不得为了方便而重复创建同一功能的分支或 PR。

当前项目已有的 `feature/project-bootstrap` 和 PR #1，应在完成当前 Phase 1 工作时继续沿用。

除非用户明确要求，否则不得重建、替换或关闭现有 PR。

## 4.4 Pull Request Rules

每个独立功能通过独立 PR 交付。

PR 必须包含：

- 功能目标
- 修改内容
- 涉及模块
- 测试结果
- 构建结果
- 已知问题
- 后续工作

不得将多个无关功能强行放入同一个 PR。

## 4.5 Merge Policy

AI Agent 不得自行决定合并 PR。

必须遵循：

1. 完成开发。
2. 执行自动化测试。
3. 通过 GitHub Actions。
4. 生成 Windows 测试包。
5. 用户下载测试包。
6. 用户进行实机验证。
7. 用户明确批准后，才能合并到 `main`。

**用户实机验收之前，禁止合并 PR。**

不得擅自执行：

- Merge Pull Request
- Squash and Merge
- Rebase and Merge
- 直接向 `main` 推送功能代码

---

# 5. Development Workflow

## 5.1 Before Development

开始任务前必须：

1. 阅读相关文档。
2. 检查当前 Git 分支。
3. 检查工作区是否存在未提交修改。
4. 检查远端分支状态。
5. 检查是否已有相关 PR。
6. 检查相关模块的现有实现。
7. 明确本次任务的范围和验收标准。

不得在未检查工作区的情况下覆盖文件或重置代码。

不得擅自删除用户已有的修改。

## 5.2 Scope Control

只实现用户明确要求的功能，以及实现该功能不可缺少的依赖修改。

禁止：

- 擅自增加新功能。
- 擅自重构无关模块。
- 擅自更换技术栈。
- 擅自升级依赖。
- 擅自改变数据库结构。
- 擅自修改已经确定的产品行为。
- 为了代码整洁而进行大范围无关修改。

如果发现其他问题：

1. 记录问题。
2. 判断是否影响当前功能。
3. 不影响当前任务时，列入待处理事项。
4. 影响当前任务时，说明原因并处理，必要时先取得用户确认。

## 5.3 Implementation

实现过程中必须：

- 优先复用已有模块。
- 遵循项目现有代码风格。
- 保持模块职责清晰。
- 避免不必要的全局状态。
- 避免重复实现相同功能。
- 对关键业务逻辑添加测试。
- 对错误进行明确处理。
- 不隐藏关键异常。

不得使用临时占位实现冒充真实功能。

如果确实需要占位实现，必须明确标记：

- TODO
- NOT IMPLEMENTED
- PLACEHOLDER

并说明限制。

## 5.4 After Development

完成修改后必须：

1. 检查修改文件。
2. 检查代码差异。
3. 执行相关测试。
4. 执行格式检查。
5. 执行静态检查。
6. 执行适用的构建检查。
7. 检查是否引入新的依赖。
8. 检查是否修改数据库结构。
9. 更新相关文档。
10. 汇报实际验证结果。

不得只根据代码看起来正确就宣布任务完成。

---

# 6. Portable Application Requirements

便携式运行是 DeXo 的核心产品要求。

不能将普通桌面应用的默认系统目录行为直接当作 DeXo 的数据存储方案。

## 6.1 Portable Directory Structure

应用的数据应集中在约定的便携式 `data/` 目录中。

目标结构示例：

```text
DeXo/
├── DeXo.exe
├── data/
│   ├── database/
│   ├── logs/
│   ├── config/
│   ├── cache/
│   └── webview/
└── resources/
```

实际目录结构以项目文档和实现为准。

不得随意改变已经确定的数据目录结构。

## 6.2 Data Directory Rules

必须遵循：

1. 用户数据不得静默写入 AppData。
2. 不得静默写入当前工作目录。
3. 不得静默写入程序目录以外的未知位置。
4. 不得在路径解析失败时自动切换到其他目录。
5. 不得因为目录创建失败而使用临时目录继续运行。
6. 必须明确处理文件系统权限错误。
7. 必须明确处理路径解析错误。

## 6.3 Path Resolution

`data_dir()` 和 `data_dir_for_exe()` 必须使用明确的错误返回类型。

推荐使用：

```rust
Result<PathBuf, AppError>
```

或项目统一定义的错误类型。

禁止：

```rust
unwrap_or_else(|| PathBuf::from("."))
```

禁止：

```rust
unwrap_or_default()
```

禁止在关键路径解析逻辑中使用静默 fallback。

如果 `current_exe()` 失败：

- 返回明确错误。
- 记录错误信息。
- 阻止应用进入依赖该路径的正常运行阶段。

如果目录创建失败：

- 返回明确错误。
- 不允许静默忽略。
- 不允许自动切换到系统目录。

## 6.4 Path Testing

必须覆盖：

- 正常获取可执行文件路径。
- 正常生成 data 目录。
- 目标目录不存在。
- 目标目录创建失败。
- 可执行文件路径解析失败。
- 路径不符合预期。
- 关键目录不可写。

路径测试必须验证实际返回结果和错误行为。

## 6.5 Tauri Directory Mapping

Tauri 自身可能使用多个系统目录。

未来引入 Tauri Directory API 或相关插件时，必须将以下目录统一映射到便携式 `data/`：

- app_config_dir
- app_data_dir
- app_local_data_dir
- app_cache_dir
- app_log_dir

在完成映射和实机验证之前，不得宣称所有 Tauri 数据目录都已实现便携式重定向。

当前 `webview` 数据目录配置不代表上述目录全部完成映射。

---

# 7. Rust Development Rules

## 7.1 Error Handling

Rust 代码必须明确处理错误。

关键业务逻辑禁止随意使用：

- `unwrap()`
- `expect()`
- `unwrap_or_default()`
- 静默忽略错误的 `let _ = ...`

在测试代码中可以根据测试目的合理使用 `unwrap()`。

在生产代码中，如果确实使用 `unwrap()` 或 `expect()`，必须有明确的不可达性依据，并说明原因。

优先使用：

- `Result`
- `Option`
- `?`
- 明确的错误类型
- 结构化错误上下文

## 7.2 Logging

日志必须使用项目统一的日志机制。

禁止：

- 将密码写入日志。
- 将 API Secret 写入日志。
- 将私钥写入日志。
- 将完整签名认证材料写入日志。
- 在生产代码中大量使用临时调试输出。

敏感信息必须脱敏。

## 7.3 Async and Concurrency

异步任务必须考虑：

- 生命周期
- 锁竞争
- 任务取消
- 错误传播
- 资源释放
- 网络超时
- 并发访问

不得通过无限等待或无超时请求掩盖潜在问题。

---

# 8. Frontend Development Rules

## 8.1 TypeScript

优先使用明确类型。

禁止为了快速通过编译而大量使用：

```typescript
any
```

不得使用类型断言掩盖实际类型错误。

## 8.2 React

遵循 React 组件化原则。

要求：

- 组件职责清晰。
- 状态管理范围合理。
- 避免不必要的重复渲染。
- 避免在组件中直接混入大量业务逻辑。
- 网络请求和业务服务应保持合理分层。
- 复杂业务逻辑应独立于 UI 组件。

## 8.3 UI Consistency

所有新 UI 必须遵循项目已有的设计规范。

不得未经批准：

- 更换整体 UI 框架。
- 引入新的 UI 组件库。
- 大范围改变视觉风格。
- 破坏现有布局和交互。
- 使用未经确认的设计语言替代项目规范。

## 8.4 Frontend Error Handling

必须处理：

- 加载状态
- 空数据状态
- 网络错误
- 交易所连接错误
- 用户输入错误
- 后端调用失败
- 数据格式异常

不能只考虑正常成功路径。

---

# 9. Exchange Adapter Architecture

交易所接入是 DeXo 的核心功能之一。

所有交易所接入必须遵循统一 Adapter 架构。

## 9.1 Adapter Isolation

每个独立交易所 Adapter 应负责：

- 身份认证
- API 请求
- 签名
- 市场信息
- 账户信息
- 余额查询
- 仓位查询
- 订单管理
- 成交记录
- 错误转换
- 限频管理
- 网络异常处理

交易所之间不得通过复制粘贴大量业务逻辑形成相互耦合。

## 9.2 Unified Interface

不同交易所必须通过统一接口暴露共同业务能力。

交易所独有的能力可以通过扩展接口提供。

不得为了强行统一而丢失交易所自身的重要业务语义。

## 9.3 Exchange vs Sub-DEX

必须区分：

- 独立交易所。
- 同一交易所下的子 DEX。
- 独立市场命名空间。
- 独立认证体系。

不能把所有交易平台名称都直接视为独立交易所 Adapter。

例如：

- Entropy 应作为 Hyperliquid HIP-3 体系下的子 DEX / 市场命名空间处理，而不是默认创建独立 Adapter。
- Bulk 应单独评估其 Solana 钱包认证和签名模型。
- Arcus 应在实际开发前核验 API 开放状态、认证要求和可用市场。

## 9.4 Exchange Verification

交易所接入状态必须区分：

- VERIFIED：已经验证可以接入。
- CANDIDATE：候选，尚待验证。
- SUBMARKET：通过现有 Adapter 的子市场或命名空间接入。
- UNSUPPORTED：当前不支持。

不得把未经验证的交易所标记为已完成。

交易所 API 可能变化，正式开发前必须核验最新官方 API 文档。

## 9.5 Authentication Security

必须保护：

- API Key
- API Secret
- Private Key
- Wallet Credentials
- Access Token
- Refresh Token
- Signature Material

不得将敏感凭证直接写入普通日志、Git 仓库或无保护的配置文件。

不得在没有用户授权的情况下执行真实交易。

---

# 10. Trading and Risk Management

交易功能涉及真实资金，必须严格遵守安全要求。

## 10.1 Order Execution

订单执行必须明确处理：

- 订单方向
- 订单类型
- 数量
- 价格
- 精度
- 最小下单量
- 最小名义价值
- 交易所限频
- 订单状态
- 部分成交
- 完全成交
- 撤单
- 拒单
- 网络超时
- 重复请求

不得假设请求成功就意味着订单已经成交。

## 10.2 Idempotency

对于可能重复执行的操作，必须考虑幂等性。

特别是：

- 下单
- 撤单
- 资金转移
- 策略执行
- 自动重试

不能因为网络超时就盲目重复提交可能产生资金风险的请求。

## 10.3 Risk Controls

涉及风险控制的功能必须考虑：

- 最大订单金额
- 最大仓位
- 杠杆限制
- 最大亏损
- 账户余额
- 可用保证金
- 交易所规则
- 策略异常退出

如果风险控制尚未实现，必须明确标记，不得将其描述为已具备保护能力。

## 10.4 Live Trading

任何真实交易执行功能：

1. 必须有明确的用户操作入口。
2. 必须清晰展示交易参数。
3. 必须处理交易所响应。
4. 必须记录可审计的执行结果。
5. 不得在测试流程中误触真实交易。
6. 不得擅自使用用户的真实账户进行验证。

---

# 11. SQLite and Database Rules

## 11.1 Database Location

SQLite 数据库必须位于便携式 `data/` 目录中。

不得静默使用系统默认数据库路径。

## 11.2 Current Dependency

当前项目使用：

`rusqlite = 0.32.1`

暂时保持该版本。

除非存在明确理由并获得相应批准，否则不升级。

## 11.3 Database Initialization

数据库初始化必须区分：

- 全新数据库。
- 已有合法数据库。
- 已有数据库但缺少版本信息。
- 数据库版本不兼容。
- 数据库文件损坏。

不得将所有情况都当作新数据库处理。

## 11.4 Migration Rules

未来 Phase 6 数据库迁移必须遵守：

1. 仅对真正的全新数据库执行初始 Schema 创建。
2. 通过明确的元数据表记录 Schema 版本。
3. 如果数据库已经存在业务表，但缺少 `dexo_meta` 或有效版本记录，必须拒绝自动初始化。
4. 不得通过 `INSERT OR IGNORE` 掩盖版本异常。
5. 不得静默重建用户数据库。
6. 不得在未经授权的情况下删除或覆盖用户数据。
7. 数据迁移必须具备明确的版本顺序。
8. 迁移必须尽可能使用事务。
9. 迁移失败必须保留可诊断信息。
10. 迁移前必须考虑数据备份和恢复策略。

## 11.5 Data Integrity

涉及用户交易数据的操作必须考虑：

- 事务一致性
- 唯一性约束
- 外键关系
- 数据校验
- 并发访问
- 崩溃恢复

不能通过忽略数据库错误来维持表面上的正常运行。

---

# 12. Configuration and Secrets

## 12.1 Configuration

配置文件必须与敏感凭证分开考虑。

配置项必须有明确默认值和校验逻辑。

无效配置必须产生明确错误。

## 12.2 Secrets

任何真实凭证不得提交到 Git。

包括：

- API Key
- API Secret
- Private Key
- Wallet Seed
- Access Token
- Refresh Token
- Password

不得在示例配置中使用真实用户凭证。

示例配置必须使用虚构占位值。

## 12.3 Environment Variables

如果使用环境变量：

- 必须说明变量用途。
- 必须验证变量格式。
- 必须避免将敏感值写入构建日志。
- 不得将本地环境依赖误当作正式运行依赖。

---

# 13. Testing Requirements

测试必须覆盖实际业务行为，而不只是代码能否编译。

## 13.1 Rust Tests

根据模块执行：

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

如果某项检查不适用于当前工作区，必须说明原因。

## 13.2 Frontend Tests and Build

根据项目实际配置执行：

```bash
npm ci
npm run build
```

如果项目已经配置其他测试命令，也必须执行与本次修改相关的测试。

## 13.3 Tauri Tests

必须区分：

- Rust 单元测试。
- 前端构建。
- Tauri 应用构建。
- Windows 安装或便携包构建。
- Windows 实机启动验证。

以上不是同一项验证。

只通过 `cargo test`，不能宣称完整 Tauri 应用已验证。

只通过前端构建，也不能宣称桌面应用已经可以正常运行。

## 13.4 Integration Tests

涉及多个模块的功能必须考虑集成测试。

例如：

- 前端与 Rust 命令调用。
- Rust 与 SQLite。
- Adapter 与交易所 API。
- 账户管理与凭证存储。
- 策略引擎与订单执行。

## 13.5 Test Integrity

禁止：

- 删除失败测试来获得绿色结果。
- 跳过测试却不说明。
- 修改测试断言以掩盖实际错误。
- 将未运行的测试标记为通过。
- 将模拟测试描述为真实交易所验证。

测试结果必须如实汇报。

---

# 14. GitHub Actions and Test Builds

DeXo 使用 GitHub Actions 自动执行构建和测试。

## 14.1 CI Requirements

Windows CI 应根据项目实际配置执行：

1. Checkout。
2. 安装 Node.js。
3. 安装 Rust。
4. 安装所需 Windows 构建依赖。
5. 安装 npm 依赖。
6. 前端构建。
7. Rust 格式检查。
8. Rust 静态检查。
9. Rust 测试。
10. Tauri Release 构建。
11. 上传测试 Artifact。

CI 的具体步骤必须与仓库实际情况保持一致。

不得添加未经验证的命令并声称 CI 已经完成。

## 14.2 Artifact Requirements

Windows 测试构建必须通过 GitHub Actions Artifact 提供。

Artifact 应包含实机测试所需的文件。

不得仅因为 `DeXo.exe` 存在就认定测试包完整。

必须检查：

- 可执行文件。
- 必要运行时依赖。
- Tauri 资源文件。
- 配置文件。
- 便携式目录初始化行为。
- 启动所需的其他文件。

Artifact 建议保留 14 天，除非用户另有要求。

## 14.3 No Formal Releases

当前项目采用测试包交付流程。

不得擅自：

- 创建 GitHub Release。
- 发布正式版本。
- 创建正式版本标签。
- 将测试构建描述为正式发行版。

## 14.4 User Acceptance

用户下载 Artifact 后进行实机测试。

测试至少应关注：

- 能否启动。
- 能否正常显示主窗口。
- 是否正确初始化 `data/`。
- 数据是否写入预期位置。
- 重启后数据是否保留。
- 日志是否正常。
- 退出是否正常。
- 是否存在缺失文件或运行时依赖。

用户验收前不得合并 PR。

---

# 15. Documentation Maintenance

代码变更涉及架构、配置、安装、运行或用户行为时，必须同步更新相关文档。

需要关注：

- README.md
- DEVELOPMENT_GUIDE.md
- IMPLEMENTATION_SPEC.md
- AGENTS.md
- 模块相关文档

文档必须反映真实实现。

不得出现：

- 文档声称已实现，代码实际上没有。
- 文档描述的命令已经失效。
- 文档使用过期依赖版本。
- 文档把候选功能描述为已完成。
- 文档省略重要限制条件。

## 15.1 AGENTS.md Maintenance

当项目长期开发规则、架构约束或工具使用规范发生变化时，必须评估是否需要更新本文件。

不要为了单个临时任务频繁修改项目级规范。

---

# 16. Dependency Management

新增依赖之前必须：

1. 检查现有依赖是否已提供相同能力。
2. 判断是否确实需要新增依赖。
3. 检查维护状态。
4. 检查许可证。
5. 检查与当前技术栈的兼容性。
6. 评估体积和运行时影响。
7. 评估安全性。

不得为了方便而重复引入功能相似的库。

不得未经批准大规模升级依赖。

升级依赖必须说明：

- 原版本
- 新版本
- 升级原因
- 兼容性影响
- 测试结果
- 潜在风险

---

# 17. Security Requirements

DeXo 处理交易账户和潜在资金操作，安全要求必须贯穿整个项目。

## 17.1 Sensitive Data

禁止将以下信息提交到仓库：

- API Secret
- 私钥
- 助记词
- 钱包种子
- 用户密码
- 访问令牌
- 真实账户凭证

## 17.2 Network Security

网络请求必须考虑：

- TLS
- 请求超时
- 错误处理
- 证书验证
- 限频
- 重试策略
- 响应数据验证

不得为了方便而关闭 TLS 证书验证。

## 17.3 Input Validation

所有来自以下来源的数据都必须视为外部输入：

- 用户输入
- 交易所 API
- 网络响应
- 配置文件
- 数据库
- 第三方服务

必须进行必要的格式、范围和业务规则校验。

## 17.4 Security Changes

如果发现可能造成以下问题的漏洞：

- 凭证泄漏
- 未授权交易
- 资金损失
- 数据损坏
- 任意文件访问
- 权限绕过

必须优先报告，并在得到适当授权后处理。

---

# 18. Prohibited Actions

没有用户明确授权时，AI Agent 不得执行以下操作：

1. 创建长期 `dev` 分支。
2. 创建未经批准的长期分支。
3. 合并 PR。
4. 直接推送功能代码到 `main`。
5. 创建正式 Release。
6. 删除远端分支。
7. 删除已有 PR。
8. 强制覆盖用户已有修改。
9. 执行破坏性数据库操作。
10. 清空用户数据。
11. 使用真实账户进行未经批准的交易。
12. 修改真实交易所 API 凭证。
13. 大范围重构无关模块。
14. 擅自改变产品需求。
15. 擅自升级关键依赖。
16. 将未验证的功能标记为完成。

如果某项操作确实是完成任务所必需的，必须先说明原因并取得用户授权。

---

# 19. AI Agent Reporting Requirements

每项开发任务完成后，必须提交清晰的执行报告。

报告至少包含以下内容：

## 19.1 Task Summary

说明本次完成了什么。

## 19.2 Modified Files

列出所有修改、新增和删除的文件。

## 19.3 Implementation Details

说明关键实现和设计决策。

## 19.4 Testing Results

逐项报告：

- 执行的命令。
- 是否通过。
- 失败原因。
- 未执行的测试及原因。

不得只写“测试通过”。

## 19.5 Build Results

说明：

- 前端构建结果。
- Rust 构建结果。
- Tauri 构建结果。
- Windows 构建结果。

没有实际运行的项目必须标记为未验证。

## 19.6 Git Information

报告：

- 当前分支。
- Commit SHA。
- PR 编号。
- PR 地址。
- 是否已推送。

如果没有执行 Git 提交，必须明确说明。

## 19.7 CI Information

如果适用，报告：

- GitHub Actions 地址。
- Workflow 运行结果。
- Artifact 名称。
- Artifact 下载位置。
- Artifact 是否可以正常下载。

## 19.8 Known Issues

列出：

- 未解决的问题。
- 已知缺陷。
- 尚未验证的行为。
- 后续需要处理的事项。

## 19.9 Completion Status

使用以下状态：

- COMPLETED：实现和要求的验证均已完成。
- PARTIALLY COMPLETED：部分实现，仍有明确未完成事项。
- BLOCKED：存在阻塞，无法继续。
- NOT VERIFIED：代码可能已实现，但缺少必要验证。

不得将 PARTIALLY COMPLETED、BLOCKED 或 NOT VERIFIED 描述为 COMPLETED。

---

# 20. Decision and Approval Rules

AI Agent 可以自主处理：

- 不改变产品行为的局部代码实现。
- 常规错误处理。
- 必要的单元测试。
- 与当前功能直接相关的文档更新。
- 不涉及破坏性操作的格式调整。

以下事项必须先获得用户明确批准：

- 改变产品需求。
- 改变核心架构。
- 改变数据库结构或迁移策略。
- 更换核心技术栈。
- 升级关键依赖。
- 引入重大第三方依赖。
- 创建或修改长期分支策略。
- 合并 PR。
- 发布正式版本。
- 删除或覆盖用户数据。
- 执行真实资金交易。
- 修改真实账户凭证。

如果存在多个合理实现方案：

1. 先分析方案差异。
2. 说明各自影响。
3. 给出推荐方案。
4. 对重大架构决策等待用户批准。

不要把已经明确的项目决定反复作为问题询问用户。

---

# 21. Current Project Decisions

以下是 DeXo 当前已经确定的项目决策。

AI Agent 必须遵守，除非用户明确修改。

## 21.1 Git Strategy

- `main` 是唯一长期分支。
- 不创建长期 `dev` 分支。
- 使用短期 `feature/*` 分支。
- 每个功能通过独立 PR 交付。
- 用户实机验收前不合并 PR。

## 21.2 Current Bootstrap Work

当前 Phase 1 项目骨架通过：

`feature/project-bootstrap`

对应 PR：

`https://github.com/KPP69/DeXo/pull/1`

继续使用现有分支和 PR，不要重新创建。

## 21.3 Portable Data

- `data/` 是便携式用户数据目录。
- `data_dir()` 和 `data_dir_for_exe()` 必须返回明确的 Result。
- 不允许静默回退到 `.` 或当前工作目录。
- 路径解析或目录创建失败必须明确报错。
- 必须增加对应测试。

## 21.4 Tauri Directory Mapping

`appDirectoriesOverride` 当前属于已知缺口。

暂不作为 Phase 1 阻塞项。

首次引入相关 Tauri Directory API 或插件时，必须将应用目录映射到便携式 `data/`，并进行 Windows 实机验证。

## 21.5 Exchange List

- Entropy 通过 Hyperliquid HIP-3 子 DEX / namespace 处理。
- Arcus 保留候选，开发前核验 API 和认证。
- Bulk 保留独立候选 Adapter，单独设计 Solana 钱包认证。
- Lighter、AsterDex、Hyperliquid 的当前接入状态不得在未重新核验时标记为已验证。

## 21.6 SQLite

- 暂时保留 rusqlite 0.32.1。
- 不因其他项目版本不同而无依据升级。
- Phase 6 必须执行严格数据库迁移规则。
- 有业务表但无数据库版本元数据时必须拒绝自动初始化。
- 不允许通过 `INSERT OR IGNORE` 掩盖版本错误。

## 21.7 CI and Delivery

- 使用 GitHub Actions 构建 Windows 测试包。
- 使用 Artifact 交付测试包。
- 不创建正式 Release。
- Artifact 建议保留 14 天。
- 用户实机测试通过前不合并 PR。

---

# 22. Final Instruction

DeXo 是一个涉及交易账户、交易执行、用户数据和便携式运行环境的桌面应用。

AI Agent 的职责不是尽可能多地修改代码，而是在明确的产品目标和架构约束下，准确、可验证、可追溯地完成任务。

必须始终遵守：

- 不擅自改变已经确定的决策。
- 不隐瞒错误。
- 不伪造测试结果。
- 不把未验证功能标记为完成。
- 不进行未经批准的破坏性操作。
- 不绕过用户验收流程。
- 不牺牲便携性和数据安全来换取表面上的构建成功。

**当代码实现、测试结果和文档描述不一致时，以实际可验证的代码行为为依据，并主动修正文档。**

**当任务要求与本文件冲突时，停止相关操作，说明冲突，并等待用户确认。**
