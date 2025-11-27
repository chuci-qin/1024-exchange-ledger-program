//! CPI Helpers for calling Vault Program and Fund Program
//!
//! 架构说明:
//! - Vault Program: 处理用户资金 (保证金锁定/释放/结算)
//! - Fund Program: 处理系统资金 (清算罚金/穿仓覆盖/保险基金)

use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
    pubkey::Pubkey,
};
use borsh::BorshSerialize;

// =============================================================================
// Vault Program CPI
// =============================================================================

/// Vault Program 指令枚举
/// 必须与 Vault 程序中的 VaultInstruction 顺序完全一致！
#[derive(BorshSerialize)]
#[repr(u8)]
enum VaultInstruction {
    Initialize { _ledger: [u8; 32], _delegation: [u8; 32], _fund: [u8; 32] }, // 0
    InitializeUser,                                                           // 1
    Deposit { _amount: u64 },                                                 // 2
    Withdraw { _amount: u64 },                                                // 3
    LockMargin { amount: u64 },                                               // 4
    ReleaseMargin { amount: u64 },                                            // 5
    ClosePositionSettle {                                                     // 6
        margin_to_release: u64,
        realized_pnl: i64,
        fee: u64,
    },
    LiquidatePosition {                                                       // 7
        margin: u64,
        user_remainder: u64,
        liquidation_penalty: u64,
    },
    AddAuthorizedCaller { _caller: [u8; 32] },                                // 8
    RemoveAuthorizedCaller { _caller: [u8; 32] },                             // 9
    SetPaused { _paused: bool },                                              // 10
    UpdateAdmin { _new_admin: [u8; 32] },                                     // 11
    SetFundProgram { _fund_program: [u8; 32] },                               // 12
}

/// CPI: 锁定保证金 (Vault Program)
pub fn lock_margin<'a>(
    vault_program_id: &Pubkey,
    vault_config: AccountInfo<'a>,
    user_account: AccountInfo<'a>,
    caller_program: AccountInfo<'a>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *vault_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*vault_config.key, false),
            AccountMeta::new(*user_account.key, false),
            AccountMeta::new_readonly(*caller_program.key, false),
        ],
        data: VaultInstruction::LockMargin { amount }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[vault_config, user_account, caller_program],
        signer_seeds,
    )
}

/// CPI: 释放保证金 (Vault Program)
pub fn release_margin<'a>(
    vault_program_id: &Pubkey,
    vault_config: AccountInfo<'a>,
    user_account: AccountInfo<'a>,
    caller_program: AccountInfo<'a>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *vault_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*vault_config.key, false),
            AccountMeta::new(*user_account.key, false),
            AccountMeta::new_readonly(*caller_program.key, false),
        ],
        data: VaultInstruction::ReleaseMargin { amount }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[vault_config, user_account, caller_program],
        signer_seeds,
    )
}

/// CPI: 平仓结算 (Vault Program - 仅用户账户部分)
///
/// 注意: 手续费分配由单独调用 Fund Program 处理
pub fn close_position_settle<'a>(
    vault_program_id: &Pubkey,
    vault_config: AccountInfo<'a>,
    user_account: AccountInfo<'a>,
    caller_program: AccountInfo<'a>,
    margin_to_release: u64,
    realized_pnl: i64,
    fee: u64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *vault_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*vault_config.key, false),
            AccountMeta::new(*user_account.key, false),
            AccountMeta::new_readonly(*caller_program.key, false),
        ],
        data: VaultInstruction::ClosePositionSettle {
            margin_to_release,
            realized_pnl,
            fee,
        }
        .try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[vault_config, user_account, caller_program],
        signer_seeds,
    )
}

/// CPI: 清算用户账户 (Vault Program)
///
/// 执行完整的清算资金处理:
/// 1. 更新用户账户状态 (清空保证金, 返还剩余)
/// 2. 将清算罚金从 Vault Token Account 转入 Insurance Fund Vault
pub fn liquidate_position<'a>(
    vault_program_id: &Pubkey,
    vault_config: AccountInfo<'a>,
    user_account: AccountInfo<'a>,
    caller_program: AccountInfo<'a>,
    vault_token_account: AccountInfo<'a>,
    insurance_fund_vault: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
    margin: u64,
    user_remainder: u64,
    liquidation_penalty: u64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *vault_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*vault_config.key, false),
            AccountMeta::new(*user_account.key, false),
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*vault_token_account.key, false),
            AccountMeta::new(*insurance_fund_vault.key, false),
            AccountMeta::new_readonly(*token_program.key, false),
        ],
        data: VaultInstruction::LiquidatePosition {
            margin,
            user_remainder,
            liquidation_penalty,
        }
        .try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[
            vault_config,
            user_account,
            caller_program,
            vault_token_account,
            insurance_fund_vault,
            token_program,
        ],
        signer_seeds,
    )
}

// =============================================================================
// Fund Program CPI (Insurance Fund Operations)
// =============================================================================

/// Fund Program 指令枚举 (仅保险基金相关)
/// 必须与 Fund Program 中的 FundInstruction 顺序完全一致！
#[derive(BorshSerialize)]
#[repr(u8)]
enum FundInstruction {
    // 跳过前面的指令 (0-69)...使用占位
    _Placeholder0,  // 0 - Initialize
    _Placeholder1,  // 1 - CreateFund
    _Placeholder2,  // 2 - UpdateFund
    _Placeholder3,  // 3 - SetFundOpen
    _Placeholder4,  // 4 - SetFundPaused
    _Placeholder5,  // 5 - CloseFund
    _Placeholder6,  // 6 - DepositToFund
    _Placeholder7,  // 7 - RedeemFromFund
    _Placeholder8,  // 8 - TradeFund
    _Placeholder9,  // 9 - CloseFundPosition
    _Placeholder10, // 10 - CollectFees
    _Placeholder11, // 11 - UpdateAuthority
    _Placeholder12, // 12 - SetProgramPaused
    _Placeholder13, // 13 - UpdateNAV
    _Placeholder14, // 14 - RecordPnL
    
    // Insurance Fund Operations (15-22)
    InitializeInsuranceFund { adl_trigger_threshold_e6: i64, withdrawal_delay_secs: i64, authorized_caller: [u8; 32] }, // 15
    AddLiquidationIncome { amount_e6: i64 },      // 16
    AddADLProfit { amount_e6: i64 },              // 17
    CoverShortfall { shortfall_e6: i64 },         // 18
    UpdateHourlySnapshot,                          // 19
    SetADLInProgress { in_progress: bool },       // 20
    CheckADLTrigger { shortfall_e6: i64 },        // 21
    AddTradingFee { fee_e6: i64 },                // 22 - V1 简化: 手续费直接转入保险基金
}

/// CPI: 添加清算收入到保险基金 (Fund Program)
///
/// 当发生清算时，清算罚金应转入保险基金
pub fn add_liquidation_income<'a>(
    fund_program_id: &Pubkey,
    caller_program: AccountInfo<'a>,
    fund_account: AccountInfo<'a>,
    insurance_config: AccountInfo<'a>,
    amount_e6: i64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *fund_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*fund_account.key, false),
            AccountMeta::new(*insurance_config.key, false),
        ],
        data: FundInstruction::AddLiquidationIncome { amount_e6 }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[caller_program, fund_account, insurance_config],
        signer_seeds,
    )
}

/// CPI: 添加ADL盈余到保险基金 (Fund Program)
///
/// 当ADL执行后有盈余时，转入保险基金
pub fn add_adl_profit<'a>(
    fund_program_id: &Pubkey,
    caller_program: AccountInfo<'a>,
    fund_account: AccountInfo<'a>,
    insurance_config: AccountInfo<'a>,
    amount_e6: i64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *fund_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*fund_account.key, false),
            AccountMeta::new(*insurance_config.key, false),
        ],
        data: FundInstruction::AddADLProfit { amount_e6 }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[caller_program, fund_account, insurance_config],
        signer_seeds,
    )
}

/// CPI: 覆盖穿仓损失 (Fund Program)
///
/// 当用户穿仓时，从保险基金覆盖损失
/// 如果保险基金不足，需要触发 ADL
pub fn cover_shortfall<'a>(
    fund_program_id: &Pubkey,
    caller_program: AccountInfo<'a>,
    fund_account: AccountInfo<'a>,
    insurance_config: AccountInfo<'a>,
    fund_vault: AccountInfo<'a>,
    destination: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
    shortfall_e6: i64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *fund_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*fund_account.key, false),
            AccountMeta::new(*insurance_config.key, false),
            AccountMeta::new(*fund_vault.key, false),
            AccountMeta::new(*destination.key, false),
            AccountMeta::new_readonly(*token_program.key, false),
        ],
        data: FundInstruction::CoverShortfall { shortfall_e6 }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[caller_program, fund_account, insurance_config, fund_vault, destination, token_program],
        signer_seeds,
    )
}

/// CPI: 设置ADL进行中状态 (Fund Program)
///
/// ADL期间暂停LP赎回
pub fn set_adl_in_progress<'a>(
    fund_program_id: &Pubkey,
    caller_program: AccountInfo<'a>,
    insurance_config: AccountInfo<'a>,
    in_progress: bool,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *fund_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*insurance_config.key, false),
        ],
        data: FundInstruction::SetADLInProgress { in_progress }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[caller_program, insurance_config],
        signer_seeds,
    )
}

/// CPI: 添加交易手续费到保险基金 (Fund Program)
///
/// V1 简化方案: 交易手续费直接转入保险基金
/// 
/// 在 ClosePosition 执行后调用，将手续费从 Vault 转入 Insurance Fund
pub fn add_trading_fee<'a>(
    fund_program_id: &Pubkey,
    caller_program: AccountInfo<'a>,
    fund_account: AccountInfo<'a>,
    insurance_config: AccountInfo<'a>,
    vault_token_account: AccountInfo<'a>,
    insurance_fund_vault: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
    fee_e6: i64,
    signer_seeds: &[&[&[u8]]],
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *fund_program_id,
        accounts: vec![
            AccountMeta::new_readonly(*caller_program.key, false),
            AccountMeta::new(*fund_account.key, false),
            AccountMeta::new(*insurance_config.key, false),
            AccountMeta::new(*vault_token_account.key, false),
            AccountMeta::new(*insurance_fund_vault.key, false),
            AccountMeta::new_readonly(*token_program.key, false),
        ],
        data: FundInstruction::AddTradingFee { fee_e6 }.try_to_vec()?,
    };

    invoke_signed(
        &instruction,
        &[
            caller_program,
            fund_account,
            insurance_config,
            vault_token_account,
            insurance_fund_vault,
            token_program,
        ],
        signer_seeds,
    )
}

// =============================================================================
// Helper Functions
// =============================================================================

/// 计算开仓所需保证金
/// margin = size * price / leverage / 1e6
pub fn calculate_required_margin(size_e6: u64, price_e6: u64, leverage: u8) -> Result<u64, crate::error::LedgerError> {
    if leverage == 0 {
        return Err(crate::error::LedgerError::InvalidLeverage);
    }

    let notional = (size_e6 as u128)
        .checked_mul(price_e6 as u128)
        .ok_or(crate::error::LedgerError::Overflow)?;

    let margin = notional
        .checked_div(leverage as u128)
        .ok_or(crate::error::LedgerError::Overflow)?;

    let margin = margin
        .checked_div(1_000_000)
        .ok_or(crate::error::LedgerError::Overflow)?;

    Ok(margin as u64)
}

/// 计算手续费
/// fee = size * price * fee_rate / 1e12
pub fn calculate_fee(size_e6: u64, price_e6: u64, fee_rate_e6: u64) -> Result<u64, crate::error::LedgerError> {
    let notional = (size_e6 as u128)
        .checked_mul(price_e6 as u128)
        .ok_or(crate::error::LedgerError::Overflow)?;

    let fee = notional
        .checked_mul(fee_rate_e6 as u128)
        .ok_or(crate::error::LedgerError::Overflow)?;

    // 除以 1e12 (1e6 * 1e6)
    let fee = fee
        .checked_div(1_000_000_000_000)
        .ok_or(crate::error::LedgerError::Overflow)?;

    Ok(fee as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_required_margin() {
        // 1 BTC at $50,000 with 10x leverage = $5,000 margin
        let size_e6 = 1_000_000; // 1 BTC
        let price_e6 = 50_000_000_000u64; // $50,000
        let leverage = 10u8;

        let margin = calculate_required_margin(size_e6, price_e6, leverage).unwrap();
        assert_eq!(margin, 5_000_000_000); // $5,000 in e6
    }

    #[test]
    fn test_calculate_fee() {
        // 1 BTC at $50,000 with 0.1% fee = $50 fee
        let size_e6 = 1_000_000; // 1 BTC
        let price_e6 = 50_000_000_000u64; // $50,000
        let fee_rate_e6 = 1_000; // 0.1% = 0.001 * 1e6

        let fee = calculate_fee(size_e6, price_e6, fee_rate_e6).unwrap();
        assert_eq!(fee, 50_000_000); // $50 in e6
    }
}
