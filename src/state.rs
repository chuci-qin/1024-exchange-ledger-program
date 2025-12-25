//! Ledger Program State Definitions
//!
//! 核心账户结构:
//! - LedgerConfig: 全局配置
//! - RelayerConfig: 多 Relayer 配置
//! - TradeBatch: 交易批次 (多签)
//! - Position: 用户仓位 (PDA)
//! - TradeRecord: 成交记录
//! - UserStats: 用户统计

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;
use crate::utils::{mul_e6, div_e6, checked_sub, checked_add};

// ============================================================================
// Side (仓位方向)
// ============================================================================

/// 仓位方向/订单方向
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// 多头/买入
    Long,
    /// 空头/卖出
    Short,
}

impl Side {
    /// 获取反向方向
    pub fn opposite(&self) -> Self {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
        }
    }
}
use solana_program::program_error::ProgramError;

// ============================================================================
// Constants
// ============================================================================

/// 最大 Relayer 数量
pub const MAX_RELAYERS: usize = 5;

/// 最大签名数量
pub const MAX_SIGNATURES: usize = 5;

/// 最大杠杆倍数 (100x)
pub const MAX_LEVERAGE: u8 = 100;

/// 默认清算阈值 (维持保证金率 2.5%)
pub const DEFAULT_MAINTENANCE_MARGIN_RATE: i64 = 25_000; // 2.5% in e6

/// 清算罚金率 (1%)
pub const LIQUIDATION_PENALTY_RATE: i64 = 10_000; // 1% in e6

/// 交易批次过期时间 (60 秒)
pub const TRADE_BATCH_EXPIRY_SECONDS: i64 = 60;

// ============================================================================
// LedgerConfig (全局配置)
// ============================================================================

/// LedgerConfig (全局配置)
/// 
/// ⚠️ 重要：此结构必须与链上已部署的账户数据格式完全匹配！
/// 链上账户大小: 243 bytes
/// 
/// 修复记录 (2025-12-10):
/// - 移除 delegation_program 字段以匹配链上数据格式
/// - delegation_program 功能暂时不使用，后续如需添加需要数据迁移
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct LedgerConfig {
    /// 账户鉴别器 (8 bytes)
    pub discriminator: [u8; 8],
    /// 管理员 (32 bytes)
    pub admin: Pubkey,
    /// Vault Program ID (用户资金管理) (32 bytes)
    pub vault_program: Pubkey,
    /// Fund Program ID (保险基金/系统资金管理) (32 bytes)
    pub fund_program: Pubkey,
    /// 全局序列号 (用于交易排序) (8 bytes)
    pub global_sequence: u64,
    /// 总开仓数 (8 bytes)
    pub total_positions_opened: u64,
    /// 总平仓数 (8 bytes)
    pub total_positions_closed: u64,
    /// 总成交量 (USDC, e6) (8 bytes)
    pub total_volume_e6: u64,
    /// 总手续费 (e6) (8 bytes)
    pub total_fees_collected_e6: u64,
    /// 总清算数 (8 bytes)
    pub total_liquidations: u64,
    /// 总 ADL 次数 (8 bytes)
    pub total_adl_count: u64,
    /// 是否暂停 (1 byte)
    pub is_paused: bool,
    /// Bump (1 byte)
    pub bump: u8,
    /// 创建时间 (8 bytes)
    pub created_at: i64,
    /// 最后更新时间 (8 bytes)
    pub last_update_ts: i64,
    /// 预留空间 (65 bytes) - 用于未来扩展
    pub reserved: [u8; 65],
}

impl LedgerConfig {
    pub const DISCRIMINATOR: [u8; 8] = *b"ledgcfg_";
    /// 链上账户大小 - 必须与已部署账户匹配！
    pub const SIZE: usize = 8 + // discriminator
        32 + // admin
        32 + // vault_program
        32 + // fund_program
        8 + // global_sequence
        8 + // total_positions_opened
        8 + // total_positions_closed
        8 + // total_volume_e6
        8 + // total_fees_collected_e6
        8 + // total_liquidations
        8 + // total_adl_count
        1 + // is_paused
        1 + // bump
        8 + // created_at
        8 + // last_update_ts
        65; // reserved
    // Total: 243 bytes (与链上账户匹配)

    pub fn next_sequence(&mut self) -> u64 {
        let seq = self.global_sequence;
        self.global_sequence = self.global_sequence.saturating_add(1);
        seq
    }
}

// ============================================================================
// RelayerConfig (多 Relayer 配置)
// ============================================================================

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct RelayerConfig {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 管理员 (可添加/移除 Relayer)
    pub admin: Pubkey,
    /// 授权的 Relayers
    pub authorized_relayers: Vec<Pubkey>,
    /// 需要的签名数 (e.g., 2 of 3)
    pub required_signatures: u8,
    /// Bump
    pub bump: u8,
    /// 最后更新时间
    pub last_update_ts: i64,
}

impl RelayerConfig {
    pub const DISCRIMINATOR: [u8; 8] = *b"rlycfg__";
    pub const SIZE: usize = 8 + // discriminator
        32 + // admin
        4 + (32 * MAX_RELAYERS) + // authorized_relayers (Vec)
        1 + // required_signatures
        1 + // bump
        8 + // last_update_ts
        32; // reserved

    /// 检查是否为授权 Relayer
    pub fn is_authorized(&self, relayer: &Pubkey) -> bool {
        self.authorized_relayers.contains(relayer)
    }

    /// 检查签名数是否足够
    pub fn has_enough_signatures(&self, count: u8) -> bool {
        count >= self.required_signatures
    }

    /// 获取 Relayer 数量
    pub fn relayer_count(&self) -> usize {
        self.authorized_relayers.len()
    }
}

// ============================================================================
// TradeBatch (交易批次 - 多签)
// ============================================================================

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct RelayerSignature {
    /// Relayer 公钥
    pub relayer: Pubkey,
    /// 签名时间
    pub signed_at: i64,
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct TradeBatch {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 批次 ID
    pub batch_id: u64,
    /// 数据哈希 (SHA256)
    pub data_hash: [u8; 32],
    /// 已收集的签名
    pub signatures: Vec<RelayerSignature>,
    /// 是否已执行
    pub executed: bool,
    /// 创建时间
    pub created_at: i64,
    /// 过期时间
    pub expires_at: i64,
    /// 创建者 (第一个签名的 Relayer)
    pub creator: Pubkey,
    /// Bump
    pub bump: u8,
}

impl TradeBatch {
    pub const DISCRIMINATOR: [u8; 8] = *b"trdbatch";
    pub const SIZE: usize = 8 + // discriminator
        8 + // batch_id
        32 + // data_hash
        4 + ((32 + 8) * MAX_SIGNATURES) + // signatures (Vec<RelayerSignature>)
        1 + // executed
        8 + // created_at
        8 + // expires_at
        32 + // creator
        1 + // bump
        32; // reserved

    /// 添加签名
    pub fn add_signature(&mut self, relayer: Pubkey, timestamp: i64) -> Result<(), crate::error::LedgerError> {
        // 检查是否已签名
        if self.signatures.iter().any(|s| s.relayer == relayer) {
            return Err(crate::error::LedgerError::RelayerAlreadySigned);
        }

        self.signatures.push(RelayerSignature {
            relayer,
            signed_at: timestamp,
        });

        Ok(())
    }

    /// 检查是否过期
    pub fn is_expired(&self, current_time: i64) -> bool {
        current_time > self.expires_at
    }

    /// 获取签名数
    pub fn signature_count(&self) -> u8 {
        self.signatures.len() as u8
    }

    /// 验证数据哈希
    pub fn verify_hash(&self, data: &[u8]) -> bool {
        let computed = crate::utils::compute_hash(data);
        computed == self.data_hash
    }
}

// ============================================================================
// Position (用户仓位 PDA)
// ============================================================================

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct Position {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引 (0=BTC, 1=ETH, 2=SOL, ...)
    pub market_index: u8,
    /// 方向 (Long/Short)
    pub side: Side,
    /// 仓位大小 (合约数量, e6)
    pub size_e6: u64,
    /// 平均入场价格 (e6)
    pub entry_price_e6: u64,
    /// 已锁定保证金 (e6)
    pub margin_e6: u64,
    /// 杠杆倍数
    pub leverage: u8,
    /// 清算价格 (e6)
    pub liquidation_price_e6: u64,
    /// 未实现盈亏 (e6) - 由链下计算，链上验证
    pub unrealized_pnl_e6: i64,
    /// 最后资金费率结算时间
    pub last_funding_ts: i64,
    /// 累计资金费支付 (e6)
    pub cumulative_funding_e6: i64,
    /// 挂单数量
    pub open_order_count: u8,
    /// 开仓时间
    pub opened_at: i64,
    /// 最后更新时间
    pub last_update_ts: i64,
    /// Bump
    pub bump: u8,
    /// 预留空间
    pub reserved: [u8; 32],
}

impl Position {
    pub const DISCRIMINATOR: [u8; 8] = *b"position";
    pub const SIZE: usize = 8 + // discriminator
        32 + // user
        1 + // market_index
        1 + // side
        8 + // size_e6
        8 + // entry_price_e6
        8 + // margin_e6
        1 + // leverage
        8 + // liquidation_price_e6
        8 + // unrealized_pnl_e6
        8 + // last_funding_ts
        8 + // cumulative_funding_e6
        1 + // open_order_count
        8 + // opened_at
        8 + // last_update_ts
        1 + // bump
        32; // reserved

    /// PDA Seeds prefix: ["position", user]
    /// 注意: market_index 需要在调用处传入
    pub const SEED_PREFIX: &'static [u8] = b"position";

    /// 计算仓位价值 (notional value)
    pub fn notional_value_e6(&self) -> Result<u64, ProgramError> {
        // notional = size * entry_price / 1e6
        let result = (self.size_e6 as u128)
            .checked_mul(self.entry_price_e6 as u128)
            .ok_or(crate::error::LedgerError::Overflow)?;
        let result = result.checked_div(1_000_000).ok_or(crate::error::LedgerError::Overflow)?;
        Ok(result as u64)
    }

    /// 计算未实现盈亏
    /// Long: (mark_price - entry_price) * size / 1e6
    /// Short: (entry_price - mark_price) * size / 1e6
    pub fn calculate_unrealized_pnl(&self, mark_price_e6: u64) -> Result<i64, ProgramError> {
        let size = self.size_e6 as i64;
        let entry = self.entry_price_e6 as i64;
        let mark = mark_price_e6 as i64;

        let price_diff = match self.side {
            Side::Long => checked_sub(mark, entry)?,
            Side::Short => checked_sub(entry, mark)?,
        };

        mul_e6(price_diff, size)
    }

    /// 计算清算价格
    /// Long: entry_price * (1 - 1/leverage + maintenance_margin_rate)
    /// Short: entry_price * (1 + 1/leverage - maintenance_margin_rate)
    pub fn calculate_liquidation_price(&self) -> Result<u64, ProgramError> {
        let entry = self.entry_price_e6 as i64;
        let leverage_factor = div_e6(1_000_000, self.leverage as i64)?; // 1/leverage in e6
        let mmr = DEFAULT_MAINTENANCE_MARGIN_RATE; // 2.5% in e6

        let factor = match self.side {
            Side::Long => {
                // 1 - 1/leverage + mmr
                let f = checked_sub(1_000_000, leverage_factor)?;
                checked_add(f, mmr)?
            }
            Side::Short => {
                // 1 + 1/leverage - mmr
                let f = checked_add(1_000_000, leverage_factor)?;
                checked_sub(f, mmr)?
            }
        };

        let liq_price = mul_e6(entry, factor)?;
        Ok(liq_price.max(0) as u64)
    }

    /// 检查是否应该被清算
    pub fn should_liquidate(&self, mark_price_e6: u64) -> bool {
        match self.side {
            Side::Long => mark_price_e6 <= self.liquidation_price_e6,
            Side::Short => mark_price_e6 >= self.liquidation_price_e6,
        }
    }

    /// 更新入场价格 (加仓时)
    /// new_entry = (old_entry * old_size + new_price * add_size) / (old_size + add_size)
    pub fn update_entry_price(&mut self, add_size_e6: u64, add_price_e6: u64) -> Result<(), ProgramError> {
        let old_notional = (self.size_e6 as u128)
            .checked_mul(self.entry_price_e6 as u128)
            .ok_or(crate::error::LedgerError::Overflow)?;
        let add_notional = (add_size_e6 as u128)
            .checked_mul(add_price_e6 as u128)
            .ok_or(crate::error::LedgerError::Overflow)?;

        let total_notional = old_notional
            .checked_add(add_notional)
            .ok_or(crate::error::LedgerError::Overflow)?;
        let total_size = (self.size_e6 as u128)
            .checked_add(add_size_e6 as u128)
            .ok_or(crate::error::LedgerError::Overflow)?;

        if total_size == 0 {
            return Err(crate::error::LedgerError::InvalidPositionSize.into());
        }

        let new_entry = total_notional
            .checked_div(total_size)
            .ok_or(crate::error::LedgerError::Overflow)?;

        self.entry_price_e6 = new_entry as u64;
        self.size_e6 = total_size as u64;

        // 重新计算清算价格
        self.liquidation_price_e6 = self.calculate_liquidation_price()?;

        Ok(())
    }

    /// 检查仓位是否为空
    pub fn is_empty(&self) -> bool {
        self.size_e6 == 0
    }
}

// ============================================================================
// TradeRecord (成交记录)
// ============================================================================

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct TradeRecord {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 全局序列号
    pub sequence: u64,
    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,
    /// 交易类型 (0=Open, 1=Close, 2=Liquidation, 3=ADL)
    pub trade_type: u8,
    /// 方向
    pub side: Side,
    /// 成交数量 (e6)
    pub size_e6: u64,
    /// 成交价格 (e6)
    pub price_e6: u64,
    /// 实现盈亏 (e6) - 仅平仓/清算有值
    pub realized_pnl_e6: i64,
    /// 手续费 (e6)
    pub fee_e6: u64,
    /// 锁定保证金 (e6) - 开仓
    pub margin_locked_e6: u64,
    /// 释放保证金 (e6) - 平仓
    pub margin_released_e6: u64,
    /// 成交时间
    pub timestamp: i64,
    /// 批次 ID
    pub batch_id: u64,
    /// Bump
    pub bump: u8,
}

impl TradeRecord {
    pub const DISCRIMINATOR: [u8; 8] = *b"traderc_";
    pub const SIZE: usize = 8 + // discriminator
        8 + // sequence
        32 + // user
        1 + // market_index
        1 + // trade_type
        1 + // side
        8 + // size_e6
        8 + // price_e6
        8 + // realized_pnl_e6
        8 + // fee_e6
        8 + // margin_locked_e6
        8 + // margin_released_e6
        8 + // timestamp
        8 + // batch_id
        1 + // bump
        32; // reserved

    /// PDA Seeds prefix
    pub const SEED_PREFIX: &'static [u8] = b"trade";
}

/// 交易类型
pub mod trade_type {
    pub const OPEN: u8 = 0;
    pub const CLOSE: u8 = 1;
    pub const LIQUIDATION: u8 = 2;
    pub const ADL: u8 = 3;
    pub const FUNDING: u8 = 4;
}

// ============================================================================
// UserStats (用户统计)
// ============================================================================

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct UserStats {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 用户钱包
    pub user: Pubkey,
    /// 总交易次数
    pub total_trades: u64,
    /// 总成交量 (e6)
    pub total_volume_e6: u64,
    /// 总实现盈亏 (e6)
    pub total_realized_pnl_e6: i64,
    /// 总手续费支付 (e6)
    pub total_fees_paid_e6: u64,
    /// 总资金费支付 (e6)
    pub total_funding_paid_e6: i64,
    /// 总清算次数
    pub total_liquidations: u64,
    /// 首次交易时间
    pub first_trade_at: i64,
    /// 最后交易时间
    pub last_trade_at: i64,
    /// Bump
    pub bump: u8,
}

impl UserStats {
    pub const DISCRIMINATOR: [u8; 8] = *b"usrstats";
    pub const SIZE: usize = 8 + // discriminator
        32 + // user
        8 + // total_trades
        8 + // total_volume_e6
        8 + // total_realized_pnl_e6
        8 + // total_fees_paid_e6
        8 + // total_funding_paid_e6
        8 + // total_liquidations
        8 + // first_trade_at
        8 + // last_trade_at
        1 + // bump
        32; // reserved

    /// PDA Seeds prefix
    pub const SEED_PREFIX: &'static [u8] = b"user_stats";
}

// ============================================================================
// Tests
// ============================================================================

// ============================================================================
// PredictionMarketPosition (预测市场仓位 PDA) - Phase 2 TODO
// ============================================================================

/// 预测市场结果类型
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictionOutcome {
    /// Yes 结果
    Yes,
    /// No 结果
    No,
}

/// 预测市场仓位状态
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictionMarketPositionStatus {
    /// 活跃（可交易）
    Active,
    /// 已结算（等待领取）
    Settled,
    /// 已领取（关闭）
    Claimed,
}

/// 预测市场仓位 (PDA)
/// PDA Seeds: ["prediction_market_position", user, event_id]
/// 
/// 预测市场仓位与永续合约仓位的主要区别：
/// 1. 没有杠杆，1:1 保证金
/// 2. 结算价格只有 0 或 1（对应 No/Yes 赢）
/// 3. 有明确的结算时间
/// 4. 份额代替数量
/// 
/// TODO: Phase 2 实现
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct PredictionMarketPosition {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 用户钱包
    pub user: Pubkey,
    /// 事件ID (SHA256 hash of event name, e.g., "US_ELECTION_2024")
    pub event_id: [u8; 32],
    /// 预测结果 (Yes/No)
    pub outcome: PredictionOutcome,
    /// 持有份额 (e6) - 1 份 = 1 USDC 赢时的收益
    pub shares_e6: u64,
    /// 平均买入价格 (e6) - 0.00 ~ 1.00 USD
    pub avg_price_e6: u64,
    /// 锁定保证金 (e6) = shares * avg_price
    pub margin_e6: u64,
    /// 仓位状态
    pub status: PredictionMarketPositionStatus,
    /// 结算价格 (e6) - 0 或 1_000_000
    pub settlement_price_e6: u64,
    /// 实现盈亏 (e6) - 结算后计算
    pub realized_pnl_e6: i64,
    /// 创建时间
    pub created_at: i64,
    /// 结算时间 (0 = 未结算)
    pub settled_at: i64,
    /// 领取时间 (0 = 未领取)
    pub claimed_at: i64,
    /// Bump
    pub bump: u8,
    /// 预留空间
    pub reserved: [u8; 32],
}

impl PredictionMarketPosition {
    pub const DISCRIMINATOR: [u8; 8] = *b"pm_pos__";
    pub const SIZE: usize = 8 + // discriminator
        32 + // user
        32 + // event_id
        1 + // outcome
        8 + // shares_e6
        8 + // avg_price_e6
        8 + // margin_e6
        1 + // status
        8 + // settlement_price_e6
        8 + // realized_pnl_e6
        8 + // created_at
        8 + // settled_at
        8 + // claimed_at
        1 + // bump
        32; // reserved

    /// PDA Seeds prefix: ["prediction_market_position", user, event_id]
    pub const SEED_PREFIX: &'static [u8] = b"prediction_market_position";

    /// 检查仓位是否为空（可清理）
    pub fn is_empty(&self) -> bool {
        self.shares_e6 == 0 || 
        self.status == PredictionMarketPositionStatus::Claimed
    }

    /// 检查是否已结算
    pub fn is_settled(&self) -> bool {
        self.status == PredictionMarketPositionStatus::Settled ||
        self.status == PredictionMarketPositionStatus::Claimed
    }

    /// 计算未实现盈亏（基于当前市场价格）
    /// 
    /// 如果用户持有 Yes 份额:
    ///   PnL = shares * (current_price - avg_price)
    /// 
    /// 如果用户持有 No 份额:
    ///   PnL = shares * ((1 - current_price) - avg_price)
    ///   Note: No 份额的 avg_price 是买入 No 的价格
    pub fn calculate_unrealized_pnl(&self, current_price_e6: u64) -> i64 {
        if self.shares_e6 == 0 {
            return 0;
        }

        let shares = self.shares_e6 as i64;
        let avg_price = self.avg_price_e6 as i64;
        let current = current_price_e6 as i64;

        // PnL = shares * (current - avg) / 1e6
        let price_diff = current - avg_price;
        (shares as i128 * price_diff as i128 / 1_000_000) as i64
    }

    /// 计算结算盈亏
    /// 
    /// 如果预测正确（赢）: PnL = shares * (1 - avg_price)
    /// 如果预测错误（输）: PnL = -shares * avg_price (损失全部保证金)
    pub fn calculate_settlement_pnl(&self, winning_outcome: PredictionOutcome) -> i64 {
        if self.shares_e6 == 0 {
            return 0;
        }

        let shares = self.shares_e6 as i64;
        let avg_price = self.avg_price_e6 as i64;

        if self.outcome == winning_outcome {
            // 赢了：收益 = 份额 * (1 - 买入价格)
            let profit_per_share = 1_000_000 - avg_price;
            (shares as i128 * profit_per_share as i128 / 1_000_000) as i64
        } else {
            // 输了：损失 = 份额 * 买入价格 (即全部保证金)
            -((shares as i128 * avg_price as i128 / 1_000_000) as i64)
        }
    }
}

/// 预测市场事件配置 (全局 PDA)
/// PDA Seeds: ["prediction_market_event", event_id]
/// 
/// TODO: Phase 2 实现
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct PredictionMarketEvent {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 事件ID (SHA256)
    pub event_id: [u8; 32],
    /// 事件名称 (UTF-8, max 64 bytes)
    pub name: [u8; 64],
    /// 事件描述 (UTF-8, max 256 bytes)
    pub description: [u8; 256],
    /// Yes 份额总供应量 (e6)
    pub yes_supply_e6: u64,
    /// No 份额总供应量 (e6)
    pub no_supply_e6: u64,
    /// 当前 Yes 价格 (e6) - 由 AMM 或订单簿决定
    pub yes_price_e6: u64,
    /// 事件结束时间（结算截止）
    pub end_time: i64,
    /// 是否已结算
    pub is_settled: bool,
    /// 赢家结果 (None = 未结算)
    pub winning_outcome: Option<PredictionOutcome>,
    /// 结算时间
    pub settled_at: i64,
    /// 创建者
    pub creator: Pubkey,
    /// 创建时间
    pub created_at: i64,
    /// Bump
    pub bump: u8,
    /// 预留空间
    pub reserved: [u8; 64],
}

impl PredictionMarketEvent {
    pub const DISCRIMINATOR: [u8; 8] = *b"pm_evnt_";
    pub const SEED_PREFIX: &'static [u8] = b"prediction_market_event";
}

// ============================================================================
// Spot 交易相关结构 (Phase 2/3)
// ============================================================================

/// Spot 交易方向
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotSide {
    /// 买入 (使用 quote 购买 base)
    Buy,
    /// 卖出 (出售 base 获得 quote)
    Sell,
}

/// Spot 成交记录 (PDA)
/// Seeds: ["spot_trade", sequence.to_le_bytes()]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct SpotTradeRecord {
    /// 账户鉴别器
    pub discriminator: [u8; 8],
    /// 全局序列号
    pub sequence: u64,
    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引 (u16 for consistency with Listing Program)
    pub market_index: u16,
    /// 交易方向
    pub side: SpotSide,
    /// Base 数量 (e6)
    pub base_amount_e6: u64,
    /// Quote 数量 (e6)
    pub quote_amount_e6: u64,
    /// 成交价格 (e6)
    pub price_e6: u64,
    /// 手续费 (e6, 以 quote token 计)
    pub fee_e6: u64,
    /// 手续费类型 (0=Taker, 1=Maker)
    pub fee_type: u8,
    /// 成交时间
    pub timestamp: i64,
    /// 批次 ID
    pub batch_id: u64,
    /// Bump
    pub bump: u8,
    /// 预留空间
    pub reserved: [u8; 32],
}

impl SpotTradeRecord {
    pub const DISCRIMINATOR: [u8; 8] = *b"spot_trd";
    pub const SIZE: usize = 8 + // discriminator
        8 + // sequence
        32 + // user
        2 + // market_index (u16)
        1 + // side
        8 + // base_amount_e6
        8 + // quote_amount_e6
        8 + // price_e6
        8 + // fee_e6
        1 + // fee_type
        8 + // timestamp
        8 + // batch_id
        1 + // bump
        32; // reserved

    /// PDA Seeds prefix
    pub const SEED_PREFIX: &'static [u8] = b"spot_trade";
}

/// Spot 手续费类型
pub mod spot_fee_type {
    pub const TAKER: u8 = 0;
    pub const MAKER: u8 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_calculate_unrealized_pnl() {
        let mut pos = Position {
            discriminator: Position::DISCRIMINATOR,
            user: Pubkey::new_unique(),
            market_index: 0,
            side: Side::Long,
            size_e6: 1_000_000, // 1 BTC
            entry_price_e6: 50_000_000_000, // $50,000
            margin_e6: 5_000_000_000, // $5,000
            leverage: 10,
            liquidation_price_e6: 0,
            unrealized_pnl_e6: 0,
            last_funding_ts: 0,
            cumulative_funding_e6: 0,
            open_order_count: 0,
            opened_at: 0,
            last_update_ts: 0,
            bump: 255,
            reserved: [0; 32],
        };

        // Mark price = $55,000 -> PnL = +$5,000
        let pnl = pos.calculate_unrealized_pnl(55_000_000_000).unwrap();
        assert_eq!(pnl, 5_000_000_000); // $5,000 in e6

        // Mark price = $45,000 -> PnL = -$5,000
        let pnl = pos.calculate_unrealized_pnl(45_000_000_000).unwrap();
        assert_eq!(pnl, -5_000_000_000); // -$5,000 in e6

        // Short position
        pos.side = Side::Short;
        // Mark price = $45,000 -> PnL = +$5,000 for short
        let pnl = pos.calculate_unrealized_pnl(45_000_000_000).unwrap();
        assert_eq!(pnl, 5_000_000_000); // $5,000 in e6
    }

    #[test]
    fn test_position_should_liquidate() {
        let mut pos = Position {
            discriminator: Position::DISCRIMINATOR,
            user: Pubkey::new_unique(),
            market_index: 0,
            side: Side::Long,
            size_e6: 1_000_000,
            entry_price_e6: 50_000_000_000, // $50,000
            margin_e6: 5_000_000_000,
            leverage: 10,
            liquidation_price_e6: 45_000_000_000, // $45,000
            unrealized_pnl_e6: 0,
            last_funding_ts: 0,
            cumulative_funding_e6: 0,
            open_order_count: 0,
            opened_at: 0,
            last_update_ts: 0,
            bump: 255,
            reserved: [0; 32],
        };

        // Long: 价格低于清算价 -> 应该清算
        assert!(pos.should_liquidate(44_000_000_000));
        assert!(!pos.should_liquidate(46_000_000_000));

        // Short: 价格高于清算价 -> 应该清算
        pos.side = Side::Short;
        pos.liquidation_price_e6 = 55_000_000_000; // $55,000
        assert!(pos.should_liquidate(56_000_000_000));
        assert!(!pos.should_liquidate(54_000_000_000));
    }

    #[test]
    fn test_trade_batch_add_signature() {
        let mut batch = TradeBatch {
            discriminator: TradeBatch::DISCRIMINATOR,
            batch_id: 1,
            data_hash: [0; 32],
            signatures: vec![],
            executed: false,
            created_at: 0,
            expires_at: 100,
            creator: Pubkey::new_unique(),
            bump: 255,
        };

        let relayer1 = Pubkey::new_unique();
        let relayer2 = Pubkey::new_unique();

        // 添加第一个签名
        batch.add_signature(relayer1, 10).unwrap();
        assert_eq!(batch.signature_count(), 1);

        // 添加第二个签名
        batch.add_signature(relayer2, 20).unwrap();
        assert_eq!(batch.signature_count(), 2);

        // 尝试重复签名 - 应该失败
        let result = batch.add_signature(relayer1, 30);
        assert!(result.is_err());
    }

    #[test]
    fn test_relayer_config() {
        let relayer1 = Pubkey::new_unique();
        let relayer2 = Pubkey::new_unique();
        let relayer3 = Pubkey::new_unique();

        let config = RelayerConfig {
            discriminator: RelayerConfig::DISCRIMINATOR,
            admin: Pubkey::new_unique(),
            authorized_relayers: vec![relayer1, relayer2, relayer3],
            required_signatures: 2,
            bump: 255,
            last_update_ts: 0,
        };

        assert!(config.is_authorized(&relayer1));
        assert!(config.is_authorized(&relayer2));
        assert!(!config.is_authorized(&Pubkey::new_unique()));

        assert!(config.has_enough_signatures(2));
        assert!(config.has_enough_signatures(3));
        assert!(!config.has_enough_signatures(1));
    }

    // ========== Prediction Market Tests ==========

    #[test]
    fn test_prediction_market_position_unrealized_pnl() {
        let pos = PredictionMarketPosition {
            discriminator: PredictionMarketPosition::DISCRIMINATOR,
            user: Pubkey::new_unique(),
            event_id: [0; 32],
            outcome: PredictionOutcome::Yes,
            shares_e6: 100_000_000, // 100 shares
            avg_price_e6: 600_000,  // $0.60 avg price
            margin_e6: 60_000_000,  // 100 * 0.60 = $60
            status: PredictionMarketPositionStatus::Active,
            settlement_price_e6: 0,
            realized_pnl_e6: 0,
            created_at: 0,
            settled_at: 0,
            claimed_at: 0,
            bump: 255,
            reserved: [0; 32],
        };

        // Current price = $0.70 -> PnL = 100 * (0.70 - 0.60) = +$10
        let pnl = pos.calculate_unrealized_pnl(700_000);
        assert_eq!(pnl, 10_000_000); // $10 in e6

        // Current price = $0.50 -> PnL = 100 * (0.50 - 0.60) = -$10
        let pnl = pos.calculate_unrealized_pnl(500_000);
        assert_eq!(pnl, -10_000_000); // -$10 in e6
    }

    #[test]
    fn test_prediction_market_position_settlement_pnl() {
        let pos = PredictionMarketPosition {
            discriminator: PredictionMarketPosition::DISCRIMINATOR,
            user: Pubkey::new_unique(),
            event_id: [0; 32],
            outcome: PredictionOutcome::Yes,
            shares_e6: 100_000_000, // 100 shares
            avg_price_e6: 600_000,  // $0.60 avg price
            margin_e6: 60_000_000,  // $60 margin
            status: PredictionMarketPositionStatus::Active,
            settlement_price_e6: 0,
            realized_pnl_e6: 0,
            created_at: 0,
            settled_at: 0,
            claimed_at: 0,
            bump: 255,
            reserved: [0; 32],
        };

        // Yes wins: PnL = 100 * (1.00 - 0.60) = +$40
        let pnl = pos.calculate_settlement_pnl(PredictionOutcome::Yes);
        assert_eq!(pnl, 40_000_000); // $40 in e6

        // No wins: PnL = -100 * 0.60 = -$60 (lose all margin)
        let pnl = pos.calculate_settlement_pnl(PredictionOutcome::No);
        assert_eq!(pnl, -60_000_000); // -$60 in e6
    }

    #[test]
    fn test_prediction_market_position_is_empty() {
        let mut pos = PredictionMarketPosition {
            discriminator: PredictionMarketPosition::DISCRIMINATOR,
            user: Pubkey::new_unique(),
            event_id: [0; 32],
            outcome: PredictionOutcome::Yes,
            shares_e6: 100_000_000,
            avg_price_e6: 600_000,
            margin_e6: 60_000_000,
            status: PredictionMarketPositionStatus::Active,
            settlement_price_e6: 0,
            realized_pnl_e6: 0,
            created_at: 0,
            settled_at: 0,
            claimed_at: 0,
            bump: 255,
            reserved: [0; 32],
        };

        assert!(!pos.is_empty()); // Has shares, active

        pos.shares_e6 = 0;
        assert!(pos.is_empty()); // No shares

        pos.shares_e6 = 100_000_000;
        pos.status = PredictionMarketPositionStatus::Claimed;
        assert!(pos.is_empty()); // Already claimed
    }
}

