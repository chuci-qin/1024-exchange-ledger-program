//! 1024 DEX Exchange Ledger Program
//!
//! 核心交易账本程序，负责:
//! - 仓位管理 (Position PDA)
//! - 多 Relayer 多签机制
//! - 开仓/平仓原子操作
//! - 清算和 ADL
//! - 资金费率结算
//! - 成交记录

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    pubkey::Pubkey,
};

pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;
pub mod utils;
pub mod cpi;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

/// 程序入口点
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    processor::process_instruction(program_id, accounts, instruction_data)
}

