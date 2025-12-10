# 1024 Exchange Ledger Program

> 核心交易账本程序 - 仓位管理、多 Relayer 多签、清算与 ADL

---

## 🔴 核心铁律

> **所有用户行为必须上链！** 详见 [TODO.md](./TODO.md)

---

## 📋 目录

- [概述](#概述)
- [架构设计](#架构设计)
- [账户结构](#账户结构)
- [指令详解](#指令详解)
- [多签机制](#多签机制)
- [清算与 ADL](#清算与-adl)
- [PDA 地址推导](#pda-地址推导)
- [CPI 调用](#cpi-调用)
- [构建与部署](#构建与部署)
- [测试](#测试)
- [错误代码](#错误代码)

---

## 概述

### 程序职责

1024 Exchange Ledger Program 是 1024 DEX 的核心交易引擎，负责：

| 职责 | 说明 |
|------|------|
| **仓位管理** | Position PDA 生命周期管理 |
| **多签交易** | 2-of-3 Relayer 多签机制 |
| **开仓/平仓** | 原子化交易操作 |
| **清算** | 保证金不足时的强制平仓 |
| **ADL (自动减仓)** | 保险基金不足时的风险控制 |
| **资金费率** | 永续合约资金费结算 |

### 部署信息

| 网络 | Program ID |
|------|-----------|
| 1024Chain Testnet | `Hf5vLwWoFK6e22wwYqT33YUCsxoTz3Jv2FEjrSa3GJPw` |
| 1024Chain Mainnet | TBD |

### 系统交互

```
                          ┌──────────────────────┐
                          │ Off-Chain Matching   │
                          │       Engine         │
                          └──────────┬───────────┘
                                     │
                                     ▼
┌───────────────────────────────────────────────────────────────────┐
│                    1024-exchange-ledger-program                    │
│                         (交易账本)                                  │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│   ┌─────────────┐    ┌─────────────┐    ┌─────────────┐          │
│   │  Relayer A  │────│  TradeBatch │────│  Relayer C  │          │
│   └─────────────┘    │   (多签)    │    └─────────────┘          │
│         │            └─────────────┘          │                   │
│         │                   │                 │                   │
│         ▼                   ▼                 ▼                   │
│   ┌─────────────────────────────────────────────────────────┐    │
│   │              Position PDAs (用户仓位)                    │    │
│   │   [market=0, user=A, long]  [market=1, user=B, short]   │    │
│   └─────────────────────────────────────────────────────────┘    │
│                                                                   │
└────────────────────────────┬──────────────────────────────────────┘
                             │
            ┌────────────────┼────────────────┐
            │                │                │
            ▼                ▼                ▼
   ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐
   │ Vault       │  │ Fund        │  │ Delegation      │
   │ Program     │  │ Program     │  │ Program         │
   │ (保证金)     │  │ (保险基金)   │  │ (API授权)       │
   └─────────────┘  └─────────────┘  └─────────────────┘
```

---

## 架构设计

### 多 Relayer 多签流程

解决单一 Relayer 信任问题的去中心化方案：

```
┌─────────────────────────────────────────────────────────────────┐
│                    Multi-Relayer 2-of-3 Flow                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Step 1: Relayer A 提交批次                                     │
│   ┌─────────────┐                                               │
│   │ Relayer A   │───── SubmitTradeBatch ─────►  TradeBatch PDA  │
│   └─────────────┘           (签名 1)            [signatures: 1] │
│                                                                 │
│   Step 2: Relayer B 确认                                        │
│   ┌─────────────┐                                               │
│   │ Relayer B   │───── ConfirmTradeBatch ────►  TradeBatch PDA  │
│   └─────────────┘           (签名 2)            [signatures: 2] │
│                                                                 │
│   Step 3: 任意 Relayer 执行 (已达到 2/3)                          │
│   ┌─────────────┐                                               │
│   │ Relayer C   │───── ExecuteTradeBatch ────►  Execute Trades  │
│   └─────────────┘                              [OpenPosition x3] │
│                                                [ClosePosition x2]│
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 数据哈希验证

确保所有 Relayer 确认的是同一批交易：

```
trades = [
    {user: A, market: 0, side: Long, size: 100, price: 50000},
    {user: B, market: 0, side: Short, size: 100, price: 50000},
]

data_hash = SHA256(DOMAIN_PREFIX || program_id || batch_id || serialize(trades))
```

---

## 账户结构

### 1. LedgerConfig (全局配置)

**PDA Seeds:** `["ledger_config"]`

```rust
pub struct LedgerConfig {
    pub discriminator: u64,
    pub admin: Pubkey,
    pub vault_program: Pubkey,          // Vault Program ID
    pub fund_program: Pubkey,           // Fund Program ID
    pub delegation_program: Option<Pubkey>, // Delegation Program ID
    
    // 统计数据
    pub global_sequence: u64,           // 全局序列号
    pub total_positions_opened: u64,    // 累计开仓数
    pub total_positions_closed: u64,    // 累计平仓数
    pub total_volume_e6: u64,           // 累计交易量 (e6)
    pub total_fees_collected_e6: u64,   // 累计手续费 (e6)
    pub total_liquidations: u64,        // 累计清算数
    pub total_adl_count: u64,           // 累计 ADL 次数
    
    pub is_paused: bool,
    pub bump: u8,
    pub last_update_ts: i64,
    pub reserved: [u8; 64],
}
```

### 2. RelayerConfig (多签配置)

**PDA Seeds:** `["relayer_config"]`

```rust
pub struct RelayerConfig {
    pub discriminator: u64,
    pub relayers: Vec<Pubkey>,          // 授权 Relayer 列表 (最多10个)
    pub required_signatures: u8,        // 所需签名数 (默认 2)
    pub is_active: Vec<bool>,           // 各 Relayer 激活状态
    pub admin: Pubkey,
    pub bump: u8,
}
```

### 3. TradeBatch (交易批次)

**PDA Seeds:** `["batch", batch_id.to_le_bytes()]`

```rust
pub struct TradeBatch {
    pub discriminator: u64,
    pub batch_id: u64,                  // 批次 ID
    pub data_hash: [u8; 32],            // 交易数据哈希
    pub signatures: Vec<RelayerSignature>, // 已收集的签名
    pub executed: bool,                 // 是否已执行
    pub created_at: i64,                // 创建时间
    pub expires_at: i64,                // 过期时间
    pub bump: u8,
}

pub struct RelayerSignature {
    pub relayer: Pubkey,                // 签名者
    pub signature: [u8; 64],            // Ed25519 签名
    pub signed_at: i64,                 // 签名时间
}
```

### 4. Position (用户仓位)

**PDA Seeds:** `["position", user_pubkey, market_index.to_le_bytes()]`

```rust
pub struct Position {
    pub discriminator: u64,
    pub user: Pubkey,                   // 用户钱包
    pub market_index: u8,               // 市场索引 (0=BTC, 1=ETH, ...)
    pub side: Side,                     // Long/Short
    pub size_e6: u64,                   // 仓位大小 (e6)
    pub entry_price_e6: u64,            // 入场均价 (e6)
    pub margin_e6: u64,                 // 保证金 (e6)
    pub leverage: u8,                   // 杠杆倍数 (1-100)
    pub liquidation_price_e6: u64,      // 清算价格 (e6)
    pub unrealized_pnl_e6: i64,         // 未实现盈亏 (e6)
    pub last_funding_ts: i64,           // 上次资金费时间
    pub cumulative_funding_e6: i64,     // 累计资金费 (e6)
    pub bump: u8,
    pub last_update_ts: i64,
    pub reserved: [u8; 32],
}
```

### 5. UserStats (用户统计)

**PDA Seeds:** `["user_stats", user_pubkey]`

```rust
pub struct UserStats {
    pub discriminator: u64,
    pub user: Pubkey,
    pub total_trades: u64,              // 总交易次数
    pub total_volume_e6: u64,           // 总交易量 (e6)
    pub total_fees_paid_e6: u64,        // 总手续费 (e6)
    pub total_realized_pnl_e6: i64,     // 总已实现盈亏 (e6)
    pub total_liquidations: u64,        // 被清算次数
    pub first_trade_ts: i64,            // 首次交易时间
    pub last_trade_ts: i64,             // 最后交易时间
    pub bump: u8,
    pub reserved: [u8; 32],
}
```

---

## 指令详解

### 初始化指令

#### 1. Initialize

初始化 Ledger 配置。

```rust
Initialize {
    delegation_program: Option<Pubkey>,
}
```

| 账户 | 类型 | 说明 |
|------|------|------|
| 0 | `[signer]` | Admin |
| 1 | `[writable]` | LedgerConfig PDA |
| 2 | `[]` | Vault Program |
| 3 | `[]` | Fund Program |
| 4 | `[]` | System Program |

#### 2. InitializeRelayers

配置多签 Relayer 列表。

```rust
InitializeRelayers {
    relayers: Vec<Pubkey>,
    required_signatures: u8,
}
```

#### 3. InitializeUserStats

创建用户统计账户（Relayer 自动创建）。

### 多签指令

#### 4. SubmitTradeBatch

提交交易批次（第一个 Relayer）。

```rust
SubmitTradeBatch {
    batch_id: u64,
    data_hash: [u8; 32],
}
```

| 账户 | 类型 | 说明 |
|------|------|------|
| 0 | `[signer]` | Relayer |
| 1 | `[writable]` | TradeBatch PDA (自动创建) |
| 2 | `[]` | RelayerConfig |
| 3 | `[]` | System Program |

#### 5. ConfirmTradeBatch

确认交易批次（后续 Relayer）。

```rust
ConfirmTradeBatch {
    batch_id: u64,
    data_hash: [u8; 32],
}
```

#### 6. ExecuteTradeBatch

执行交易批次（签名足够后）。

```rust
ExecuteTradeBatch {
    batch_id: u64,
    trades: Vec<TradeData>,
}
```

**TradeData 结构:**

```rust
pub struct TradeData {
    pub user: Pubkey,
    pub market_index: u8,
    pub trade_type: u8,        // 0=Open, 1=Close
    pub side: Side,            // Long/Short
    pub size_e6: u64,
    pub price_e6: u64,
    pub leverage: u8,
}
```

### 交易指令

#### 7. OpenPosition

开仓（原子操作）。

```rust
OpenPosition {
    user: Pubkey,
    market_index: u8,
    side: Side,
    size_e6: u64,
    price_e6: u64,
    leverage: u8,
    batch_id: u64,
}
```

**内部流程:**
1. 创建/更新 Position PDA
2. CPI 调用 Vault.LockMargin
3. 创建 TradeRecord
4. 更新 UserStats

#### 8. ClosePosition

平仓（原子操作）。

```rust
ClosePosition {
    user: Pubkey,
    market_index: u8,
    size_e6: u64,       // 0 = 全部平仓
    price_e6: u64,
    batch_id: u64,
}
```

**内部流程:**
1. 读取 Position
2. 计算 PnL
3. CPI 调用 Vault.ClosePositionSettle
4. 更新/关闭 Position
5. 更新 UserStats

### 清算指令

#### 9. Liquidate

强制清算仓位。

```rust
Liquidate {
    user: Pubkey,
    market_index: u8,
    mark_price_e6: u64,
}
```

**内部流程:**
1. 验证清算条件 (mark_price vs liquidation_price)
2. CPI 调用 Vault.LiquidatePosition
3. CPI 调用 Fund.AddLiquidationIncome
4. 如有穿仓，CPI 调用 Fund.CoverShortfall
5. 关闭 Position
6. 更新 UserStats

#### 10. TriggerADL

触发自动减仓。

```rust
TriggerADL {
    market_index: u8,
    shortfall_e6: u64,
    bankrupt_side: Side,
}
```

**ADL 排序规则:**
- 选择反向（盈利）仓位
- 按 PnL 比例排序（高 PnL 先被减仓）
- 逐个减仓直到覆盖穿仓损失

### 资金费率指令

#### 11. SettleFunding

结算资金费率。

```rust
SettleFunding {
    user: Pubkey,
    market_index: u8,
    funding_rate_e6: i64,    // 正=Long支付Short, 负=Short支付Long
    index_price_e6: u64,
}
```

**计算公式:**
```
funding_payment = position_value * funding_rate
position_value = size * index_price
```

### 管理指令

| 指令 | 说明 |
|------|------|
| `AddRelayer` | 添加 Relayer |
| `RemoveRelayer` | 移除 Relayer |
| `UpdateRequiredSignatures` | 更新所需签名数 |
| `SetPaused` | 暂停/恢复 |
| `UpdateAdmin` | 更新管理员 |
| `UpdateVaultProgram` | 更新 Vault Program ID |
| `UpdateFundProgram` | 更新 Fund Program ID |

---

## 多签机制

### 签名验证流程

```rust
// 1. 验证 Relayer 在白名单中
fn verify_relayer(config: &RelayerConfig, relayer: &Pubkey) -> bool {
    config.relayers.iter()
        .zip(config.is_active.iter())
        .any(|(r, active)| r == relayer && *active)
}

// 2. 验证数据哈希一致
fn verify_batch_hash(
    program_id: &Pubkey,
    batch_id: u64,
    trades: &[TradeData],
    expected_hash: &[u8; 32],
) -> bool {
    let computed = compute_batch_hash(program_id, batch_id, trades);
    computed == *expected_hash
}

// 3. 验证签名数量
fn has_enough_signatures(
    batch: &TradeBatch,
    required: u8,
) -> bool {
    batch.signatures.len() >= required as usize
}
```

### 哈希计算

```rust
const DOMAIN_PREFIX: &[u8] = b"1024_LEDGER_BATCH_V1";

pub fn compute_batch_hash(
    program_id: &Pubkey,
    batch_id: u64,
    trades: &[TradeData],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_PREFIX);
    hasher.update(program_id.as_ref());
    hasher.update(&batch_id.to_le_bytes());
    hasher.update(&trades.try_to_vec().unwrap());
    hasher.finalize().into()
}
```

---

## 清算与 ADL

### 清算价格计算

```rust
// Long 仓位清算价格
liquidation_price = entry_price * (1 - 1/leverage + maintenance_margin_rate)

// Short 仓位清算价格
liquidation_price = entry_price * (1 + 1/leverage - maintenance_margin_rate)

// 示例: BTC $50,000, 10x 杠杆, 维持保证金率 0.5%
// Long: $50,000 * (1 - 0.1 + 0.005) = $45,250
// Short: $50,000 * (1 + 0.1 - 0.005) = $54,750
```

### 清算流程

```
┌─────────────────────────────────────────────────────────────────┐
│                         清算流程                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   1. 标记价格 < 清算价格 (Long) 或 > 清算价格 (Short)             │
│      ↓                                                          │
│   2. 计算仓位价值和亏损                                          │
│      ↓                                                          │
│   3. 分配资金:                                                   │
│      ├── 清算罚金 (2%) → Insurance Fund                         │
│      ├── 剩余保证金 → 用户                                       │
│      └── 穿仓损失 → Insurance Fund 覆盖                          │
│      ↓                                                          │
│   4. 如果 Insurance Fund 不足 → 触发 ADL                         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### ADL 触发条件

```rust
// 三个条件任一满足即触发 ADL:
pub fn should_trigger_adl(&self, balance: i64, shortfall: i64) -> ADLTriggerReason {
    // 1. 穿仓 - 保险基金无法覆盖
    if shortfall > 0 && balance < shortfall {
        return ADLTriggerReason::Bankruptcy;
    }
    
    // 2. 余额不足 - 低于阈值
    if balance < self.adl_trigger_threshold_e6 {
        return ADLTriggerReason::InsufficientBalance;
    }
    
    // 3. 快速下降 - 1小时内下降超过30%
    if self.balance_1h_ago_e6 > 0 {
        let threshold = self.balance_1h_ago_e6 * 70 / 100;
        if balance < threshold {
            return ADLTriggerReason::RapidDecline;
        }
    }
    
    ADLTriggerReason::None
}
```

---

## PDA 地址推导

### TypeScript 示例

```typescript
const LEDGER_PROGRAM_ID = new PublicKey('Hf5vLwWoFK6e22wwYqT33YUCsxoTz3Jv2FEjrSa3GJPw');

// LedgerConfig PDA
const [ledgerConfigPDA] = await PublicKey.findProgramAddress(
    [Buffer.from("ledger_config")],
    LEDGER_PROGRAM_ID
);

// RelayerConfig PDA
const [relayerConfigPDA] = await PublicKey.findProgramAddress(
    [Buffer.from("relayer_config")],
    LEDGER_PROGRAM_ID
);

// TradeBatch PDA
const batchId = 12345n;
const [tradeBatchPDA] = await PublicKey.findProgramAddress(
    [Buffer.from("batch"), Buffer.from(batchId.toString(16).padStart(16, '0'), 'hex')],
    LEDGER_PROGRAM_ID
);

// Position PDA
const marketIndex = 0; // BTC
const [positionPDA] = await PublicKey.findProgramAddress(
    [
        Buffer.from("position"),
        userWallet.toBuffer(),
        Buffer.from([marketIndex]),
    ],
    LEDGER_PROGRAM_ID
);

// UserStats PDA
const [userStatsPDA] = await PublicKey.findProgramAddress(
    [Buffer.from("user_stats"), userWallet.toBuffer()],
    LEDGER_PROGRAM_ID
);
```

---

## CPI 调用

### 调用 Vault Program

```rust
// 开仓时锁定保证金
cpi::lock_margin(
    vault_program,
    vault_config,
    user_account,
    margin_amount,
)?;

// 平仓时结算
cpi::close_position_settle(
    vault_program,
    vault_config,
    user_account,
    margin_to_release,
    realized_pnl,
    fee,
)?;

// 清算时
cpi::liquidate_position(
    vault_program,
    vault_config,
    user_account,
    margin,
    user_remainder,
    liquidation_penalty,
)?;
```

### 调用 Fund Program

```rust
// 清算收入记录
cpi::add_liquidation_income(
    fund_program,
    insurance_fund,
    insurance_config,
    penalty_amount,
)?;

// 覆盖穿仓
cpi::cover_shortfall(
    fund_program,
    insurance_fund,
    insurance_config,
    shortfall_amount,
)?;
```

---

## 构建与部署

### 构建

```bash
cd 1024-exchange-ledger-program

# 编译检查
cargo check

# 运行测试
cargo test --lib

# 构建 BPF 程序
cargo build-sbf
```

### 部署

```bash
# 部署到 1024Chain Testnet
solana program deploy target/deploy/ledger_program.so \
    --url https://testnet-rpc.1024chain.com/rpc/ \
    --program-id Hf5vLwWoFK6e22wwYqT33YUCsxoTz3Jv2FEjrSa3GJPw \
    --use-rpc
```

---

## 测试

### 单元测试覆盖

| 测试项 | 文件 | 状态 |
|--------|------|------|
| Position 盈亏计算 | `state.rs` | ✅ |
| Position 清算判断 | `state.rs` | ✅ |
| TradeBatch 签名收集 | `state.rs` | ✅ |
| RelayerConfig 权限验证 | `state.rs` | ✅ |
| e6 精度数学运算 | `utils.rs` | ✅ |
| 哈希计算 | `utils.rs` | ✅ |
| 保证金/手续费计算 | `utils.rs` | ✅ |

### 运行测试

```bash
cargo test --lib
# 9 tests passed
```

---

## 错误代码

| 错误 | Code | 说明 |
|------|------|------|
| `InsufficientMargin` | 0 | 保证金不足 |
| `PositionNotFound` | 1 | 仓位不存在 |
| `PositionNotLiquidatable` | 2 | 仓位未达到清算条件 |
| `InvalidRelayer` | 3 | 无效的 Relayer |
| `InsufficientSignatures` | 4 | 签名数量不足 |
| `InvalidDataHash` | 5 | 数据哈希不匹配 |
| `BatchAlreadyExecuted` | 6 | 批次已执行 |
| `BatchExpired` | 7 | 批次已过期 |
| `LedgerPaused` | 8 | Ledger 已暂停 |
| `InvalidMarket` | 9 | 无效的市场索引 |
| `InvalidLeverage` | 10 | 无效的杠杆倍数 |
| `InvalidPrice` | 11 | 无效的价格 |
| `Overflow` | 12 | 数值溢出 |

---

## 文件结构

```
1024-exchange-ledger-program/
├── Cargo.toml
├── README.md
├── rust-toolchain.toml
└── src/
    ├── lib.rs          # 程序入口点
    ├── state.rs        # 账户结构定义
    ├── instruction.rs  # 指令枚举定义
    ├── processor.rs    # 指令处理逻辑
    ├── error.rs        # 错误类型
    ├── utils.rs        # 工具函数 (哈希/数学)
    └── cpi.rs          # CPI Helper 函数
```

---

## License

MIT

---

*Last Updated: 2025-12-10*
