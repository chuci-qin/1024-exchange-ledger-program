# 1024 Exchange Ledger Program

> 核心交易账本程序 - 仓位管理、多Relayer多签、清算与ADL

## 概述

1024 Exchange Ledger Program 是 1024 DEX 的核心交易程序，负责：

- ✅ 仓位管理 (Position PDA)
- ✅ 多 Relayer 多签机制 (2-of-3)
- ✅ 开仓/平仓原子操作
- ✅ 清算与保险基金
- ✅ ADL (自动减仓)
- ✅ 资金费率结算
- ✅ 成交记录

## 核心功能

### 多 Relayer 多签

解决 Relayer 信任问题的去中心化方案：

```
┌─────────────────────────────────────────────────────┐
│                  Multi-Relayer Flow                  │
├─────────────────────────────────────────────────────┤
│                                                      │
│   Relayer A ──┐                                      │
│               │  SubmitTradeBatch                    │
│               ├──────────────────► TradeBatch PDA   │
│   Relayer B ──┤  ConfirmTradeBatch      (2-of-3)    │
│               │                                      │
│   Relayer C ──┘  ExecuteTradeBatch                   │
│                          │                           │
│                          ▼                           │
│                   Execute Trades                     │
│                                                      │
└─────────────────────────────────────────────────────┘
```

### 指令列表

| 类型 | 指令 | 功能 |
|------|------|------|
| **初始化** | `Initialize` | 初始化 LedgerConfig |
| | `InitializeRelayers` | 配置多签 Relayers |
| | `InitializeUserStats` | 创建用户统计账户 |
| **多签** | `SubmitTradeBatch` | 提交交易批次 (第一签) |
| | `ConfirmTradeBatch` | 确认交易批次 (后续签) |
| | `ExecuteTradeBatch` | 执行交易批次 |
| **交易** | `OpenPosition` | 开仓 (原子操作) |
| | `ClosePosition` | 平仓 (原子操作) |
| **清算** | `Liquidate` | 清算仓位 |
| | `TriggerADL` | 触发自动减仓 |
| **资金费** | `SettleFunding` | 结算资金费率 |
| **管理** | `AddRelayer` / `RemoveRelayer` | 管理 Relayer |
| | `SetPaused` / `UpdateAdmin` | 暂停/更新管理员 |

## 账户结构

### LedgerConfig (全局配置)

```rust
pub struct LedgerConfig {
    pub admin: Pubkey,
    pub vault_program: Pubkey,
    pub delegation_program: Option<Pubkey>,
    pub global_sequence: u64,
    pub total_positions_opened: u64,
    pub total_positions_closed: u64,
    pub total_volume_e6: u64,
    pub total_fees_collected_e6: u64,
    pub total_liquidations: u64,
    pub total_adl_count: u64,
    pub is_paused: bool,
    // ...
}
```

### Position (仓位 PDA)

Seeds: `["position", user, market_index]`

```rust
pub struct Position {
    pub user: Pubkey,
    pub market_index: u8,
    pub side: Side,              // Long/Short
    pub size_e6: u64,            // 仓位大小
    pub entry_price_e6: u64,     // 入场价格
    pub margin_e6: u64,          // 保证金
    pub leverage: u8,            // 杠杆
    pub liquidation_price_e6: u64,
    pub unrealized_pnl_e6: i64,
    pub last_funding_ts: i64,
    pub cumulative_funding_e6: i64,
    // ...
}
```

### TradeBatch (多签交易批次)

```rust
pub struct TradeBatch {
    pub batch_id: u64,
    pub data_hash: [u8; 32],
    pub signatures: Vec<RelayerSignature>,
    pub executed: bool,
    pub created_at: i64,
    pub expires_at: i64,
    // ...
}
```

## 关键计算

### 清算价格

```rust
// Long: entry_price * (1 - 1/leverage + maintenance_margin_rate)
// Short: entry_price * (1 + 1/leverage - maintenance_margin_rate)
```

### 未实现盈亏

```rust
// Long: (mark_price - entry_price) * size / 1e6
// Short: (entry_price - mark_price) * size / 1e6
```

### 资金费

```rust
// funding_payment = position_value * funding_rate
// Long 支付 Short (funding_rate > 0)
// Short 支付 Long (funding_rate < 0)
```

## 构建

```bash
# 编译检查
cargo check

# 运行测试
cargo test --lib

# 构建 BPF 程序
cargo build-sbf
```

## 测试

当前测试覆盖：
- ✅ Position 盈亏计算
- ✅ Position 清算判断
- ✅ TradeBatch 签名收集
- ✅ RelayerConfig 权限验证
- ✅ e6 精度数学运算
- ✅ 哈希计算
- ✅ 保证金/手续费计算

```bash
cargo test --lib
# 9 tests passed
```

## CPI 调用

Ledger Program 通过 CPI 调用 Vault Program：

```rust
// 开仓时锁定保证金
cpi::lock_margin(vault_program, vault_config, user_account, amount)?;

// 平仓时结算
cpi::close_position_settle(vault_program, ..., margin, pnl, fee)?;

// 清算时
cpi::liquidate_position(vault_program, ..., margin, remainder, penalty, shortfall)?;
```

## 安全特性

### 多签验证

```rust
if !relayer_config.has_enough_signatures(batch.signature_count()) {
    return Err(LedgerError::InsufficientSignatures);
}
```

### 数据哈希验证

```rust
let computed_hash = compute_hash(&trades);
if batch.data_hash != computed_hash {
    return Err(LedgerError::InvalidDataHash);
}
```

### 清算条件验证

```rust
if !position.should_liquidate(mark_price_e6) {
    return Err(LedgerError::PositionNotLiquidatable);
}
```

## License

MIT
