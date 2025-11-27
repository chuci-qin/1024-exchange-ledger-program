//! Ledger Program Error Types

use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerError {
    // === 通用错误 ===
    #[error("Invalid instruction data")]
    InvalidInstructionData,

    #[error("Invalid account")]
    InvalidAccount,

    #[error("Account not writable")]
    AccountNotWritable,

    #[error("Missing required signature")]
    MissingRequiredSignature,

    #[error("Arithmetic overflow")]
    Overflow,

    #[error("Invalid program ID")]
    InvalidProgramId,

    // === Relayer 相关 ===
    #[error("Unauthorized relayer")]
    UnauthorizedRelayer,

    #[error("Insufficient signatures for multi-sig")]
    InsufficientSignatures,

    #[error("Duplicate signature")]
    DuplicateSignature,

    #[error("Trade batch expired")]
    TradeBatchExpired,

    #[error("Trade batch already executed")]
    TradeBatchAlreadyExecuted,

    #[error("Invalid data hash")]
    InvalidDataHash,

    #[error("Relayer already signed")]
    RelayerAlreadySigned,

    // === Position 相关 ===
    #[error("Position not found")]
    PositionNotFound,

    #[error("Position already exists")]
    PositionAlreadyExists,

    #[error("Invalid position side")]
    InvalidPositionSide,

    #[error("Position has open orders")]
    PositionHasOpenOrders,

    #[error("Invalid position size")]
    InvalidPositionSize,

    #[error("Position size exceeds max")]
    PositionSizeExceedsMax,

    // === 交易相关 ===
    #[error("Invalid trade amount")]
    InvalidTradeAmount,

    #[error("Invalid price")]
    InvalidPrice,

    #[error("Invalid leverage")]
    InvalidLeverage,

    #[error("Leverage exceeds max")]
    LeverageExceedsMax,

    #[error("Insufficient margin")]
    InsufficientMargin,

    #[error("Invalid market index")]
    InvalidMarketIndex,

    #[error("Market not active")]
    MarketNotActive,

    // === 清算相关 ===
    #[error("Position not liquidatable")]
    PositionNotLiquidatable,

    #[error("Invalid liquidation price")]
    InvalidLiquidationPrice,

    #[error("Liquidation in progress")]
    LiquidationInProgress,

    // === ADL 相关 ===
    #[error("ADL not triggered")]
    ADLNotTriggered,

    #[error("No opposing positions for ADL")]
    NoOpposingPositionsForADL,

    /// P0-2: ADL 未被要求（保险基金充足）
    #[error("ADL not required - insurance fund sufficient")]
    ADLNotRequired,

    /// P0-2: 无效的 ADL 目标仓位
    #[error("Invalid ADL target position")]
    InvalidADLTarget,

    /// P0-2: ADL 已在进行中
    #[error("ADL already in progress")]
    ADLInProgress,

    // === Funding 相关 ===
    #[error("Funding not due")]
    FundingNotDue,

    #[error("Invalid funding rate")]
    InvalidFundingRate,

    // === CPI 相关 ===
    #[error("CPI call failed")]
    CPICallFailed,

    #[error("Invalid vault program")]
    InvalidVaultProgram,

    // === 管理相关 ===
    #[error("Invalid admin")]
    InvalidAdmin,

    #[error("Ledger paused")]
    LedgerPaused,

    #[error("Already initialized")]
    AlreadyInitialized,

    // === Batch 相关 ===
    #[error("Insufficient accounts for trade batch")]
    InsufficientAccounts,
}

impl From<LedgerError> for ProgramError {
    fn from(e: LedgerError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

