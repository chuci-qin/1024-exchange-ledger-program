//! Ledger Program Instructions
//!
//! 指令分类:
//! 1. 初始化指令 - Initialize, InitializeRelayers
//! 2. 多签指令 - SubmitTradeBatch, ConfirmTradeBatch, ExecuteTradeBatch
//! 3. 交易指令 - OpenPosition, ClosePosition
//! 4. 清算指令 - Liquidate, TriggerADL
//! 5. 资金费率 - SettleFunding
//! 6. 管理指令 - UpdateRelayers, Pause, UpdateAdmin

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;
use crate::state::Side;

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub enum LedgerInstruction {
    // ========================================================================
    // 初始化指令
    // ========================================================================

    /// 初始化 Ledger Program
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` LedgerConfig PDA
    /// 2. `[]` Vault Program
    /// 3. `[]` Fund Program
    /// 4. `[]` System Program
    Initialize {
        delegation_program: Option<Pubkey>,
    },

    /// 初始化多 Relayer 配置
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` RelayerConfig PDA
    /// 2. `[]` System Program
    InitializeRelayers {
        relayers: Vec<Pubkey>,
        required_signatures: u8,
    },

    // ========================================================================
    // 多签指令 (Multi-Relayer)
    // ========================================================================

    /// 提交交易批次 (第一个 Relayer)
    ///
    /// Accounts:
    /// 0. `[signer]` Relayer
    /// 1. `[writable]` TradeBatch PDA (init if not exists)
    /// 2. `[]` RelayerConfig
    /// 3. `[]` System Program
    SubmitTradeBatch {
        batch_id: u64,
        data_hash: [u8; 32],
    },

    /// 确认交易批次 (后续 Relayer)
    ///
    /// Accounts:
    /// 0. `[signer]` Relayer
    /// 1. `[writable]` TradeBatch PDA
    /// 2. `[]` RelayerConfig
    ConfirmTradeBatch {
        batch_id: u64,
        data_hash: [u8; 32],
    },

    /// 执行交易批次 (签名足够后)
    ///
    /// 账户布局:
    /// 0. `[signer]` Any authorized Relayer
    /// 1. `[writable]` TradeBatch PDA
    /// 2. `[]` RelayerConfig
    /// 3. `[writable]` LedgerConfig
    /// 4. `[]` VaultConfig
    /// 5. `[]` Vault Program
    /// 6. `[]` Ledger Program (self)
    /// 7. `[]` System Program
    /// 8. `[writable]` Insurance Fund (for close positions, can be SystemProgram if no closes)
    /// 
    /// 然后每笔交易需要 3 个账户:
    /// For trade i (starting from index 9):
    ///   9 + i*3 + 0: `[writable]` Position PDA (seeds: ["position", user, market_index])
    ///   9 + i*3 + 1: `[writable]` UserAccount (Vault)
    ///   9 + i*3 + 2: `[writable]` UserStats PDA
    ///
    /// 示例: 2 笔交易需要 9 + 6 = 15 个账户
    ExecuteTradeBatch {
        batch_id: u64,
        trades: Vec<TradeData>,
    },

    // ========================================================================
    // 交易指令 (原子操作)
    // ========================================================================

    /// 开仓 (原子操作)
    /// 1. 创建/更新 Position PDA
    /// 2. CPI 调用 Vault.lockMargin
    /// 3. 创建 TradeRecord
    ///
    /// Accounts:
    /// 0. `[signer]` Relayer (or user for direct trades)
    /// 1. `[writable]` Position PDA
    /// 2. `[writable]` UserAccount (Vault)
    /// 3. `[writable]` VaultConfig
    /// 4. `[writable]` LedgerConfig
    /// 5. `[writable]` UserStats PDA
    /// 6. `[writable]` TradeRecord PDA
    /// 7. `[]` Vault Program
    /// 8. `[]` System Program
    OpenPosition {
        user: Pubkey,
        market_index: u8,
        side: Side,
        size_e6: u64,
        price_e6: u64,
        leverage: u8,
        batch_id: u64,
    },

    /// 平仓 (原子操作)
    /// 1. 读取 Position
    /// 2. 计算 PnL
    /// 3. CPI 调用 Vault.closePositionSettle
    /// 4. 更新/关闭 Position
    /// 5. 创建 TradeRecord
    ///
    /// Accounts:
    /// 0. `[signer]` Relayer (or user)
    /// 1. `[writable]` Position PDA
    /// 2. `[writable]` UserAccount (Vault)
    /// 3. `[writable]` VaultConfig
    /// 4. `[writable]` InsuranceFund
    /// 5. `[writable]` LedgerConfig
    /// 6. `[writable]` UserStats PDA
    /// 7. `[writable]` TradeRecord PDA
    /// 8. `[]` Vault Program
    /// 9. `[]` System Program
    ClosePosition {
        user: Pubkey,
        market_index: u8,
        size_e6: u64,
        price_e6: u64,
        batch_id: u64,
    },

    // ========================================================================
    // 清算指令
    // ========================================================================

    /// 清算 (原子操作)
    /// 1. 验证清算条件 (mark_price vs liquidation_price)
    /// 2. CPI 调用 Vault.LiquidatePosition (更新用户账户 + 转移罚金到 Insurance Fund)
    /// 3. CPI 调用 Fund.AddLiquidationIncome (更新保险基金统计)
    /// 4. CPI 调用 Fund.CoverShortfall (如有穿仓)
    /// 5. 关闭 Position
    /// 6. 更新 UserStats
    ///
    /// Accounts:
    /// 0. `[signer]` Liquidator (can be anyone)
    /// 1. `[writable]` Position PDA
    /// 2. `[writable]` UserAccount (Vault)
    /// 3. `[]` VaultConfig
    /// 4. `[writable]` LedgerConfig
    /// 5. `[writable]` UserStats PDA
    /// 6. `[]` Vault Program
    /// 7. `[writable]` Vault Token Account (用于罚金转出)
    /// 8. `[]` Fund Program
    /// 9. `[writable]` Insurance Fund Account (Fund Program)
    /// 10. `[writable]` InsuranceFundConfig (Fund Program)
    /// 11. `[writable]` Insurance Fund Vault (接收罚金)
    /// 12. `[writable]` Counterparty Vault (穿仓时接收覆盖)
    /// 13. `[]` Token Program
    Liquidate {
        user: Pubkey,
        market_index: u8,
        mark_price_e6: u64,
    },

    /// 触发 ADL (自动减仓)
    /// 当保险基金不足以覆盖穿仓时触发
    ///
    /// Accounts:
    /// 0. `[signer]` Admin or Relayer
    /// 1. `[]` InsuranceFund (或 InsuranceFundConfig in Fund Program)
    /// 2. `[writable]` LedgerConfig
    /// 3+ `[writable]` Target Position PDAs (按盈利排序)
    TriggerADL {
        market_index: u8,
        shortfall_e6: u64,
        /// 穿仓方向 (需要 ADL 反向盈利仓位)
        bankrupt_side: Side,
    },

    // ========================================================================
    // 资金费率
    // ========================================================================

    /// 结算资金费率
    ///
    /// Accounts:
    /// 0. `[signer]` Relayer
    /// 1. `[writable]` Position PDA
    /// 2. `[writable]` UserAccount (Vault)
    /// 3. `[writable]` VaultConfig
    /// 4. `[]` Vault Program
    SettleFunding {
        user: Pubkey,
        market_index: u8,
        funding_rate_e6: i64,
        index_price_e6: u64,
    },

    // ========================================================================
    // 管理指令
    // ========================================================================

    /// 添加 Relayer
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` RelayerConfig PDA
    AddRelayer {
        relayer: Pubkey,
    },

    /// 移除 Relayer
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` RelayerConfig PDA
    RemoveRelayer {
        relayer: Pubkey,
    },

    /// 更新所需签名数
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` RelayerConfig PDA
    UpdateRequiredSignatures {
        required_signatures: u8,
    },

    /// 暂停/恢复
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` LedgerConfig PDA
    SetPaused {
        paused: bool,
    },

    /// 更新管理员
    ///
    /// Accounts:
    /// 0. `[signer]` Current Admin
    /// 1. `[writable]` LedgerConfig PDA
    UpdateAdmin {
        new_admin: Pubkey,
    },

    /// 更新 Vault Program ID
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` LedgerConfig PDA
    UpdateVaultProgram {
        new_vault_program: Pubkey,
    },

    /// 更新 Fund Program ID
    ///
    /// Accounts:
    /// 0. `[signer]` Admin
    /// 1. `[writable]` LedgerConfig PDA
    UpdateFundProgram {
        new_fund_program: Pubkey,
    },

    // ========================================================================
    // 用户初始化
    // ========================================================================

    /// 初始化用户统计
    ///
    /// Accounts:
    /// 0. `[signer]` User
    /// 1. `[writable]` UserStats PDA
    /// 2. `[]` System Program
    InitializeUserStats,
}

/// 单笔交易数据 (用于批量执行)
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq)]
pub struct TradeData {
    /// 用户
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,
    /// 交易类型 (0=Open, 1=Close)
    pub trade_type: u8,
    /// 方向
    pub side: Side,
    /// 数量 (e6)
    pub size_e6: u64,
    /// 价格 (e6)
    pub price_e6: u64,
    /// 杠杆 (仅开仓)
    pub leverage: u8,
}

/// 交易数据类型常量
pub mod trade_data_type {
    pub const OPEN: u8 = 0;
    pub const CLOSE: u8 = 1;
}

