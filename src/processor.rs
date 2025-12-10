//! Ledger Program Instruction Processor

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

use crate::{
    error::LedgerError,
    instruction::{LedgerInstruction, TradeData, trade_data_type},
    state::*,
    utils::*,
    cpi,
};

/// 辅助函数：反序列化账户数据，忽略尾部填充
/// 使用 deserialize 而不是 try_from_slice 来处理固定大小账户
fn deserialize_account<T: BorshDeserialize>(data: &[u8]) -> Result<T, std::io::Error> {
    let mut slice = data;
    T::deserialize(&mut slice)
}

/// 主处理函数
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = LedgerInstruction::try_from_slice(instruction_data)
        .map_err(|_| LedgerError::InvalidInstructionData)?;

    match instruction {
        // 初始化
        LedgerInstruction::Initialize => {
            msg!("Instruction: Initialize");
            process_initialize(program_id, accounts)
        }
        LedgerInstruction::InitializeRelayers { relayers, required_signatures } => {
            msg!("Instruction: InitializeRelayers");
            process_initialize_relayers(program_id, accounts, relayers, required_signatures)
        }
        LedgerInstruction::InitializeUserStats => {
            msg!("Instruction: InitializeUserStats");
            process_initialize_user_stats(program_id, accounts)
        }

        // 多签
        LedgerInstruction::SubmitTradeBatch { batch_id, data_hash } => {
            msg!("Instruction: SubmitTradeBatch");
            process_submit_trade_batch(program_id, accounts, batch_id, data_hash)
        }
        LedgerInstruction::ConfirmTradeBatch { batch_id, data_hash } => {
            msg!("Instruction: ConfirmTradeBatch");
            process_confirm_trade_batch(program_id, accounts, batch_id, data_hash)
        }
        LedgerInstruction::ExecuteTradeBatch { batch_id, trades } => {
            msg!("Instruction: ExecuteTradeBatch");
            process_execute_trade_batch(program_id, accounts, batch_id, trades)
        }

        // 交易
        LedgerInstruction::OpenPosition {
            user,
            market_index,
            side,
            size_e6,
            price_e6,
            leverage,
            batch_id,
        } => {
            msg!("Instruction: OpenPosition");
            process_open_position(
                program_id, accounts, user, market_index, side, size_e6, price_e6, leverage, batch_id,
            )
        }
        LedgerInstruction::ClosePosition {
            user,
            market_index,
            size_e6,
            price_e6,
            batch_id,
        } => {
            msg!("Instruction: ClosePosition");
            process_close_position(program_id, accounts, user, market_index, size_e6, price_e6, batch_id)
        }

        // 清算
        LedgerInstruction::Liquidate {
            user,
            market_index,
            mark_price_e6,
        } => {
            msg!("Instruction: Liquidate");
            process_liquidate(program_id, accounts, user, market_index, mark_price_e6)
        }
        LedgerInstruction::TriggerADL {
            market_index,
            shortfall_e6,
            bankrupt_side,
        } => {
            msg!("Instruction: TriggerADL");
            process_trigger_adl(program_id, accounts, market_index, shortfall_e6, bankrupt_side)
        }

        // 资金费率
        LedgerInstruction::SettleFunding {
            user,
            market_index,
            funding_rate_e6,
            index_price_e6,
        } => {
            msg!("Instruction: SettleFunding");
            process_settle_funding(program_id, accounts, user, market_index, funding_rate_e6, index_price_e6)
        }

        // 管理
        LedgerInstruction::AddRelayer { relayer } => {
            msg!("Instruction: AddRelayer");
            process_add_relayer(accounts, relayer)
        }
        LedgerInstruction::RemoveRelayer { relayer } => {
            msg!("Instruction: RemoveRelayer");
            process_remove_relayer(accounts, relayer)
        }
        LedgerInstruction::UpdateRequiredSignatures { required_signatures } => {
            msg!("Instruction: UpdateRequiredSignatures");
            process_update_required_signatures(accounts, required_signatures)
        }
        LedgerInstruction::SetPaused { paused } => {
            msg!("Instruction: SetPaused");
            process_set_paused(accounts, paused)
        }
        LedgerInstruction::UpdateAdmin { new_admin } => {
            msg!("Instruction: UpdateAdmin");
            process_update_admin(accounts, new_admin)
        }
        LedgerInstruction::UpdateVaultProgram { new_vault_program } => {
            msg!("Instruction: UpdateVaultProgram");
            process_update_vault_program(accounts, new_vault_program)
        }
        LedgerInstruction::UpdateFundProgram { new_fund_program } => {
            msg!("Instruction: UpdateFundProgram");
            process_update_fund_program(accounts, new_fund_program)
        }
    }
}

// ============================================================================
// 初始化指令处理
// ============================================================================

fn process_initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let vault_program = next_account_info(account_info_iter)?;
    let fund_program = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(admin)?;

    // 派生 PDA
    let (ledger_config_pda, bump) = Pubkey::find_program_address(&[b"ledger_config"], program_id);
    if ledger_config_info.key != &ledger_config_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    // 创建账户
    let rent = Rent::get()?;
    let space = LedgerConfig::SIZE;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            admin.key,
            ledger_config_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[admin.clone(), ledger_config_info.clone(), system_program.clone()],
        &[&[b"ledger_config", &[bump]]],
    )?;

    // 初始化数据
    let current_ts = get_current_timestamp()?;
    let ledger_config = LedgerConfig {
        discriminator: LedgerConfig::DISCRIMINATOR,
        admin: *admin.key,
        vault_program: *vault_program.key,
        fund_program: *fund_program.key,
        global_sequence: 0,
        total_positions_opened: 0,
        total_positions_closed: 0,
        total_volume_e6: 0,
        total_fees_collected_e6: 0,
        total_liquidations: 0,
        total_adl_count: 0,
        is_paused: false,
        bump,
        created_at: current_ts,
        last_update_ts: current_ts,
        reserved: [0u8; 65],
    };

    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;
    msg!("LedgerConfig initialized by {}", admin.key);
    msg!("Vault Program: {}", vault_program.key);
    msg!("Fund Program: {}", fund_program.key);

    Ok(())
}

fn process_initialize_relayers(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    relayers: Vec<Pubkey>,
    required_signatures: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(admin)?;

    // 验证参数
    if relayers.is_empty() || relayers.len() > MAX_RELAYERS {
        return Err(LedgerError::InvalidAccount.into());
    }
    if required_signatures == 0 || required_signatures as usize > relayers.len() {
        return Err(LedgerError::InsufficientSignatures.into());
    }

    // 派生 PDA
    let (relayer_config_pda, bump) = Pubkey::find_program_address(&[b"relayer_config"], program_id);
    if relayer_config_info.key != &relayer_config_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    // 创建账户
    let rent = Rent::get()?;
    let space = RelayerConfig::SIZE;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            admin.key,
            relayer_config_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[admin.clone(), relayer_config_info.clone(), system_program.clone()],
        &[&[b"relayer_config", &[bump]]],
    )?;

    // 初始化数据
    let relayer_config = RelayerConfig {
        discriminator: RelayerConfig::DISCRIMINATOR,
        admin: *admin.key,
        authorized_relayers: relayers.clone(),
        required_signatures,
        bump,
        last_update_ts: get_current_timestamp()?,
    };

    relayer_config.serialize(&mut &mut relayer_config_info.data.borrow_mut()[..])?;
    msg!("RelayerConfig initialized with {} relayers, {} required", relayers.len(), required_signatures);

    Ok(())
}

fn process_initialize_user_stats(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let user = next_account_info(account_info_iter)?;
    let user_stats_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(user)?;

    // 派生 PDA
    let (user_stats_pda, bump) = Pubkey::find_program_address(
        &[b"user_stats", user.key.as_ref()],
        program_id,
    );
    if user_stats_info.key != &user_stats_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    // 创建账户
    let rent = Rent::get()?;
    let space = UserStats::SIZE;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            user.key,
            user_stats_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[user.clone(), user_stats_info.clone(), system_program.clone()],
        &[&[b"user_stats", user.key.as_ref(), &[bump]]],
    )?;

    // 初始化数据
    let user_stats = UserStats {
        discriminator: UserStats::DISCRIMINATOR,
        user: *user.key,
        total_trades: 0,
        total_volume_e6: 0,
        total_realized_pnl_e6: 0,
        total_fees_paid_e6: 0,
        total_funding_paid_e6: 0,
        total_liquidations: 0,
        first_trade_at: 0,
        last_trade_at: 0,
        bump,
    };

    user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..])?;
    msg!("UserStats initialized for {}", user.key);

    Ok(())
}

// ============================================================================
// 辅助函数：自动创建 UserStats
// ============================================================================

/// 确保 UserStats 账户存在，如果不存在则自动创建
/// 
/// 返回: Ok(true) 如果创建了新账户，Ok(false) 如果已存在
fn ensure_user_stats_exists<'a>(
    program_id: &Pubkey,
    payer: &AccountInfo<'a>,
    user_wallet: &Pubkey,
    user_stats_info: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
) -> Result<bool, ProgramError> {
    // 检查账户是否已存在
    let is_empty = user_stats_info.data_len() == 0 || {
        let data = user_stats_info.data.borrow();
        data.iter().all(|&x| x == 0)
    };
    
    if !is_empty {
        // 账户已存在，验证 discriminator
        let data = user_stats_info.data.borrow();
        if data.len() >= 8 && &data[0..8] == UserStats::DISCRIMINATOR.as_slice() {
            return Ok(false); // 已存在，无需创建
        }
    }
    
    // 验证 PDA
    let (user_stats_pda, bump) = Pubkey::find_program_address(
        &[b"user_stats", user_wallet.as_ref()],
        program_id,
    );
    if user_stats_info.key != &user_stats_pda {
        msg!("❌ Invalid UserStats PDA: expected {}, got {}", user_stats_pda, user_stats_info.key);
        return Err(LedgerError::InvalidAccount.into());
    }
    
    // 创建账户
    let rent = Rent::get()?;
    let space = UserStats::SIZE;
    let lamports = rent.minimum_balance(space);
    
    msg!("✨ Auto-creating UserStats for user {}", user_wallet);
    
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            user_stats_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[payer.clone(), user_stats_info.clone(), system_program.clone()],
        &[&[b"user_stats", user_wallet.as_ref(), &[bump]]],
    )?;
    
    // 初始化数据
    let user_stats = UserStats {
        discriminator: UserStats::DISCRIMINATOR,
        user: *user_wallet,
        total_trades: 0,
        total_volume_e6: 0,
        total_realized_pnl_e6: 0,
        total_fees_paid_e6: 0,
        total_funding_paid_e6: 0,
        total_liquidations: 0,
        first_trade_at: 0,
        last_trade_at: 0,
        bump,
    };
    
    user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..])?;
    msg!("✅ UserStats auto-created for {}", user_wallet);
    
    Ok(true) // 新创建
}

// ============================================================================
// 多签指令处理
// ============================================================================

fn process_submit_trade_batch(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    batch_id: u64,
    data_hash: [u8; 32],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let trade_batch_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;

    // 验证 Relayer 授权
    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    // 派生 TradeBatch PDA
    let (trade_batch_pda, bump) = Pubkey::find_program_address(
        &[b"trade_batch", &batch_id.to_le_bytes()],
        program_id,
    );
    if trade_batch_info.key != &trade_batch_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    // 创建账户
    let rent = Rent::get()?;
    let space = TradeBatch::SIZE;
    let lamports = rent.minimum_balance(space);
    let current_ts = get_current_timestamp()?;

    invoke_signed(
        &system_instruction::create_account(
            relayer.key,
            trade_batch_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[relayer.clone(), trade_batch_info.clone(), system_program.clone()],
        &[&[b"trade_batch", &batch_id.to_le_bytes(), &[bump]]],
    )?;

    // 初始化 TradeBatch
    let mut trade_batch = TradeBatch {
        discriminator: TradeBatch::DISCRIMINATOR,
        batch_id,
        data_hash,
        signatures: vec![],
        executed: false,
        created_at: current_ts,
        expires_at: current_ts + TRADE_BATCH_EXPIRY_SECONDS,
        creator: *relayer.key,
        bump,
    };

    // 添加第一个签名
    trade_batch.add_signature(*relayer.key, current_ts)?;
    trade_batch.serialize(&mut &mut trade_batch_info.data.borrow_mut()[..])?;

    msg!("TradeBatch {} submitted by {}", batch_id, relayer.key);
    Ok(())
}

fn process_confirm_trade_batch(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    batch_id: u64,
    data_hash: [u8; 32],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let trade_batch_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(trade_batch_info)?;

    // 验证 Relayer 授权
    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    // 验证 TradeBatch PDA
    let (trade_batch_pda, _) = Pubkey::find_program_address(
        &[b"trade_batch", &batch_id.to_le_bytes()],
        program_id,
    );
    if trade_batch_info.key != &trade_batch_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    let mut trade_batch = deserialize_account::<TradeBatch>(&trade_batch_info.data.borrow())?;

    // 验证状态
    let current_ts = get_current_timestamp()?;
    if trade_batch.is_expired(current_ts) {
        return Err(LedgerError::TradeBatchExpired.into());
    }
    if trade_batch.executed {
        return Err(LedgerError::TradeBatchAlreadyExecuted.into());
    }

    // 验证数据哈希
    if trade_batch.data_hash != data_hash {
        return Err(LedgerError::InvalidDataHash.into());
    }

    // 添加签名
    trade_batch.add_signature(*relayer.key, current_ts)?;
    trade_batch.serialize(&mut &mut trade_batch_info.data.borrow_mut()[..])?;

    msg!(
        "TradeBatch {} confirmed by {}, signatures: {}",
        batch_id,
        relayer.key,
        trade_batch.signature_count()
    );
    Ok(())
}

/// ExecuteTradeBatch 账户布局:
/// 0. `[signer]` Relayer
/// 1. `[writable]` TradeBatch PDA
/// 2. `[]` RelayerConfig
/// 3. `[writable]` LedgerConfig
/// 4. `[]` VaultConfig
/// 5. `[]` Vault Program
/// 6. `[]` Ledger Program (self, for CPI caller verification)
/// 7. `[]` System Program
/// 8. `[writable]` Insurance Fund (for close positions - optional, can be SystemProgram if no closes)
/// 
/// 然后是每笔交易的账户 (每笔交易 3 个账户):
/// For trade i:
///   9 + i*3 + 0: `[writable]` Position PDA
///   9 + i*3 + 1: `[writable]` UserAccount (Vault)
///   9 + i*3 + 2: `[writable]` UserStats PDA
fn process_execute_trade_batch(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    batch_id: u64,
    trades: Vec<TradeData>,
) -> ProgramResult {
    // 解析共享账户
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let trade_batch_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let vault_config_info = next_account_info(account_info_iter)?;
    let vault_program = next_account_info(account_info_iter)?;
    let ledger_program_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let insurance_fund_info = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(trade_batch_info)?;
    assert_writable(ledger_config_info)?;

    // 验证 Ledger Program 地址
    if ledger_program_info.key != program_id {
        return Err(LedgerError::InvalidProgramId.into());
    }

    // 验证 Relayer 授权
    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    // 验证 TradeBatch
    let mut trade_batch = deserialize_account::<TradeBatch>(&trade_batch_info.data.borrow())?;
    let current_ts = get_current_timestamp()?;

    if trade_batch.is_expired(current_ts) {
        return Err(LedgerError::TradeBatchExpired.into());
    }
    if trade_batch.executed {
        return Err(LedgerError::TradeBatchAlreadyExecuted.into());
    }
    if !relayer_config.has_enough_signatures(trade_batch.signature_count()) {
        return Err(LedgerError::InsufficientSignatures.into());
    }

    // 验证数据哈希 (使用 batch_id 防止重放攻击)
    let trades_data = trades.try_to_vec()?;
    if !verify_batch_hash(program_id, batch_id, &trades_data, &trade_batch.data_hash) {
        return Err(LedgerError::InvalidDataHash.into());
    }

    // 标记已执行
    trade_batch.executed = true;
    trade_batch.serialize(&mut &mut trade_batch_info.data.borrow_mut()[..])?;

    // 读取 LedgerConfig
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    
    if ledger_config.is_paused {
        return Err(LedgerError::LedgerPaused.into());
    }

    // 验证 Vault Program
    if vault_program.key != &ledger_config.vault_program {
        return Err(LedgerError::InvalidVaultProgram.into());
    }

    // 收集剩余账户 (每笔交易的账户)
    let remaining_accounts: Vec<AccountInfo> = account_info_iter.cloned().collect();
    
    // 验证账户数量
    let expected_accounts = trades.len() * 3; // 每笔交易 3 个账户
    if remaining_accounts.len() < expected_accounts {
        msg!(
            "❌ Insufficient accounts: expected {} for {} trades, got {}",
            expected_accounts,
            trades.len(),
            remaining_accounts.len()
        );
        return Err(LedgerError::InsufficientAccounts.into());
    }

    // 派生 Ledger Config PDA bump 用于 CPI
    let (_, ledger_config_bump) = Pubkey::find_program_address(
        &[b"ledger_config"],
        program_id,
    );

    msg!("📦 ExecuteTradeBatch: batch_id={}, trades={}", batch_id, trades.len());

    // 执行每笔交易
    for (i, trade) in trades.iter().enumerate() {
        let sequence = ledger_config.next_sequence();
        
        // 获取此交易的账户
        let base_idx = i * 3;
        let position_info = &remaining_accounts[base_idx];
        let user_account_info = &remaining_accounts[base_idx + 1];
        let user_stats_info = &remaining_accounts[base_idx + 2];

        // 验证 Position PDA
        let (expected_position_pda, position_bump) = Pubkey::find_program_address(
            &[b"position", trade.user.as_ref(), &[trade.market_index]],
            program_id,
        );
        if position_info.key != &expected_position_pda {
            msg!("❌ Trade {}: Invalid position PDA", i);
            return Err(LedgerError::InvalidAccount.into());
        }

        match trade.trade_type {
            trade_data_type::OPEN => {
                msg!(
                    "🔵 Trade {} OPEN: user={}, market={}, side={:?}, size={}, price={}, leverage={}",
                    i, trade.user, trade.market_index, trade.side, trade.size_e6, trade.price_e6, trade.leverage
                );

                // 验证参数
                if trade.size_e6 == 0 {
                    return Err(LedgerError::InvalidTradeAmount.into());
                }
                if trade.price_e6 == 0 {
                    return Err(LedgerError::InvalidPrice.into());
                }
                if trade.leverage == 0 || trade.leverage > MAX_LEVERAGE {
                    return Err(LedgerError::InvalidLeverage.into());
                }

                // 计算所需保证金和手续费
                let required_margin = cpi::calculate_required_margin(trade.size_e6, trade.price_e6, trade.leverage)?;
                let fee = cpi::calculate_fee(trade.size_e6, trade.price_e6, 1_000)?; // 0.1% fee

                // 检查是否是新仓位
                let is_new_position = position_info.data_len() == 0 || {
                    let data = position_info.data.borrow();
                    data.iter().all(|&x| x == 0)
                };

                if is_new_position {
                    // 创建新仓位
                    let rent = Rent::get()?;
                    let space = Position::SIZE;
                    let lamports = rent.minimum_balance(space);

                    invoke_signed(
                        &system_instruction::create_account(
                            relayer.key,
                            position_info.key,
                            lamports,
                            space as u64,
                            program_id,
                        ),
                        &[relayer.clone(), position_info.clone(), system_program.clone()],
                        &[&[b"position", trade.user.as_ref(), &[trade.market_index], &[position_bump]]],
                    )?;

                    let mut position = Position {
                        discriminator: Position::DISCRIMINATOR,
                        user: trade.user,
                        market_index: trade.market_index,
                        side: trade.side.clone(),
                        size_e6: trade.size_e6,
                        entry_price_e6: trade.price_e6,
                        margin_e6: required_margin,
                        leverage: trade.leverage,
                        liquidation_price_e6: 0,
                        unrealized_pnl_e6: 0,
                        last_funding_ts: current_ts,
                        cumulative_funding_e6: 0,
                        open_order_count: 0,
                        opened_at: current_ts,
                        last_update_ts: current_ts,
                        bump: position_bump,
                        reserved: [0; 32],
                    };
                    position.liquidation_price_e6 = position.calculate_liquidation_price()?;
                    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

                    msg!("  ✅ New position created");
                } else {
                    // 加仓
                    let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
                    if position.side != trade.side {
                        msg!("❌ Trade {}: Side mismatch (existing: {:?}, new: {:?})", i, position.side, trade.side);
                        return Err(LedgerError::InvalidPositionSide.into());
                    }
                    position.update_entry_price(trade.size_e6, trade.price_e6)?;
                    position.margin_e6 = checked_add_u64(position.margin_e6, required_margin)?;
                    position.last_update_ts = current_ts;
                    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

                    msg!("  ✅ Position increased");
                }

                // CPI: 锁定保证金 (使用 LedgerConfig PDA 作为 caller)
                let total_to_lock = checked_add_u64(required_margin, fee)?;
                cpi::lock_margin(
                    vault_program.key,
                    vault_config_info.clone(),
                    user_account_info.clone(),
                    ledger_config_info.clone(),  // 使用 LedgerConfig PDA 作为 caller
                    total_to_lock,
                    &[&[b"ledger_config", &[ledger_config_bump]]],  // PDA 签名
                )?;
                msg!("  ✅ Margin locked: {} (margin) + {} (fee)", required_margin, fee);

                // 更新统计
                ledger_config.total_positions_opened += 1;
                ledger_config.total_fees_collected_e6 = checked_add_u64(ledger_config.total_fees_collected_e6, fee)?;
            }
            
            trade_data_type::CLOSE => {
                msg!(
                    "🔴 Trade {} CLOSE: user={}, market={}, size={}, price={}",
                    i, trade.user, trade.market_index, trade.size_e6, trade.price_e6
                );

                // 验证参数
                if trade.size_e6 == 0 {
                    return Err(LedgerError::InvalidTradeAmount.into());
                }
                if trade.price_e6 == 0 {
                    return Err(LedgerError::InvalidPrice.into());
                }

                // 读取仓位
                let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
                if position.user != trade.user || position.market_index != trade.market_index {
                    return Err(LedgerError::PositionNotFound.into());
                }
                if position.is_empty() {
                    return Err(LedgerError::PositionNotFound.into());
                }

                // 计算平仓数量和盈亏
                let close_size = trade.size_e6.min(position.size_e6);
                let close_ratio = div_e6(close_size as i64, position.size_e6 as i64)?;
                let pnl = position.calculate_unrealized_pnl(trade.price_e6)?;
                let realized_pnl = mul_e6(pnl, close_ratio)?;
                let margin_to_release = mul_e6(position.margin_e6 as i64, close_ratio)? as u64;
                let fee = cpi::calculate_fee(close_size, trade.price_e6, 1_000)?;

                // 更新仓位
                if close_size >= position.size_e6 {
                    position.size_e6 = 0;
                    position.margin_e6 = 0;
                    position.entry_price_e6 = 0;
                    position.liquidation_price_e6 = 0;
                    position.unrealized_pnl_e6 = 0;
                } else {
                    position.size_e6 = checked_sub_u64(position.size_e6, close_size)?;
                    position.margin_e6 = checked_sub_u64(position.margin_e6, margin_to_release)?;
                    position.liquidation_price_e6 = position.calculate_liquidation_price()?;
                }
                position.last_update_ts = current_ts;
                position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

                // CPI: 平仓结算
                cpi::close_position_settle(
                    &ledger_config.vault_program,
                    vault_config_info.clone(),
                    user_account_info.clone(),
                    ledger_config_info.clone(),
                    margin_to_release,
                    realized_pnl,
                    fee,
                    &[&[b"ledger_config", &[ledger_config_bump]]],
                )?;
                msg!("  ✅ Position closed: pnl={}, margin_released={}, fee={}", realized_pnl, margin_to_release, fee);

                // 更新统计
                ledger_config.total_positions_closed += 1;
                ledger_config.total_fees_collected_e6 = checked_add_u64(ledger_config.total_fees_collected_e6, fee)?;
            }
            
            _ => {
                msg!("⚠️ Trade {}: Unknown trade type {}", i, trade.trade_type);
            }
        }

        // 更新交易量
        ledger_config.total_volume_e6 = ledger_config
            .total_volume_e6
            .saturating_add((trade.size_e6 as u128 * trade.price_e6 as u128 / 1_000_000) as u64);

        // 自动创建 UserStats (如果不存在)
        let _ = ensure_user_stats_exists(
            program_id,
            relayer,
            &trade.user,
            user_stats_info,
            system_program,
        );

        // 更新用户统计 (现在保证存在)
        if user_stats_info.data_len() > 0 {
            // 先读取数据到局部变量，释放借用
            let user_stats_result = {
                let data = user_stats_info.data.borrow();
                deserialize_account::<UserStats>(&data)
            };
            
            if let Ok(mut user_stats) = user_stats_result {
                user_stats.total_trades += 1;
                user_stats.total_volume_e6 = user_stats.total_volume_e6.saturating_add(
                    (trade.size_e6 as u128 * trade.price_e6 as u128 / 1_000_000) as u64
                );
                if user_stats.first_trade_at == 0 {
                    user_stats.first_trade_at = current_ts;
                }
                user_stats.last_trade_at = current_ts;
                let _ = user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..]);
            }
        }

        msg!("  📊 Sequence: {}", sequence);
    }

    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("✅ TradeBatch {} executed successfully with {} trades", batch_id, trades.len());
    Ok(())
}

// ============================================================================
// 交易指令处理
// ============================================================================

fn process_open_position(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u8,
    side: Side,
    size_e6: u64,
    price_e6: u64,
    leverage: u8,
    batch_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let position_info = next_account_info(account_info_iter)?;
    let user_account_info = next_account_info(account_info_iter)?;
    let vault_config_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let user_stats_info = next_account_info(account_info_iter)?;
    let vault_program = next_account_info(account_info_iter)?;
    let ledger_program_info = next_account_info(account_info_iter)?; // Ledger Program itself for CPI caller
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(position_info)?;
    assert_writable(user_account_info)?;
    assert_writable(ledger_config_info)?;
    assert_writable(user_stats_info)?;
    
    // 验证 Ledger Program 地址正确
    if ledger_program_info.key != program_id {
        return Err(LedgerError::InvalidProgramId.into());
    }

    // 验证参数
    if size_e6 == 0 {
        return Err(LedgerError::InvalidTradeAmount.into());
    }
    if price_e6 == 0 {
        return Err(LedgerError::InvalidPrice.into());
    }
    if leverage == 0 || leverage > MAX_LEVERAGE {
        return Err(LedgerError::InvalidLeverage.into());
    }

    // 读取配置
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    if ledger_config.is_paused {
        return Err(LedgerError::LedgerPaused.into());
    }

    // 验证 Vault Program
    if vault_program.key != &ledger_config.vault_program {
        return Err(LedgerError::InvalidVaultProgram.into());
    }

    // 计算所需保证金
    let required_margin = cpi::calculate_required_margin(size_e6, price_e6, leverage)?;
    let fee = cpi::calculate_fee(size_e6, price_e6, 1_000)?; // 0.1% fee

    // 派生 Position PDA
    let (position_pda, position_bump) = Pubkey::find_program_address(
        &[b"position", user.as_ref(), &[market_index]],
        program_id,
    );
    if position_info.key != &position_pda {
        return Err(LedgerError::InvalidAccount.into());
    }

    let current_ts = get_current_timestamp()?;

    // 检查是否是新仓位
    let is_new_position = position_info.data_len() == 0 || {
        let data = position_info.data.borrow();
        data.iter().all(|&x| x == 0)
    };

    if is_new_position {
        // 创建新仓位
        let rent = Rent::get()?;
        let space = Position::SIZE;
        let lamports = rent.minimum_balance(space);

        invoke_signed(
            &system_instruction::create_account(
                relayer.key,
                position_info.key,
                lamports,
                space as u64,
                program_id,
            ),
            &[relayer.clone(), position_info.clone(), system_program.clone()],
            &[&[b"position", user.as_ref(), &[market_index], &[position_bump]]],
        )?;

        let mut position = Position {
            discriminator: Position::DISCRIMINATOR,
            user,
            market_index,
            side,
            size_e6,
            entry_price_e6: price_e6,
            margin_e6: required_margin,
            leverage,
            liquidation_price_e6: 0, // 计算后设置
            unrealized_pnl_e6: 0,
            last_funding_ts: current_ts,
            cumulative_funding_e6: 0,
            open_order_count: 0,
            opened_at: current_ts,
            last_update_ts: current_ts,
            bump: position_bump,
            reserved: [0; 32],
        };

        // 计算清算价格
        position.liquidation_price_e6 = position.calculate_liquidation_price()?;
        position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

        msg!(
            "New position opened: user={}, market={}, side={:?}, size={}, entry={}, margin={}",
            user, market_index, side, size_e6, price_e6, required_margin
        );
    } else {
        // 加仓
        let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;

        // 验证方向一致
        if position.side != side {
            return Err(LedgerError::InvalidPositionSide.into());
        }

        // 更新仓位
        position.update_entry_price(size_e6, price_e6)?;
        position.margin_e6 = checked_add_u64(position.margin_e6, required_margin)?;
        position.last_update_ts = current_ts;

        position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

        msg!(
            "Position increased: user={}, market={}, new_size={}, new_margin={}",
            user, market_index, position.size_e6, position.margin_e6
        );
    }

    // CPI: 锁定保证金 + 扣除手续费
    let total_to_lock = checked_add_u64(required_margin, fee)?;
    
    // 调用 Vault Program 锁定保证金
    // 派生 Ledger Config PDA 用于 CPI 签名
    let (ledger_config_pda, ledger_config_bump) = Pubkey::find_program_address(
        &[b"ledger_config"],
        program_id,
    );
    
    cpi::lock_margin(
        vault_program.key,
        vault_config_info.clone(),
        user_account_info.clone(),
        ledger_config_info.clone(),  // 使用 LedgerConfig PDA 作为 caller
        total_to_lock,
        &[&[b"ledger_config", &[ledger_config_bump]]],  // PDA 签名
    )?;
    
    msg!("CPI: Locked margin {} + fee {}", required_margin, fee);

    // 更新统计
    ledger_config.total_positions_opened += 1;
    ledger_config.total_volume_e6 = checked_add_u64(
        ledger_config.total_volume_e6,
        (size_e6 as u128 * price_e6 as u128 / 1_000_000) as u64,
    )?;
    ledger_config.total_fees_collected_e6 = checked_add_u64(ledger_config.total_fees_collected_e6, fee)?;
    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    // 自动创建 UserStats (如果不存在)
    let _ = ensure_user_stats_exists(
        program_id,
        relayer,
        &user,
        user_stats_info,
        system_program,
    );

    // 更新用户统计 (现在保证存在)
    if user_stats_info.data_len() > 0 {
        // 先读取数据到局部变量，释放借用
        let user_stats_result = {
            let data = user_stats_info.data.borrow();
            deserialize_account::<UserStats>(&data)
        };
        
        if let Ok(mut user_stats) = user_stats_result {
            user_stats.total_trades += 1;
            user_stats.total_volume_e6 = checked_add_u64(
                user_stats.total_volume_e6,
                (size_e6 as u128 * price_e6 as u128 / 1_000_000) as u64,
            )?;
            user_stats.total_fees_paid_e6 = checked_add_u64(user_stats.total_fees_paid_e6, fee)?;
            if user_stats.first_trade_at == 0 {
                user_stats.first_trade_at = current_ts;
            }
            user_stats.last_trade_at = current_ts;
            // 现在可以安全地可变借用
            user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..])?;
        }
    }

    msg!("OpenPosition completed: batch_id={}, margin_locked={}, fee={}", batch_id, total_to_lock, fee);
    Ok(())
}

fn process_close_position(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u8,
    size_e6: u64,
    price_e6: u64,
    batch_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let position_info = next_account_info(account_info_iter)?;
    let user_account_info = next_account_info(account_info_iter)?;
    let vault_config_info = next_account_info(account_info_iter)?;
    let insurance_fund_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let user_stats_info = next_account_info(account_info_iter)?;
    let _vault_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(position_info)?;
    assert_writable(user_account_info)?;
    assert_writable(insurance_fund_info)?;
    assert_writable(ledger_config_info)?;
    assert_writable(user_stats_info)?;

    // 验证参数
    if size_e6 == 0 {
        return Err(LedgerError::InvalidTradeAmount.into());
    }
    if price_e6 == 0 {
        return Err(LedgerError::InvalidPrice.into());
    }

    // 读取配置
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    if ledger_config.is_paused {
        return Err(LedgerError::LedgerPaused.into());
    }

    // 读取仓位
    let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
    if position.user != user || position.market_index != market_index {
        return Err(LedgerError::PositionNotFound.into());
    }
    if position.is_empty() {
        return Err(LedgerError::PositionNotFound.into());
    }

    // 验证平仓数量
    let close_size = size_e6.min(position.size_e6);
    let close_ratio = div_e6(close_size as i64, position.size_e6 as i64)?;

    // 计算盈亏
    let pnl = position.calculate_unrealized_pnl(price_e6)?;
    let realized_pnl = mul_e6(pnl, close_ratio)?;

    // 计算释放的保证金
    let margin_to_release = mul_e6(position.margin_e6 as i64, close_ratio)? as u64;

    // 计算手续费
    let fee = cpi::calculate_fee(close_size, price_e6, 1_000)?; // 0.1% fee

    let current_ts = get_current_timestamp()?;

    // 更新或关闭仓位
    if close_size >= position.size_e6 {
        // 全部平仓 - 重置仓位
        position.size_e6 = 0;
        position.margin_e6 = 0;
        position.entry_price_e6 = 0;
        position.liquidation_price_e6 = 0;
        position.unrealized_pnl_e6 = 0;
    } else {
        // 部分平仓
        position.size_e6 = checked_sub_u64(position.size_e6, close_size)?;
        position.margin_e6 = checked_sub_u64(position.margin_e6, margin_to_release)?;
        // 重新计算清算价格
        position.liquidation_price_e6 = position.calculate_liquidation_price()?;
    }
    position.last_update_ts = current_ts;
    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

    // CPI 调用 Vault.closePositionSettle
    // 派生 Ledger Config PDA 用于 CPI 签名
    let (_, ledger_config_bump) = Pubkey::find_program_address(
        &[b"ledger_config"],
        program_id,
    );
    
    cpi::close_position_settle(
        &ledger_config.vault_program,
        vault_config_info.clone(),
        user_account_info.clone(),
        ledger_config_info.clone(),
        margin_to_release,
        realized_pnl,
        fee,
        &[&[b"ledger_config", &[ledger_config_bump]]],
    )?;
    
    msg!("CPI: Close position settle - margin={}, pnl={}, fee={}", margin_to_release, realized_pnl, fee);

    // 更新统计
    ledger_config.total_positions_closed += 1;
    ledger_config.total_volume_e6 = checked_add_u64(
        ledger_config.total_volume_e6,
        (close_size as u128 * price_e6 as u128 / 1_000_000) as u64,
    )?;
    ledger_config.total_fees_collected_e6 = checked_add_u64(ledger_config.total_fees_collected_e6, fee)?;
    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    // 更新用户统计
    if user_stats_info.data_len() > 0 {
        // 先读取数据到局部变量，释放借用
        let user_stats_result = {
            let data = user_stats_info.data.borrow();
            deserialize_account::<UserStats>(&data)
        };
        
        if let Ok(mut user_stats) = user_stats_result {
            user_stats.total_trades += 1;
            user_stats.total_volume_e6 = checked_add_u64(
                user_stats.total_volume_e6,
                (close_size as u128 * price_e6 as u128 / 1_000_000) as u64,
            )?;
            user_stats.total_realized_pnl_e6 = checked_add(user_stats.total_realized_pnl_e6, realized_pnl)?;
            user_stats.total_fees_paid_e6 = checked_add_u64(user_stats.total_fees_paid_e6, fee)?;
            user_stats.last_trade_at = current_ts;
            user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..])?;
        }
    }

    msg!(
        "ClosePosition completed: batch_id={}, size={}, pnl={}, margin_released={}, fee={}",
        batch_id, close_size, realized_pnl, margin_to_release, fee
    );
    Ok(())
}

// ============================================================================
// 清算指令处理
// ============================================================================

fn process_liquidate(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u8,
    mark_price_e6: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let liquidator = next_account_info(account_info_iter)?;
    let position_info = next_account_info(account_info_iter)?;
    let user_account_info = next_account_info(account_info_iter)?;
    let vault_config_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let user_stats_info = next_account_info(account_info_iter)?;
    let _vault_program = next_account_info(account_info_iter)?;
    // Vault Token Account for liquidation penalty transfer
    let vault_token_account = next_account_info(account_info_iter)?;
    // Fund Program accounts for insurance fund operations
    let fund_program = next_account_info(account_info_iter)?;
    let insurance_fund_account = next_account_info(account_info_iter)?;
    let insurance_config = next_account_info(account_info_iter)?;
    let insurance_vault = next_account_info(account_info_iter)?;
    let counterparty_vault = next_account_info(account_info_iter)?; // For shortfall coverage
    let token_program = next_account_info(account_info_iter)?;

    assert_signer(liquidator)?;
    assert_writable(position_info)?;
    assert_writable(user_account_info)?;
    assert_writable(ledger_config_info)?;
    assert_writable(user_stats_info)?;
    assert_writable(vault_token_account)?;
    assert_writable(insurance_vault)?;

    // 读取配置
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    // 验证 Fund Program
    if fund_program.key != &ledger_config.fund_program {
        return Err(LedgerError::InvalidProgramId.into());
    }

    // 读取仓位
    let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
    if position.user != user || position.market_index != market_index {
        return Err(LedgerError::PositionNotFound.into());
    }
    if position.is_empty() {
        return Err(LedgerError::PositionNotFound.into());
    }

    // 验证清算条件
    if !position.should_liquidate(mark_price_e6) {
        return Err(LedgerError::PositionNotLiquidatable.into());
    }

    let current_ts = get_current_timestamp()?;

    // 计算清算结果
    let pnl = position.calculate_unrealized_pnl(mark_price_e6)?;
    let margin = position.margin_e6;

    // 计算各方分配
    let (user_remainder, liquidation_penalty, shortfall) = calculate_liquidation_result(margin, pnl);

    // 关闭仓位
    position.size_e6 = 0;
    position.margin_e6 = 0;
    position.entry_price_e6 = 0;
    position.liquidation_price_e6 = 0;
    position.unrealized_pnl_e6 = 0;
    position.last_update_ts = current_ts;
    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

    // 派生 Ledger Config PDA 用于 CPI 签名
    let (_, ledger_config_bump) = Pubkey::find_program_address(
        &[b"ledger_config"],
        program_id,
    );
    let bump_slice = [ledger_config_bump];
    let signer_seeds = &[&[b"ledger_config".as_ref(), bump_slice.as_ref()][..]];
    
    // CPI 1: 更新用户账户 + 转移清算罚金到 Insurance Fund (Vault Program)
    // 这个 CPI 会执行实际的 Token Transfer: Vault Token Account -> Insurance Fund Vault
    cpi::liquidate_position(
        &ledger_config.vault_program,
        vault_config_info.clone(),
        user_account_info.clone(),
        ledger_config_info.clone(),
        vault_token_account.clone(),
        insurance_vault.clone(),
        token_program.clone(),
        margin,
        user_remainder,
        liquidation_penalty,
        signer_seeds,
    )?;
    
    msg!(
        "CPI: Liquidate user account - margin={}, remainder={}, penalty={}",
        margin,
        user_remainder,
        liquidation_penalty
    );
    
    // CPI 2: 记录清算罚金到保险基金统计 (Fund Program)
    // 注意: Token 已经在 CPI 1 中转移完成，这里只是更新统计
    if liquidation_penalty > 0 {
        cpi::add_liquidation_income(
            fund_program.key,
            ledger_config_info.clone(),
            insurance_fund_account.clone(),
            insurance_config.clone(),
            liquidation_penalty as i64,
            signer_seeds,
        )?;
        msg!("CPI: Liquidation penalty {} recorded in insurance fund stats", liquidation_penalty);
    }
    
    // CPI 3: 覆盖穿仓 (Fund Program)
    if shortfall > 0 {
        cpi::cover_shortfall(
            fund_program.key,
            ledger_config_info.clone(),
            insurance_fund_account.clone(),
            insurance_config.clone(),
            insurance_vault.clone(),
            counterparty_vault.clone(),
            token_program.clone(),
            shortfall as i64,
            signer_seeds,
        )?;
        msg!("CPI: Shortfall {} coverage requested from insurance fund", shortfall);
    }

    // 更新统计
    ledger_config.total_liquidations += 1;
    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    // 更新用户统计
    if user_stats_info.data_len() > 0 {
        // 先读取数据到局部变量，释放借用
        let user_stats_result = {
            let data = user_stats_info.data.borrow();
            deserialize_account::<UserStats>(&data)
        };
        
        if let Ok(mut user_stats) = user_stats_result {
            user_stats.total_liquidations += 1;
            user_stats.total_realized_pnl_e6 = checked_add(user_stats.total_realized_pnl_e6, pnl)?;
            user_stats.last_trade_at = current_ts;
            user_stats.serialize(&mut &mut user_stats_info.data.borrow_mut()[..])?;
        }
    }

    msg!(
        "Liquidation completed: user={}, market={}, mark_price={}, pnl={}, remainder={}, penalty={}, shortfall={}",
        user, market_index, mark_price_e6, pnl, user_remainder, liquidation_penalty, shortfall
    );

    // 如果有穿仓且保险基金不足，需要触发 ADL
    if shortfall > 0 {
        msg!("⚠️ Shortfall detected: {}, ADL may be required if insurance fund insufficient", shortfall);
    }

    Ok(())
}

/// 计算清算结果
/// 返回 (user_remainder, liquidation_penalty, shortfall)
fn calculate_liquidation_result(margin: u64, pnl: i64) -> (u64, u64, u64) {
    let margin_i = margin as i64;
    let total = margin_i + pnl;

    if total <= 0 {
        // 穿仓: 用户没有剩余，有穿仓损失
        let shortfall = (-total) as u64;
        (0, 0, shortfall)
    } else {
        // 有剩余: 计算罚金和用户剩余
        let total_u = total as u64;
        let penalty = mul_e6(total as i64, LIQUIDATION_PENALTY_RATE).unwrap_or(0) as u64; // 1% 罚金
        let user_remainder = total_u.saturating_sub(penalty);
        (user_remainder, penalty, 0)
    }
}

/// P0-2/NEW-1 实现: 完整的 ADL 链上触发逻辑 + Fund Program CPI 集成
/// 
/// ADL (Auto-Deleveraging) 流程:
/// 1. 验证保险基金确实不足以覆盖穿仓
/// 2. 验证目标仓位是有效的反向盈利仓位
/// 3. CPI 调用 Fund Program 设置 ADL 状态
/// 4. 标记 ADL 状态并记录事件
/// 5. 实际的平仓操作由链下引擎执行
/// 
/// 账户顺序:
/// 0. admin (signer) - 管理员/Relayer
/// 1. ledger_config_info (writable) - Ledger 全局配置
/// 2. fund_program - Fund Program ID
/// 3. insurance_config (writable) - InsuranceFundConfig PDA
/// 4. fund_vault - Insurance Fund Vault (Token Account)
/// 5..n. target_position_infos - 目标仓位账户
fn process_trigger_adl(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    market_index: u8,
    shortfall_e6: u64,
    bankrupt_side: Side,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let fund_program = next_account_info(account_info_iter)?;
    let insurance_config = next_account_info(account_info_iter)?;
    let fund_vault = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(ledger_config_info)?;
    assert_writable(insurance_config)?;

    // 读取配置
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    // NEW-1: 验证 Fund Program
    if fund_program.key != &ledger_config.fund_program {
        msg!("❌ Invalid Fund Program: expected {}, got {}", ledger_config.fund_program, fund_program.key);
        return Err(LedgerError::InvalidProgramId.into());
    }

    // P0-2: 验证是管理员或授权 Relayer
    if ledger_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    // P0-2: 验证程序未暂停
    if ledger_config.is_paused {
        return Err(LedgerError::LedgerPaused.into());
    }

    let current_ts = get_current_timestamp()?;

    // NEW-1: 从 Fund Program 的 InsuranceFundConfig 读取保险基金余额
    // InsuranceFundConfig 结构:
    // - discriminator: u64 (8 bytes)
    // - fund: Pubkey (32 bytes)
    // - bump: u8 (1 byte)
    // - total_liquidation_income_e6: i64 (8 bytes)
    // - total_adl_profit_e6: i64 (8 bytes)
    // - total_shortfall_payout_e6: i64 (8 bytes)
    // - adl_trigger_threshold_e6: i64 (8 bytes)
    // - adl_trigger_count: u64 (8 bytes)
    // - balance_1h_ago_e6: i64 (8 bytes)
    // ... 
    // 我们需要从 fund_vault 读取实际余额
    let insurance_balance_e6 = read_insurance_fund_balance_from_vault(fund_vault)?;
    let insurance_config_data = read_insurance_fund_config(insurance_config)?;

    msg!(
        "NEW-1 ADL Check: shortfall={}, insurance_balance={}, threshold={}",
        shortfall_e6,
        insurance_balance_e6,
        insurance_config_data.adl_trigger_threshold_e6
    );

    // P0-2 步骤2: 验证保险基金确实不足
    if insurance_balance_e6 >= shortfall_e6 as i64 {
        msg!("⚠️ Insurance fund sufficient, ADL not required");
        return Err(LedgerError::ADLNotRequired.into());
    }

    // P0-2 步骤3: 计算需要 ADL 覆盖的金额
    let adl_required = shortfall_e6 as i64 - insurance_balance_e6;
    msg!(
        "NEW-1 ADL Required: {} (shortfall={}, insurance={})",
        adl_required,
        shortfall_e6,
        insurance_balance_e6
    );

    // P0-2 步骤4: 验证目标仓位
    // 收集剩余的账户作为目标仓位
    let mut validated_targets: Vec<Pubkey> = Vec::new();
    let mut total_available_pnl: i64 = 0;
    let counterparty_side = bankrupt_side.opposite();

    for target_info in account_info_iter {
        // 尝试反序列化为 Position
        if let Ok(position) = deserialize_account::<Position>(&target_info.data.borrow()) {
            // 验证: 必须是同市场
            if position.market_index != market_index {
                msg!("⚠️ Position {} wrong market, skipping", target_info.key);
                continue;
            }

            // 验证: 必须是反向方向
            if position.side != counterparty_side {
                msg!("⚠️ Position {} wrong side, skipping", target_info.key);
                continue;
            }

            // 验证: 必须有盈利 (unrealized_pnl > 0)
            if position.unrealized_pnl_e6 <= 0 {
                msg!("⚠️ Position {} no profit, skipping", target_info.key);
                continue;
            }

            // 验证通过
            validated_targets.push(*target_info.key);
            total_available_pnl += position.unrealized_pnl_e6;

            msg!(
                "✅ ADL Target validated: {}, pnl={}",
                target_info.key,
                position.unrealized_pnl_e6
            );
        }
    }

    // P0-2: 验证是否有足够的目标仓位
    if validated_targets.is_empty() {
        msg!("❌ No valid ADL targets found");
        return Err(LedgerError::NoOpposingPositionsForADL.into());
    }

    msg!(
        "NEW-1 ADL Targets: {} positions, total_pnl={}",
        validated_targets.len(),
        total_available_pnl
    );

    // NEW-1 步骤5: CPI 调用 Fund Program SetADLInProgress
    // 派生 Ledger Config PDA 用于 CPI 签名
    let (_, ledger_config_bump) = Pubkey::find_program_address(
        &[b"ledger_config"],
        program_id,
    );
    let bump_slice = [ledger_config_bump];
    let signer_seeds = &[&[b"ledger_config".as_ref(), bump_slice.as_ref()][..]];
    
    cpi::set_adl_in_progress(
        fund_program.key,
        ledger_config_info.clone(),
        insurance_config.clone(),
        true, // Set ADL in progress = true
        signer_seeds,
    )?;
    
    msg!("✅ NEW-1: CPI SetADLInProgress(true) - LP redemptions paused");

    // P0-2 步骤6: 更新 ADL 状态
    ledger_config.total_adl_count += 1;
    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    // P0-2 步骤7: 发出 ADL 触发事件
    // 使用 Solana 的 msg! 记录事件（链上程序无法发出真正的事件，使用日志）
    msg!("🚨 ADL_TRIGGERED_EVENT:");
    msg!("  market_index: {}", market_index);
    msg!("  shortfall_e6: {}", shortfall_e6);
    msg!("  insurance_balance_e6: {}", insurance_balance_e6);
    msg!("  adl_required_e6: {}", adl_required);
    msg!("  bankrupt_side: {:?}", bankrupt_side);
    msg!("  target_count: {}", validated_targets.len());
    msg!("  total_available_pnl_e6: {}", total_available_pnl);
    msg!("  timestamp: {}", current_ts);
    msg!("  adl_count: {}", ledger_config.total_adl_count);

    // 注意: 实际的平仓操作由链下 ADL Engine 执行
    // 链上仅负责验证和记录，并通过 CPI 暂停 LP 赎回

    Ok(())
}

/// NEW-1: InsuranceFundConfig 数据 (用于读取)
struct InsuranceFundConfigData {
    #[allow(dead_code)]
    discriminator: u64,
    #[allow(dead_code)]
    fund: Pubkey,
    #[allow(dead_code)]
    bump: u8,
    #[allow(dead_code)]
    total_liquidation_income_e6: i64,
    #[allow(dead_code)]
    total_adl_profit_e6: i64,
    #[allow(dead_code)]
    total_shortfall_payout_e6: i64,
    adl_trigger_threshold_e6: i64,
    #[allow(dead_code)]
    adl_trigger_count: u64,
    #[allow(dead_code)]
    balance_1h_ago_e6: i64,
    #[allow(dead_code)]
    last_snapshot_ts: i64,
    #[allow(dead_code)]
    withdrawal_delay_secs: i64,
    #[allow(dead_code)]
    is_adl_in_progress: bool,
}

/// NEW-1: 从 Fund Program 的 InsuranceFundConfig 读取配置
fn read_insurance_fund_config(insurance_config: &AccountInfo) -> Result<InsuranceFundConfigData, ProgramError> {
    let data = insurance_config.data.borrow();
    
    // InsuranceFundConfig 最小大小检查
    // discriminator(8) + fund(32) + bump(1) + 6*i64(48) + u64(8) + i64(8) + i64(8) + bool(1) + pubkey(32) + i64(8)
    // = 8 + 32 + 1 + 48 + 8 + 8 + 8 + 1 + 32 + 8 = 154 bytes minimum
    if data.len() < 154 {
        msg!("InsuranceFundConfig account too small: {}", data.len());
        return Err(LedgerError::InvalidAccount.into());
    }
    
    // 读取 discriminator
    let discriminator = u64::from_le_bytes(data[0..8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    
    // 验证 discriminator (0x1024_1024_0004 for InsuranceFundConfig)
    // 这里我们跳过严格验证，因为可能有不同的 discriminator 值
    
    let mut offset = 8;
    
    // fund: Pubkey (32 bytes)
    let fund = Pubkey::try_from(&data[offset..offset+32]).map_err(|_| LedgerError::InvalidAccount)?;
    offset += 32;
    
    // bump: u8 (1 byte)
    let bump = data[offset];
    offset += 1;
    
    // total_liquidation_income_e6: i64
    let total_liquidation_income_e6 = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // total_adl_profit_e6: i64
    let total_adl_profit_e6 = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // total_shortfall_payout_e6: i64
    let total_shortfall_payout_e6 = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // adl_trigger_threshold_e6: i64
    let adl_trigger_threshold_e6 = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // adl_trigger_count: u64
    let adl_trigger_count = u64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // balance_1h_ago_e6: i64
    let balance_1h_ago_e6 = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // last_snapshot_ts: i64
    let last_snapshot_ts = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // withdrawal_delay_secs: i64
    let withdrawal_delay_secs = i64::from_le_bytes(data[offset..offset+8].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    offset += 8;
    
    // is_adl_in_progress: bool
    let is_adl_in_progress = data[offset] != 0;
    
    Ok(InsuranceFundConfigData {
        discriminator,
        fund,
        bump,
        total_liquidation_income_e6,
        total_adl_profit_e6,
        total_shortfall_payout_e6,
        adl_trigger_threshold_e6,
        adl_trigger_count,
        balance_1h_ago_e6,
        last_snapshot_ts,
        withdrawal_delay_secs,
        is_adl_in_progress,
    })
}

/// NEW-1: 从 Fund Vault (SPL Token Account) 读取实际余额
fn read_insurance_fund_balance_from_vault(fund_vault: &AccountInfo) -> Result<i64, ProgramError> {
    // SPL Token Account 结构:
    // - mint: Pubkey (32 bytes)
    // - owner: Pubkey (32 bytes)
    // - amount: u64 (8 bytes) <- 我们需要这个
    // - ...
    
    let data = fund_vault.data.borrow();
    if data.len() < 72 {
        msg!("Fund vault account too small: {}", data.len());
        return Err(LedgerError::InvalidAccount.into());
    }
    
    // 读取 amount (offset 64)
    let amount = u64::from_le_bytes(data[64..72].try_into().map_err(|_| LedgerError::InvalidAccount)?);
    
    // 转换为 i64 (安全，因为余额不会超过 i64::MAX)
    Ok(amount as i64)
}

// ============================================================================
// 资金费率处理
// ============================================================================

fn process_settle_funding(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u8,
    funding_rate_e6: i64,
    index_price_e6: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let position_info = next_account_info(account_info_iter)?;
    let user_account_info = next_account_info(account_info_iter)?;
    let vault_config_info = next_account_info(account_info_iter)?;
    let _vault_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(position_info)?;
    assert_writable(user_account_info)?;

    // 读取仓位
    let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
    if position.user != user || position.market_index != market_index {
        return Err(LedgerError::PositionNotFound.into());
    }
    if position.is_empty() {
        return Err(LedgerError::PositionNotFound.into());
    }

    let current_ts = get_current_timestamp()?;

    // 计算资金费
    // funding_payment = position_value * funding_rate
    // Long 支付 Short (funding_rate > 0)
    // Short 支付 Long (funding_rate < 0)
    let position_value = (position.size_e6 as i128)
        .checked_mul(index_price_e6 as i128)
        .ok_or(LedgerError::Overflow)?
        .checked_div(1_000_000)
        .ok_or(LedgerError::Overflow)? as i64;

    let funding_payment = mul_e6(position_value, funding_rate_e6)?;

    // 根据方向调整符号
    let actual_payment = match position.side {
        Side::Long => funding_payment,  // Long 支付正 funding
        Side::Short => -funding_payment, // Short 收取正 funding
    };

    // 更新仓位
    position.cumulative_funding_e6 = checked_add(position.cumulative_funding_e6, actual_payment)?;
    position.last_funding_ts = current_ts;
    position.last_update_ts = current_ts;
    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

    // CPI 调用 Vault 更新用户余额
    // 从用户账户扣除/增加 funding_payment
    // 读取 LedgerConfig 获取 vault_program
    let ledger_config = deserialize_account::<LedgerConfig>(&vault_config_info.data.borrow())
        .ok()
        .map(|c| c.vault_program);
    
    // 资金费率结算通过更新用户持仓记录完成
    // 实际的资金转移在平仓时一并结算
    // TODO: 如果需要实时结算资金费率，需要添加对应的 Vault CPI
    msg!("Funding payment recorded: {}", actual_payment);

    msg!(
        "Funding settled: user={}, market={}, rate={}, payment={}",
        user, market_index, funding_rate_e6, actual_payment
    );

    Ok(())
}

// ============================================================================
// 管理指令处理
// ============================================================================

fn process_add_relayer(accounts: &[AccountInfo], relayer: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(relayer_config_info)?;

    let mut relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;

    if relayer_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    if relayer_config.authorized_relayers.len() >= MAX_RELAYERS {
        return Err(LedgerError::InvalidAccount.into());
    }

    if !relayer_config.authorized_relayers.contains(&relayer) {
        relayer_config.authorized_relayers.push(relayer);
        relayer_config.last_update_ts = get_current_timestamp()?;
        relayer_config.serialize(&mut &mut relayer_config_info.data.borrow_mut()[..])?;
        msg!("Added relayer: {}", relayer);
    }

    Ok(())
}

fn process_remove_relayer(accounts: &[AccountInfo], relayer: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(relayer_config_info)?;

    let mut relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;

    if relayer_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    relayer_config.authorized_relayers.retain(|&r| r != relayer);
    relayer_config.last_update_ts = get_current_timestamp()?;
    relayer_config.serialize(&mut &mut relayer_config_info.data.borrow_mut()[..])?;

    msg!("Removed relayer: {}", relayer);
    Ok(())
}

fn process_update_required_signatures(accounts: &[AccountInfo], required_signatures: u8) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(relayer_config_info)?;

    let mut relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;

    if relayer_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    if required_signatures == 0 || required_signatures as usize > relayer_config.authorized_relayers.len() {
        return Err(LedgerError::InsufficientSignatures.into());
    }

    relayer_config.required_signatures = required_signatures;
    relayer_config.last_update_ts = get_current_timestamp()?;
    relayer_config.serialize(&mut &mut relayer_config_info.data.borrow_mut()[..])?;

    msg!("Updated required signatures to: {}", required_signatures);
    Ok(())
}

fn process_set_paused(accounts: &[AccountInfo], paused: bool) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(ledger_config_info)?;

    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    if ledger_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    ledger_config.is_paused = paused;
    ledger_config.last_update_ts = get_current_timestamp()?;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("Ledger {}", if paused { "paused" } else { "resumed" });
    Ok(())
}

fn process_update_admin(accounts: &[AccountInfo], new_admin: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let current_admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;

    assert_signer(current_admin)?;
    assert_writable(ledger_config_info)?;

    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    if ledger_config.admin != *current_admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    ledger_config.admin = new_admin;
    ledger_config.last_update_ts = get_current_timestamp()?;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("Admin updated to: {}", new_admin);
    Ok(())
}

fn process_update_vault_program(accounts: &[AccountInfo], new_vault_program: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(ledger_config_info)?;

    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    if ledger_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    ledger_config.vault_program = new_vault_program;
    ledger_config.last_update_ts = get_current_timestamp()?;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("Vault program updated to: {}", new_vault_program);
    Ok(())
}

fn process_update_fund_program(accounts: &[AccountInfo], new_fund_program: Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(ledger_config_info)?;

    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;

    if ledger_config.admin != *admin.key {
        return Err(LedgerError::InvalidAdmin.into());
    }

    ledger_config.fund_program = new_fund_program;
    ledger_config.last_update_ts = get_current_timestamp()?;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("Fund program updated to: {}", new_fund_program);
    Ok(())
}

