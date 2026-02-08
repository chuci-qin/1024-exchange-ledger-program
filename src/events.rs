//! Structured Event Logs (Layer 2) for 1024 Exchange Ledger Program
//!
//! All exchange events are Borsh-serialized, Base64-encoded, and emitted
//! via `msg!()` into transaction logs. These logs are immutable and can be
//! parsed by the Geyser Plugin / Indexer / Block Explorer.
//!
//! Format: `EVENT:<EventName>:<Base64(BorshSerialize(event))>`
//!
//! 10 Event Types:
//!   1. OrderEvent          — 订单生命周期 (下单/成交/取消/过期/拒绝/修改)
//!   2. TradeEvent          — Perp/Spot 成交
//!   3. PositionEvent       — 仓位变动 (开/加/减/平/反转/清算/ADL)
//!   4. LiquidationEvent    — 清算触发和执行
//!   5. ADLEvent            — 自动减仓
//!   6. FundingEvent        — 资金费率结算
//!   7. DepositWithdrawEvent— 入出金/划转
//!   8. FeeEvent            — 费用收取明细
//!   9. InsuranceFundEvent  — 保险金变动
//!  10. BatchEvent          — 结算批次状态

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{msg, pubkey::Pubkey};

// ============================================================================
// Event Discriminators (8 bytes each)
// ============================================================================

/// Event discriminator constants used by Indexer to identify event type
/// without full deserialization.
pub mod event_discriminator {
    pub const ORDER: [u8; 8] = *b"evt_ordr";
    pub const TRADE: [u8; 8] = *b"evt_trad";
    pub const POSITION: [u8; 8] = *b"evt_posn";
    pub const LIQUIDATION: [u8; 8] = *b"evt_liqd";
    pub const ADL: [u8; 8] = *b"evt_adl_";
    pub const FUNDING: [u8; 8] = *b"evt_fund";
    pub const DEPOSIT_WITHDRAW: [u8; 8] = *b"evt_depw";
    pub const FEE: [u8; 8] = *b"evt_fee_";
    pub const INSURANCE_FUND: [u8; 8] = *b"evt_insf";
    pub const BATCH: [u8; 8] = *b"evt_btch";
}

// ============================================================================
// Event Name Constants (for msg! log prefix)
// ============================================================================

pub const EVENT_PREFIX: &str = "EVENT";
pub const ORDER_EVENT_NAME: &str = "OrderEvent";
pub const TRADE_EVENT_NAME: &str = "TradeEvent";
pub const POSITION_EVENT_NAME: &str = "PositionEvent";
pub const LIQUIDATION_EVENT_NAME: &str = "LiquidationEvent";
pub const ADL_EVENT_NAME: &str = "ADLEvent";
pub const FUNDING_EVENT_NAME: &str = "FundingEvent";
pub const DEPOSIT_WITHDRAW_EVENT_NAME: &str = "DepositWithdrawEvent";
pub const FEE_EVENT_NAME: &str = "FeeEvent";
pub const INSURANCE_FUND_EVENT_NAME: &str = "InsuranceFundEvent";
pub const BATCH_EVENT_NAME: &str = "BatchEvent";

// ============================================================================
// 1. OrderEvent
// ============================================================================

/// Order status (u8 enum for efficiency)
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderStatus {
    /// 订单已提交
    Placed = 0,
    /// 订单已接受, 进入 OrderBook
    Accepted = 1,
    /// 部分成交
    PartialFill = 2,
    /// 完全成交
    Filled = 3,
    /// 用户取消
    Cancelled = 4,
    /// 过期 (GTD/IOC/FOK)
    Expired = 5,
    /// 被拒绝
    Rejected = 6,
    /// 订单修改 (改价/改量)
    Amended = 7,
    /// 条件单触发
    Triggered = 8,
    /// 止损触发
    SLTriggered = 9,
    /// 止盈触发
    TPTriggered = 10,
}

/// Reason for order status change
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StatusReason {
    None = 0,
    InsufficientMargin = 1,
    InsufficientBalance = 2,
    SelfTrade = 3,
    PostOnlyWouldCross = 4,
    ReduceOnlyNoPosition = 5,
    MarketPaused = 6,
    PriceOutOfRange = 7,
    UserCancelled = 8,
    IOCNotFilled = 9,
    FOKNotFilled = 10,
    GTDExpired = 11,
    Liquidation = 12,
    ADL = 13,
}

/// OrderEvent — 订单生命周期事件
///
/// Emitted whenever an order transitions state: placed, accepted, partially
/// filled, fully filled, cancelled, expired, rejected, amended, or triggered.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct OrderEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    // --- 订单标识 ---
    /// 128-bit Order ID
    pub order_id: [u8; 16],
    /// 用户自定义 ID (client-assigned, optional — all zeros if unused)
    pub client_order_id: [u8; 16],
    /// 用户钱包
    pub user: Pubkey,

    // --- 市场 ---
    /// 市场索引 (0=BTC, 1=ETH, 2=SOL, ...)
    pub market_index: u8,
    /// 0=Perp, 1=Spot
    pub market_type: u8,

    // --- 订单信息 ---
    /// 0=Long/Buy, 1=Short/Sell
    pub side: u8,
    /// 0=Market, 1=Limit, 2=StopMarket, 3=StopLimit, 4=TakeProfitMarket, 5=TakeProfitLimit
    pub order_type: u8,
    /// 0=GTC, 1=GTD, 2=IOC, 3=FOK
    pub time_in_force: u8,
    /// Reduce-only flag
    pub reduce_only: bool,
    /// Post-only flag
    pub post_only: bool,

    // --- 价格和数量 ---
    /// 限价 (e6) — Market 单为 0
    pub price_e6: u64,
    /// 原始数量 (e6)
    pub size_e6: u64,
    /// 已成交数量 (e6)
    pub filled_size_e6: u64,
    /// 剩余数量 (e6)
    pub remaining_size_e6: u64,
    /// 触发价 (e6) — 条件单, 非条件单为 0
    pub trigger_price_e6: u64,
    /// 平均成交价 (e6)
    pub avg_fill_price_e6: u64,

    // --- 状态 ---
    /// Order status (see OrderStatus enum)
    pub status: u8,
    /// Status reason (see StatusReason enum)
    pub status_reason: u8,
}

// ============================================================================
// 2. TradeEvent
// ============================================================================

/// Trade type classification
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TradeType {
    /// 普通撮合成交
    Normal = 0,
    /// 清算成交
    Liquidation = 1,
    /// 自动减仓成交
    ADL = 2,
    /// Funding 结算
    Funding = 3,
}

/// TradeEvent — Perp/Spot 成交事件
///
/// Emitted for every fill. Contains both maker and taker information,
/// including fees, PnL, and margin deltas.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct TradeEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局成交序号 (链上唯一)
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,
    /// 所属结算批次
    pub batch_id: u64,

    // --- 市场 ---
    /// 市场索引
    pub market_index: u8,
    /// 0=Perp, 1=Spot
    pub market_type: u8,

    // --- 成交类型 ---
    /// See TradeType enum (0=Normal, 1=Liquidation, 2=ADL, 3=Funding)
    pub trade_type: u8,

    // --- Maker ---
    pub maker: Pubkey,
    pub maker_order_id: [u8; 16],
    /// 0=Long/Buy, 1=Short/Sell
    pub maker_side: u8,
    /// Maker fee (e6) — negative value = rebate
    pub maker_fee_e6: i64,

    // --- Taker ---
    pub taker: Pubkey,
    pub taker_order_id: [u8; 16],
    /// 0=Long/Buy, 1=Short/Sell
    pub taker_side: u8,
    /// Taker fee (e6)
    pub taker_fee_e6: i64,

    // --- 成交详情 ---
    /// 成交价格 (e6)
    pub price_e6: u64,
    /// 成交数量 (e6)
    pub size_e6: u64,
    /// 名义价值 (e6) = price * size
    pub notional_e6: u64,

    // --- PnL (平仓时) ---
    pub maker_realized_pnl_e6: i64,
    pub taker_realized_pnl_e6: i64,

    // --- 保证金变动 (Perp) ---
    /// Positive = locked, Negative = released
    pub maker_margin_delta_e6: i64,
    pub taker_margin_delta_e6: i64,
}

// ============================================================================
// 3. PositionEvent
// ============================================================================

/// Position event type — describes what happened to the position
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PositionEventType {
    /// 新开仓
    Opened = 0,
    /// 加仓
    Increased = 1,
    /// 减仓
    Decreased = 2,
    /// 完全平仓
    Closed = 3,
    /// 反转 (Long→Short 或 Short→Long)
    Flipped = 4,
    /// 被清算
    Liquidated = 5,
    /// 被 ADL
    ADLed = 6,
}

/// PositionEvent — 仓位变动事件
///
/// Emitted whenever a user's position changes. Contains before/after
/// snapshots and the delta, enabling full position history reconstruction.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct PositionEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,

    /// Position event type (see PositionEventType enum)
    pub event_type: u8,

    // --- 变动前 ---
    /// Side before (0=Long, 1=Short)
    pub side_before: u8,
    pub size_before_e6: u64,
    pub entry_price_before_e6: u64,
    pub margin_before_e6: u64,

    // --- 变动后 ---
    /// Side after (0=Long, 1=Short)
    pub side_after: u8,
    pub size_after_e6: u64,
    pub entry_price_after_e6: u64,
    pub margin_after_e6: u64,

    // --- 变动量 ---
    /// Positive = increase, Negative = decrease
    pub size_delta_e6: i64,
    pub realized_pnl_e6: i64,
    pub fee_e6: u64,

    /// 触发此仓位变动的 trade sequence
    pub related_trade_sequence: u64,
}

// ============================================================================
// 4. LiquidationEvent
// ============================================================================

/// LiquidationEvent — 清算触发和执行
///
/// Emitted when a position is liquidated due to insufficient margin.
/// Contains full position snapshot at liquidation time.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct LiquidationEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 被清算用户
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,

    // --- 被清算仓位 ---
    /// 0=Long, 1=Short
    pub side: u8,
    /// 仓位大小 (e6)
    pub position_size_e6: u64,
    /// 入场价格 (e6)
    pub entry_price_e6: u64,
    /// 清算时标记价格 (e6)
    pub mark_price_e6: u64,
    /// 设定的清算价格 (e6)
    pub liquidation_price_e6: u64,

    // --- 保证金 ---
    /// 保证金 (e6)
    pub margin_e6: u64,
    /// 清算时保证金率 (e6)
    pub margin_ratio_e6: u64,

    // --- 清算结果 ---
    /// 清算罚金 (e6)
    pub penalty_e6: u64,
    /// 保险金赔付 (e6)
    pub insurance_payout_e6: u64,
    /// 剩余保证金 (e6) — may be negative (bankruptcy)
    pub remaining_margin_e6: i64,
    /// 是否破产 (margin < 0)
    pub is_bankruptcy: bool,
    /// 已实现盈亏 (e6)
    pub realized_pnl_e6: i64,

    // --- 关联 ---
    /// 关联的 trade sequence
    pub related_trade_sequence: u64,
}

// ============================================================================
// 5. ADLEvent
// ============================================================================

/// ADL trigger reason
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ADLTriggerReason {
    /// 清算后破产, 保险金不足
    Bankruptcy = 0,
    /// 预防性 ADL (保险金低于阈值)
    Preemptive = 1,
}

/// ADLEvent — 自动减仓事件
///
/// Emitted when Auto-Deleveraging occurs. A bankrupt user's position
/// is force-closed against a profitable counterparty.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct ADLEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 市场索引
    pub market_index: u8,
    /// ADL trigger reason (see ADLTriggerReason)
    pub trigger_reason: u8,

    // --- 保险金状态 ---
    /// 保险金缺口 (e6)
    pub shortfall_e6: u64,
    pub insurance_balance_before_e6: i64,
    pub insurance_balance_after_e6: i64,

    // --- 破产方 ---
    pub bankrupt_user: Pubkey,
    /// 0=Long, 1=Short
    pub bankrupt_side: u8,
    pub bankrupt_size_e6: u64,

    // --- 对手方 (被 ADL 的盈利方) ---
    pub counterparty_user: Pubkey,
    /// 0=Long, 1=Short
    pub counterparty_side: u8,
    pub counterparty_size_reduced_e6: u64,
    pub counterparty_pnl_e6: i64,

    /// 关联的 trade sequence
    pub related_trade_sequence: u64,
}

// ============================================================================
// 6. FundingEvent
// ============================================================================

/// FundingEvent — 资金费率结算事件
///
/// Emitted for each user position when funding rates are settled.
/// Positive payment means the user pays; negative means the user receives.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct FundingEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,

    // --- 仓位信息 ---
    /// 0=Long, 1=Short
    pub side: u8,
    /// 当前仓位大小 (e6)
    pub position_size_e6: u64,

    // --- Funding 详情 ---
    /// 当期资金费率 (e9 精度)
    pub funding_rate_e9: i64,
    /// 资金费支付 (e6) — positive=pay, negative=receive
    pub payment_e6: i64,
    /// 标记价格 (e6)
    pub mark_price_e6: u64,

    // --- 期间 ---
    /// Funding 周期开始时间
    pub period_start: i64,
    /// Funding 周期结束时间
    pub period_end: i64,
}

// ============================================================================
// 7. DepositWithdrawEvent
// ============================================================================

/// Deposit / Withdraw event type
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DepositWithdrawType {
    /// Perp 入金
    Deposit = 0,
    /// Perp 出金
    Withdraw = 1,
    /// Spot 入金
    SpotDeposit = 2,
    /// Spot 出金
    SpotWithdraw = 3,
    /// 内部划转 (Perp ↔ Spot)
    InternalTransfer = 4,
    /// Relayer 代理入金
    RelayerDeposit = 5,
    /// 跨链桥入金
    BridgeDeposit = 6,
}

/// DepositWithdrawEvent — 入出金 / 划转事件
///
/// Emitted for every deposit, withdrawal, or internal transfer.
/// Includes balance snapshots before and after for auditability.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct DepositWithdrawEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 用户钱包
    pub user: Pubkey,

    /// Event type (see DepositWithdrawType enum)
    pub event_type: u8,

    /// Token index (u16 for compatibility with Listing Program)
    pub token_index: u16,
    /// 金额 (e6)
    pub amount_e6: u64,
    /// 变动前余额 (e6)
    pub balance_before_e6: u64,
    /// 变动后余额 (e6)
    pub balance_after_e6: u64,

    // --- 来源 (跨链入金) ---
    /// Source chain: 0=1024Chain, 1=Solana, 2=Ethereum, etc.
    pub source_chain: u8,
    /// Source chain tx hash (all zeros if same-chain)
    pub source_tx_hash: [u8; 32],
}

// ============================================================================
// 8. FeeEvent
// ============================================================================

/// Fee type classification
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FeeType {
    /// Maker fee (may be negative = rebate)
    MakerFee = 0,
    /// Taker fee
    TakerFee = 1,
    /// Liquidation penalty fee
    LiquidationPenalty = 2,
    /// Insurance fund contribution
    InsuranceContribution = 3,
    /// Funding fee
    FundingFee = 4,
}

/// FeeEvent — 费用收取明细事件
///
/// Emitted whenever a fee is charged or rebated. Linked to the originating
/// trade via `related_trade_sequence`.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct FeeEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// 用户钱包
    pub user: Pubkey,
    /// 市场索引
    pub market_index: u8,

    /// Fee type (see FeeType enum)
    pub fee_type: u8,
    /// Fee amount (e6) — positive=charged, negative=rebated
    pub amount_e6: i64,
    /// 关联的 trade sequence
    pub related_trade_sequence: u64,
}

// ============================================================================
// 9. InsuranceFundEvent
// ============================================================================

/// Insurance fund event type
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InsuranceFundEventType {
    /// 清算罚金收入
    LiquidationIncome = 0,
    /// ADL 盈余收入
    ADLProfit = 1,
    /// 破产损失赔付 (保险金支出)
    ShortfallCover = 2,
    /// 手续费分成收入
    FeeIncome = 3,
}

/// InsuranceFundEvent — 保险金变动事件
///
/// Emitted whenever the insurance fund balance changes. Contains
/// before/after snapshots for reconciliation.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct InsuranceFundEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 全局事件序号
    pub sequence: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// Event type (see InsuranceFundEventType enum)
    pub event_type: u8,
    /// 市场索引
    pub market_index: u8,
    /// Amount (e6) — positive=inflow, negative=outflow
    pub amount_e6: i64,
    /// Balance before (e6)
    pub balance_before_e6: i64,
    /// Balance after (e6)
    pub balance_after_e6: i64,

    /// 关联用户 (e.g. 被清算用户)
    pub related_user: Pubkey,
    /// Additional reason code
    pub reason: u8,
}

// ============================================================================
// 10. BatchEvent
// ============================================================================

/// Batch settlement status
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchStatus {
    /// 批次已提交
    Submitted = 0,
    /// 多签确认完成
    Confirmed = 1,
    /// 批次已执行
    Executed = 2,
    /// 批次执行失败
    Failed = 3,
}

/// BatchEvent — 结算批次状态事件
///
/// Emitted at each stage of the batch settlement lifecycle:
/// submitted → confirmed (multi-sig) → executed / failed.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub struct BatchEvent {
    /// Event discriminator
    pub discriminator: [u8; 8],
    /// 批次 ID (this event uses batch_id as its primary key, not sequence)
    pub batch_id: u64,
    /// Unix timestamp (seconds)
    pub timestamp: i64,

    /// Batch status (see BatchStatus enum)
    pub event_type: u8,
    /// Number of trades in this batch
    pub trade_count: u16,
    /// Total notional value (e6)
    pub total_notional_e6: u64,
    /// Relayer who submitted / confirmed / executed
    pub relayer: Pubkey,

    /// SHA-256 hash of batch data
    pub data_hash: [u8; 32],
    /// On-chain transaction signature (64 bytes)
    pub chain_tx: [u8; 64],
    /// Error code: 0=None, >0=specific error
    pub error_code: u8,
}

// ============================================================================
// Helper: Base64 Encoding (no external dependency)
// ============================================================================

/// Standard Base64 alphabet
const BASE64_CHARS: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes to a Base64 string.
///
/// Minimal implementation for use inside Solana programs without
/// pulling in the `base64` crate.
fn base64_encode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity((data.len() + 2) / 3 * 4);
    let chunks = data.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize]);
        result.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize]);

        if chunk.len() > 1 {
            result.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize]);
        } else {
            result.push(b'=');
        }

        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(triple & 0x3F) as usize]);
        } else {
            result.push(b'=');
        }
    }

    result
}

// ============================================================================
// Helper: emit_event
// ============================================================================

/// Serialize a Borsh-encodable event to Base64 and emit it via `msg!()`.
///
/// Log format: `EVENT:<event_name>:<base64_data>`
///
/// The Geyser Plugin / Indexer can:
///   1. Scan transaction logs for lines matching `^EVENT:`
///   2. Split on `:` to extract the event name and Base64 payload
///   3. Base64-decode → Borsh-deserialize into the corresponding struct
///
/// # Arguments
/// * `event_name` — one of the `*_EVENT_NAME` constants (e.g. `"TradeEvent"`)
/// * `event`      — reference to a `BorshSerialize`-implementing event struct
pub fn emit_event<T: BorshSerialize>(event_name: &str, event: &T) {
    // Borsh-serialize the event
    let data = match borsh::to_vec(event) {
        Ok(d) => d,
        Err(_) => {
            msg!("EVENT_ERROR: Failed to serialize {}", event_name);
            return;
        }
    };

    // Base64-encode
    let encoded = base64_encode(&data);

    // SAFETY: the base64 alphabet is pure ASCII, so this is always valid UTF-8
    let encoded_str = core::str::from_utf8(&encoded).unwrap_or("ENCODING_ERROR");

    msg!("{}:{}:{}", EVENT_PREFIX, event_name, encoded_str);
}

// ============================================================================
// Convenience emit wrappers
// ============================================================================

/// Emit an OrderEvent
pub fn emit_order_event(event: &OrderEvent) {
    emit_event(ORDER_EVENT_NAME, event);
}

/// Emit a TradeEvent
pub fn emit_trade_event(event: &TradeEvent) {
    emit_event(TRADE_EVENT_NAME, event);
}

/// Emit a PositionEvent
pub fn emit_position_event(event: &PositionEvent) {
    emit_event(POSITION_EVENT_NAME, event);
}

/// Emit a LiquidationEvent
pub fn emit_liquidation_event(event: &LiquidationEvent) {
    emit_event(LIQUIDATION_EVENT_NAME, event);
}

/// Emit an ADLEvent
pub fn emit_adl_event(event: &ADLEvent) {
    emit_event(ADL_EVENT_NAME, event);
}

/// Emit a FundingEvent
pub fn emit_funding_event(event: &FundingEvent) {
    emit_event(FUNDING_EVENT_NAME, event);
}

/// Emit a DepositWithdrawEvent
pub fn emit_deposit_withdraw_event(event: &DepositWithdrawEvent) {
    emit_event(DEPOSIT_WITHDRAW_EVENT_NAME, event);
}

/// Emit a FeeEvent
pub fn emit_fee_event(event: &FeeEvent) {
    emit_event(FEE_EVENT_NAME, event);
}

/// Emit an InsuranceFundEvent
pub fn emit_insurance_fund_event(event: &InsuranceFundEvent) {
    emit_event(INSURANCE_FUND_EVENT_NAME, event);
}

/// Emit a BatchEvent
pub fn emit_batch_event(event: &BatchEvent) {
    emit_event(BATCH_EVENT_NAME, event);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode_empty() {
        let result = base64_encode(&[]);
        assert_eq!(result, b"");
    }

    #[test]
    fn test_base64_encode_one_byte() {
        // 'A' (0x41) → QVAA (padded)
        // Actually: 0x41 => 010000 01xxxx xxxxxx -> Q, Q, =, =
        let result = base64_encode(&[0x41]);
        assert_eq!(&result, b"QQ==");
    }

    #[test]
    fn test_base64_encode_hello() {
        let result = base64_encode(b"Hello");
        let result_str = core::str::from_utf8(&result).unwrap();
        assert_eq!(result_str, "SGVsbG8=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        // Exact multiple of 3 — no padding
        let result = base64_encode(b"abc");
        let result_str = core::str::from_utf8(&result).unwrap();
        assert_eq!(result_str, "YWJj");
    }

    #[test]
    fn test_order_status_borsh_roundtrip() {
        let status = OrderStatus::PartialFill;
        let data = borsh::to_vec(&status).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0], 2); // PartialFill = 2

        let decoded = OrderStatus::try_from_slice(&data).unwrap();
        assert_eq!(decoded, OrderStatus::PartialFill);
    }

    #[test]
    fn test_status_reason_borsh_roundtrip() {
        let reason = StatusReason::PostOnlyWouldCross;
        let data = borsh::to_vec(&reason).unwrap();
        assert_eq!(data[0], 4);

        let decoded = StatusReason::try_from_slice(&data).unwrap();
        assert_eq!(decoded, StatusReason::PostOnlyWouldCross);
    }

    #[test]
    fn test_trade_type_borsh_roundtrip() {
        let tt = TradeType::ADL;
        let data = borsh::to_vec(&tt).unwrap();
        assert_eq!(data[0], 2);

        let decoded = TradeType::try_from_slice(&data).unwrap();
        assert_eq!(decoded, TradeType::ADL);
    }

    #[test]
    fn test_position_event_type_borsh_roundtrip() {
        let pet = PositionEventType::Flipped;
        let data = borsh::to_vec(&pet).unwrap();
        assert_eq!(data[0], 4);

        let decoded = PositionEventType::try_from_slice(&data).unwrap();
        assert_eq!(decoded, PositionEventType::Flipped);
    }

    #[test]
    fn test_order_event_borsh_roundtrip() {
        let event = OrderEvent {
            discriminator: event_discriminator::ORDER,
            sequence: 12345,
            timestamp: 1700000000,
            order_id: [1u8; 16],
            client_order_id: [0u8; 16],
            user: Pubkey::new_unique(),
            market_index: 0,
            market_type: 0,
            side: 0,
            order_type: 1,
            time_in_force: 0,
            reduce_only: false,
            post_only: true,
            price_e6: 97_500_000_000,
            size_e6: 100_000,
            filled_size_e6: 50_000,
            remaining_size_e6: 50_000,
            trigger_price_e6: 0,
            avg_fill_price_e6: 97_500_000_000,
            status: OrderStatus::PartialFill as u8,
            status_reason: StatusReason::None as u8,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = OrderEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_trade_event_borsh_roundtrip() {
        let event = TradeEvent {
            discriminator: event_discriminator::TRADE,
            sequence: 67890,
            timestamp: 1700000000,
            batch_id: 100,
            market_index: 0,
            market_type: 0,
            trade_type: TradeType::Normal as u8,
            maker: Pubkey::new_unique(),
            maker_order_id: [2u8; 16],
            maker_side: 1,
            maker_fee_e6: -500,
            taker: Pubkey::new_unique(),
            taker_order_id: [3u8; 16],
            taker_side: 0,
            taker_fee_e6: 1000,
            price_e6: 97_500_000_000,
            size_e6: 100_000,
            notional_e6: 9_750_000_000,
            maker_realized_pnl_e6: 0,
            taker_realized_pnl_e6: 0,
            maker_margin_delta_e6: -975_000_000,
            taker_margin_delta_e6: 975_000_000,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = TradeEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_liquidation_event_borsh_roundtrip() {
        let event = LiquidationEvent {
            discriminator: event_discriminator::LIQUIDATION,
            sequence: 99999,
            timestamp: 1700000000,
            user: Pubkey::new_unique(),
            market_index: 0,
            side: 0,
            position_size_e6: 1_000_000,
            entry_price_e6: 50_000_000_000,
            mark_price_e6: 45_000_000_000,
            liquidation_price_e6: 45_500_000_000,
            margin_e6: 5_000_000_000,
            margin_ratio_e6: 10_000,
            penalty_e6: 500_000_000,
            insurance_payout_e6: 0,
            remaining_margin_e6: 500_000_000,
            is_bankruptcy: false,
            realized_pnl_e6: -4_500_000_000,
            related_trade_sequence: 99998,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = LiquidationEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_batch_event_borsh_roundtrip() {
        let event = BatchEvent {
            discriminator: event_discriminator::BATCH,
            batch_id: 45678,
            timestamp: 1700000000,
            event_type: BatchStatus::Executed as u8,
            trade_count: 32,
            total_notional_e6: 1_200_000_000_000,
            relayer: Pubkey::new_unique(),
            data_hash: [0xAB; 32],
            chain_tx: [0xCD; 64],
            error_code: 0,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = BatchEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_deposit_withdraw_event_borsh_roundtrip() {
        let event = DepositWithdrawEvent {
            discriminator: event_discriminator::DEPOSIT_WITHDRAW,
            sequence: 11111,
            timestamp: 1700000000,
            user: Pubkey::new_unique(),
            event_type: DepositWithdrawType::BridgeDeposit as u8,
            token_index: 0,
            amount_e6: 10_000_000_000,
            balance_before_e6: 5_000_000_000,
            balance_after_e6: 15_000_000_000,
            source_chain: 2, // Ethereum
            source_tx_hash: [0xFF; 32],
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = DepositWithdrawEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_fee_event_borsh_roundtrip() {
        let event = FeeEvent {
            discriminator: event_discriminator::FEE,
            sequence: 22222,
            timestamp: 1700000000,
            user: Pubkey::new_unique(),
            market_index: 1,
            fee_type: FeeType::TakerFee as u8,
            amount_e6: 4_870_000,
            related_trade_sequence: 67890,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = FeeEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_insurance_fund_event_borsh_roundtrip() {
        let event = InsuranceFundEvent {
            discriminator: event_discriminator::INSURANCE_FUND,
            sequence: 33333,
            timestamp: 1700000000,
            event_type: InsuranceFundEventType::LiquidationIncome as u8,
            market_index: 0,
            amount_e6: 500_000_000,
            balance_before_e6: 100_000_000_000,
            balance_after_e6: 100_500_000_000,
            related_user: Pubkey::new_unique(),
            reason: 0,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = InsuranceFundEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_funding_event_borsh_roundtrip() {
        let event = FundingEvent {
            discriminator: event_discriminator::FUNDING,
            sequence: 44444,
            timestamp: 1700000000,
            user: Pubkey::new_unique(),
            market_index: 0,
            side: 0,
            position_size_e6: 1_000_000,
            funding_rate_e9: 100_000, // 0.0001 (0.01%)
            payment_e6: -12_500_000,
            mark_price_e6: 97_500_000_000,
            period_start: 1699996400,
            period_end: 1700000000,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = FundingEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_adl_event_borsh_roundtrip() {
        let event = ADLEvent {
            discriminator: event_discriminator::ADL,
            sequence: 55555,
            timestamp: 1700000000,
            market_index: 0,
            trigger_reason: ADLTriggerReason::Bankruptcy as u8,
            shortfall_e6: 1_000_000_000,
            insurance_balance_before_e6: 500_000_000,
            insurance_balance_after_e6: 0,
            bankrupt_user: Pubkey::new_unique(),
            bankrupt_side: 0,
            bankrupt_size_e6: 500_000,
            counterparty_user: Pubkey::new_unique(),
            counterparty_side: 1,
            counterparty_size_reduced_e6: 500_000,
            counterparty_pnl_e6: 2_000_000_000,
            related_trade_sequence: 55554,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = ADLEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_position_event_borsh_roundtrip() {
        let event = PositionEvent {
            discriminator: event_discriminator::POSITION,
            sequence: 66666,
            timestamp: 1700000000,
            user: Pubkey::new_unique(),
            market_index: 0,
            event_type: PositionEventType::Opened as u8,
            side_before: 0,
            size_before_e6: 0,
            entry_price_before_e6: 0,
            margin_before_e6: 0,
            side_after: 0,
            size_after_e6: 1_000_000,
            entry_price_after_e6: 97_500_000_000,
            margin_after_e6: 9_750_000_000,
            size_delta_e6: 1_000_000,
            realized_pnl_e6: 0,
            fee_e6: 4_870_000,
            related_trade_sequence: 67890,
        };

        let data = borsh::to_vec(&event).unwrap();
        let decoded = PositionEvent::try_from_slice(&data).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_all_discriminators_unique() {
        let discriminators = [
            event_discriminator::ORDER,
            event_discriminator::TRADE,
            event_discriminator::POSITION,
            event_discriminator::LIQUIDATION,
            event_discriminator::ADL,
            event_discriminator::FUNDING,
            event_discriminator::DEPOSIT_WITHDRAW,
            event_discriminator::FEE,
            event_discriminator::INSURANCE_FUND,
            event_discriminator::BATCH,
        ];

        // Ensure all discriminators are unique
        for i in 0..discriminators.len() {
            for j in (i + 1)..discriminators.len() {
                assert_ne!(
                    discriminators[i], discriminators[j],
                    "Discriminator collision at index {} and {}",
                    i, j
                );
            }
        }
    }
}
