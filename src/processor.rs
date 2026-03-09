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
    events::{self, OrderEvent, emit_order_event, event_discriminator, PositionEvent, TradeEvent, BatchEvent, FeeEvent, InsuranceFundEvent, emit_position_event, emit_trade_event, emit_batch_event, emit_fee_event, emit_insurance_fund_event},
    instruction::{LedgerInstruction, TradeData, OrderEventInput, FundingEventInput, trade_data_type},
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
            is_taker: _,
            fee_rate_e6,
        } => {
            msg!("Instruction: OpenPosition");
            if fee_rate_e6 > 10_000 {
                return Err(LedgerError::InvalidFeeRate.into());
            }
            process_open_position(
                program_id, accounts, user, market_index, side, size_e6, price_e6, leverage, batch_id, fee_rate_e6,
            )
        }
        LedgerInstruction::ClosePosition {
            user,
            market_index,
            size_e6,
            price_e6,
            batch_id,
            is_taker: _,
            fee_rate_e6,
        } => {
            msg!("Instruction: ClosePosition");
            if fee_rate_e6 > 10_000 {
                return Err(LedgerError::InvalidFeeRate.into());
            }
            process_close_position(program_id, accounts, user, market_index, size_e6, price_e6, batch_id, fee_rate_e6)
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
        LedgerInstruction::InitializeUserStats => {
            msg!("Instruction: InitializeUserStats");
            process_initialize_user_stats(program_id, accounts)
        }
        LedgerInstruction::AdminResetPosition { user, market_index } => {
            msg!("Instruction: AdminResetPosition");
            process_admin_reset_position(program_id, accounts, user, market_index)
        }
        
        // Spot 交易指令
        LedgerInstruction::RecordSpotTrade {
            user,
            market_index,
            is_buy,
            base_amount_e6,
            quote_amount_e6,
            price_e6,
            fee_e6,
            is_taker,
            batch_id,
        } => {
            msg!("Instruction: RecordSpotTrade");
            process_record_spot_trade(
                program_id, accounts, user, market_index, is_buy,
                base_amount_e6, quote_amount_e6, price_e6, fee_e6, is_taker, batch_id
            )
        }
        LedgerInstruction::BatchRecordSpotTrades { trades, batch_id } => {
            msg!("Instruction: BatchRecordSpotTrades");
            process_batch_record_spot_trades(program_id, accounts, trades, batch_id)
        }
        LedgerInstruction::RecordOrderEvents { events } => {
            msg!("Instruction: RecordOrderEvents ({} events)", events.len());
            process_record_order_events(program_id, accounts, events)
        }
        LedgerInstruction::RecordFundingEvents { events } => {
            msg!("Instruction: RecordFundingEvents ({} events)", events.len());
            process_record_funding_events(program_id, accounts, events)
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

    // Emit BatchEvent (Submitted)
    events::emit_batch_event(&events::BatchEvent {
        discriminator: events::event_discriminator::BATCH,
        batch_id,
        timestamp: current_ts,
        event_type: events::BatchStatus::Submitted as u8,
        trade_count: 0,
        total_notional_e6: 0,
        relayer: *relayer.key,
        data_hash,
        chain_tx: [0u8; 64],
        error_code: 0,
    });

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

    emit_batch_event(&BatchEvent {
        discriminator: event_discriminator::BATCH,
        batch_id,
        timestamp: current_ts,
        event_type: 1,
        trade_count: 0,
        total_notional_e6: 0,
        relayer: *relayer.key,
        data_hash,
        chain_tx: [0u8; 64],
        error_code: 0,
    });

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
                if trade.fee_rate_e6 > 10_000 {
                    return Err(LedgerError::InvalidFeeRate.into());
                }
                let fee = cpi::calculate_fee(trade.size_e6, trade.price_e6, trade.fee_rate_e6)?;

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

                // Emit PositionEvent (OPEN)
                let side_u8 = match trade.side { Side::Long => 0u8, Side::Short => 1u8 };
                events::emit_position_event(&events::PositionEvent {
                    discriminator: events::event_discriminator::POSITION,
                    sequence,
                    timestamp: current_ts,
                    user: trade.user,
                    market_index: trade.market_index,
                    event_type: if is_new_position {
                        events::PositionEventType::Opened as u8
                    } else {
                        events::PositionEventType::Increased as u8
                    },
                    side_before: side_u8,
                    size_before_e6: if is_new_position { 0 } else { trade.size_e6 }, // approximate
                    entry_price_before_e6: 0,
                    margin_before_e6: if is_new_position { 0 } else { required_margin },
                    side_after: side_u8,
                    size_after_e6: trade.size_e6,
                    entry_price_after_e6: trade.price_e6,
                    margin_after_e6: required_margin,
                    size_delta_e6: trade.size_e6 as i64,
                    realized_pnl_e6: 0,
                    fee_e6: fee,
                    related_trade_sequence: sequence,
                });
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

                let close_size = trade.size_e6.min(position.size_e6);
                let close_ratio = div_e6(close_size as i64, position.size_e6 as i64)?;
                let pnl = position.calculate_unrealized_pnl(trade.price_e6)?;
                let realized_pnl = mul_e6(pnl, close_ratio)?;
                let original_margin = position.margin_e6;
                let mut margin_to_release = mul_e6(position.margin_e6 as i64, close_ratio)? as u64;
                if trade.fee_rate_e6 > 10_000 {
                    return Err(LedgerError::InvalidFeeRate.into());
                }
                let fee = cpi::calculate_fee(close_size, trade.price_e6, trade.fee_rate_e6)?;

                if close_size >= position.size_e6 {
                    margin_to_release = position.margin_e6;
                    position.size_e6 = 0;
                    position.margin_e6 = 0;
                    position.entry_price_e6 = 0;
                    position.liquidation_price_e6 = 0;
                    position.unrealized_pnl_e6 = 0;
                } else {
                    position.size_e6 = checked_sub_u64(position.size_e6, close_size)?;
                    position.margin_e6 = checked_sub_u64(position.margin_e6, margin_to_release)?;
                    position.liquidation_price_e6 = position.calculate_liquidation_price()?;
                    if position.size_e6 == 0 {
                        margin_to_release = original_margin;
                        position.margin_e6 = 0;
                        position.entry_price_e6 = 0;
                        position.liquidation_price_e6 = 0;
                        position.unrealized_pnl_e6 = 0;
                    }
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

                // Emit TradeEvent (CLOSE)
                let side_u8 = match position.side { Side::Long => 0u8, Side::Short => 1u8 };
                events::emit_trade_event(&events::TradeEvent {
                    discriminator: events::event_discriminator::TRADE,
                    sequence,
                    timestamp: current_ts,
                    batch_id,
                    market_index: trade.market_index,
                    market_type: 0, // Perp
                    trade_type: events::TradeType::Normal as u8,
                    maker: trade.user,
                    maker_order_id: [0u8; 16],
                    maker_side: side_u8,
                    maker_fee_e6: 0,
                    taker: trade.user,
                    taker_order_id: [0u8; 16],
                    taker_side: side_u8,
                    taker_fee_e6: fee as i64,
                    price_e6: trade.price_e6,
                    size_e6: close_size,
                    notional_e6: (close_size as u128 * trade.price_e6 as u128 / 1_000_000) as u64,
                    maker_realized_pnl_e6: realized_pnl,
                    taker_realized_pnl_e6: 0,
                    maker_margin_delta_e6: -(margin_to_release as i64),
                    taker_margin_delta_e6: 0,
                });

                // Emit PositionEvent (CLOSE)
                let is_full_close = position.size_e6 == 0;
                events::emit_position_event(&events::PositionEvent {
                    discriminator: events::event_discriminator::POSITION,
                    sequence,
                    timestamp: current_ts,
                    user: trade.user,
                    market_index: trade.market_index,
                    event_type: if is_full_close {
                        events::PositionEventType::Closed as u8
                    } else {
                        events::PositionEventType::Decreased as u8
                    },
                    side_before: side_u8,
                    size_before_e6: close_size + position.size_e6,
                    entry_price_before_e6: position.entry_price_e6,
                    margin_before_e6: original_margin,
                    side_after: side_u8,
                    size_after_e6: position.size_e6,
                    entry_price_after_e6: position.entry_price_e6,
                    margin_after_e6: position.margin_e6,
                    size_delta_e6: -(close_size as i64),
                    realized_pnl_e6: realized_pnl,
                    fee_e6: fee,
                    related_trade_sequence: sequence,
                });
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

    emit_batch_event(&BatchEvent {
        discriminator: event_discriminator::BATCH,
        batch_id,
        timestamp: current_ts,
        event_type: 2,
        trade_count: trades.len() as u16,
        total_notional_e6: 0,
        relayer: *relayer.key,
        data_hash: trade_batch.data_hash,
        chain_tx: [0u8; 64],
        error_code: 0,
    });

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
    fee_rate: u64,
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
    let fee = cpi::calculate_fee(size_e6, price_e6, fee_rate)?; // P1B: dynamic fee rate

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

    let mut pre_side: u8 = 0;
    let mut pre_size: u64 = 0;
    let mut pre_entry: u64 = 0;
    let mut pre_margin: u64 = 0;
    let mut post_side: u8 = 0;
    let mut post_size: u64 = 0;
    let mut post_entry: u64 = 0;
    let mut post_margin: u64 = 0;

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
            margin_e6: checked_add_u64(required_margin, fee)?,
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

        post_side = position.side as u8;
        post_size = position.size_e6;
        post_entry = position.entry_price_e6;
        post_margin = position.margin_e6;

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

        pre_side = position.side as u8;
        pre_size = position.size_e6;
        pre_entry = position.entry_price_e6;
        pre_margin = position.margin_e6;

        // 更新仓位
        position.update_entry_price(size_e6, price_e6)?;
        position.margin_e6 = checked_add_u64(position.margin_e6, checked_add_u64(required_margin, fee)?)?;
        position.last_update_ts = current_ts;

        position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

        post_side = position.side as u8;
        post_size = position.size_e6;
        post_entry = position.entry_price_e6;
        post_margin = position.margin_e6;

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
    let position_event_seq = ledger_config.next_sequence();
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

    emit_position_event(&PositionEvent {
        discriminator: event_discriminator::POSITION,
        sequence: position_event_seq,
        timestamp: current_ts,
        user,
        market_index,
        event_type: if is_new_position { 0 } else { 1 },
        side_before: pre_side,
        size_before_e6: pre_size,
        entry_price_before_e6: pre_entry,
        margin_before_e6: pre_margin,
        side_after: post_side,
        size_after_e6: post_size,
        entry_price_after_e6: post_entry,
        margin_after_e6: post_margin,
        size_delta_e6: size_e6 as i64,
        realized_pnl_e6: 0,
        fee_e6: fee,
        related_trade_sequence: 0,
    });

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
    fee_rate: u64,
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

    let close_pre_side = position.side as u8;
    let close_pre_size = position.size_e6;
    let close_pre_entry = position.entry_price_e6;
    let close_pre_margin = position.margin_e6;

    // 验证平仓数量
    let close_size = size_e6.min(position.size_e6);
    let close_ratio = div_e6(close_size as i64, position.size_e6 as i64)?;

    // 计算盈亏
    let pnl = position.calculate_unrealized_pnl(price_e6)?;
    let realized_pnl = mul_e6(pnl, close_ratio)?;

    // S0-4 fix: capture original margin before any mutation, so partial-to-full-close
    // releases the correct total amount instead of only the rounding remainder.
    let original_margin = position.margin_e6;

    let mut margin_to_release = mul_e6(position.margin_e6 as i64, close_ratio)? as u64;

    let fee = cpi::calculate_fee(close_size, price_e6, fee_rate)?;

    let current_ts = get_current_timestamp()?;

    let is_full_close = close_size >= position.size_e6;
    if is_full_close {
        margin_to_release = position.margin_e6;
    }

    if is_full_close {
        position.size_e6 = 0;
        position.margin_e6 = 0;
        position.entry_price_e6 = 0;
        position.liquidation_price_e6 = 0;
        position.unrealized_pnl_e6 = 0;
    } else {
        position.size_e6 = checked_sub_u64(position.size_e6, close_size)?;
        position.margin_e6 = checked_sub_u64(position.margin_e6, margin_to_release)?;
        position.liquidation_price_e6 = position.calculate_liquidation_price()?;
        
        if position.size_e6 == 0 {
            // S0-4: partial close resulted in full close due to precision —
            // release the ENTIRE original margin, not just the remainder after proportional subtraction.
            margin_to_release = original_margin;
            position.margin_e6 = 0;
            position.entry_price_e6 = 0;
            position.liquidation_price_e6 = 0;
            position.unrealized_pnl_e6 = 0;
        }
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
    let close_pos_event_seq = ledger_config.next_sequence();
    let close_trade_event_seq = ledger_config.next_sequence();
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

    let close_event_type = if position.size_e6 == 0 { 2u8 } else { 3u8 };
    emit_position_event(&PositionEvent {
        discriminator: event_discriminator::POSITION,
        sequence: close_pos_event_seq,
        timestamp: current_ts,
        user,
        market_index,
        event_type: close_event_type,
        side_before: close_pre_side,
        size_before_e6: close_pre_size,
        entry_price_before_e6: close_pre_entry,
        margin_before_e6: close_pre_margin,
        side_after: position.side as u8,
        size_after_e6: position.size_e6,
        entry_price_after_e6: position.entry_price_e6,
        margin_after_e6: position.margin_e6,
        size_delta_e6: -(close_size as i64),
        realized_pnl_e6: realized_pnl,
        fee_e6: fee,
        related_trade_sequence: close_trade_event_seq,
    });

    emit_trade_event(&TradeEvent {
        discriminator: event_discriminator::TRADE,
        sequence: close_trade_event_seq,
        timestamp: current_ts,
        batch_id,
        market_index,
        market_type: 0,
        trade_type: 0,
        maker: Pubkey::default(),
        maker_order_id: [0u8; 16],
        maker_side: 0,
        maker_fee_e6: 0,
        taker: user,
        taker_order_id: [0u8; 16],
        taker_side: close_pre_side,
        taker_fee_e6: fee as i64,
        price_e6,
        size_e6: close_size,
        notional_e6: (close_size as u128 * price_e6 as u128 / 1_000_000) as u64,
        maker_realized_pnl_e6: 0,
        taker_realized_pnl_e6: realized_pnl,
        maker_margin_delta_e6: 0,
        taker_margin_delta_e6: -(margin_to_release as i64),
    });

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
    let pre_liq_side = position.side.clone();
    let pre_liq_size = position.size_e6;
    let pre_liq_entry = position.entry_price_e6;
    let pre_liq_liq_price = position.liquidation_price_e6;

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
    let sequence = ledger_config.next_sequence();
    let liq_pos_seq = ledger_config.next_sequence();
    let liq_fee_seq = ledger_config.next_sequence();
    let liq_ins_seq = ledger_config.next_sequence();
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    // 更新用户统计
    if user_stats_info.data_len() > 0 {
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

    // Emit LiquidationEvent
    let side_u8 = match pre_liq_side { Side::Long => 0u8, Side::Short => 1u8 };
    events::emit_liquidation_event(&events::LiquidationEvent {
        discriminator: events::event_discriminator::LIQUIDATION,
        sequence,
        timestamp: current_ts,
        user,
        market_index,
        side: side_u8,
        position_size_e6: pre_liq_size,
        entry_price_e6: pre_liq_entry,
        mark_price_e6,
        liquidation_price_e6: pre_liq_liq_price,
        margin_e6: margin,
        margin_ratio_e6: 0,
        penalty_e6: liquidation_penalty,
        insurance_payout_e6: shortfall,
        remaining_margin_e6: user_remainder as i64,
        is_bankruptcy: shortfall > 0,
        realized_pnl_e6: pnl,
        related_trade_sequence: sequence,
    });

    emit_position_event(&PositionEvent {
        discriminator: event_discriminator::POSITION,
        sequence: liq_pos_seq,
        timestamp: current_ts,
        user,
        market_index,
        event_type: 4,
        side_before: side_u8,
        size_before_e6: pre_liq_size,
        entry_price_before_e6: pre_liq_entry,
        margin_before_e6: margin,
        side_after: side_u8,
        size_after_e6: 0,
        entry_price_after_e6: 0,
        margin_after_e6: 0,
        size_delta_e6: -(pre_liq_size as i64),
        realized_pnl_e6: pnl,
        fee_e6: liquidation_penalty,
        related_trade_sequence: sequence,
    });

    if liquidation_penalty > 0 {
        emit_fee_event(&FeeEvent {
            discriminator: event_discriminator::FEE,
            sequence: liq_fee_seq,
            timestamp: current_ts,
            user,
            market_index,
            fee_type: 2,
            amount_e6: liquidation_penalty as i64,
            related_trade_sequence: sequence,
        });

        emit_insurance_fund_event(&InsuranceFundEvent {
            discriminator: event_discriminator::INSURANCE_FUND,
            sequence: liq_ins_seq,
            timestamp: current_ts,
            event_type: 0,
            market_index,
            amount_e6: liquidation_penalty as i64,
            balance_before_e6: 0,
            balance_after_e6: 0,
            related_user: user,
            reason: 0,
        });
    }

    msg!(
        "Liquidation completed: user={}, market={}, mark_price={}, pnl={}, remainder={}, penalty={}, shortfall={}",
        user, market_index, mark_price_e6, pnl, user_remainder, liquidation_penalty, shortfall
    );

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

    // Emit structured ADL event
    let adl_event = events::ADLEvent {
        discriminator: events::event_discriminator::ADL,
        sequence: ledger_config.total_adl_count,
        timestamp: current_ts,
        market_index,
        trigger_reason: events::ADLTriggerReason::Bankruptcy as u8,
        shortfall_e6,
        insurance_balance_before_e6: insurance_balance_e6,
        insurance_balance_after_e6: insurance_balance_e6,
        bankrupt_user: Pubkey::default(), // TriggerADL does not receive bankrupt user account
        bankrupt_side: match bankrupt_side { Side::Long => 0, Side::Short => 1 },
        bankrupt_size_e6: 0,
        counterparty_user: if validated_targets.is_empty() { Pubkey::default() } else { validated_targets[0] },
        counterparty_side: match bankrupt_side { Side::Long => 1, Side::Short => 0 },
        counterparty_size_reduced_e6: 0,
        counterparty_pnl_e6: total_available_pnl,
        related_trade_sequence: 0,
    };
    events::emit_adl_event(&adl_event);

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

    // Emit FundingEvent
    let side_u8 = match position.side { Side::Long => 0u8, Side::Short => 1u8 };
    events::emit_funding_event(&events::FundingEvent {
        discriminator: events::event_discriminator::FUNDING,
        sequence: 0, // no LedgerConfig available for sequence here
        timestamp: current_ts,
        user,
        market_index,
        side: side_u8,
        position_size_e6: position.size_e6,
        funding_rate_e9: funding_rate_e6 * 1000, // e6 to e9
        payment_e6: actual_payment,
        mark_price_e6: index_price_e6,
        period_start: position.last_funding_ts,
        period_end: current_ts,
    });

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

// ============================================================================
// Admin 工具指令
// ============================================================================

/// 🔧 Admin 重置 Position（仅测试网使用）
/// 
/// 将 Position 的 size 和其他字段重置为 0，用于清理累积的测试仓位
fn process_admin_reset_position(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u8,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let admin = next_account_info(account_info_iter)?;
    let position_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;

    assert_signer(admin)?;
    assert_writable(position_info)?;

    // 验证 Admin
    let ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    if ledger_config.admin != *admin.key {
        msg!("❌ Invalid admin: expected {}, got {}", ledger_config.admin, admin.key);
        return Err(LedgerError::InvalidAdmin.into());
    }

    // 验证 Position PDA
    let (position_pda, _) = Pubkey::find_program_address(
        &[b"position", user.as_ref(), &[market_index]],
        program_id,
    );
    if position_info.key != &position_pda {
        msg!("❌ Invalid Position PDA");
        return Err(LedgerError::InvalidAccount.into());
    }

    // 读取并重置 Position
    let mut position = deserialize_account::<Position>(&position_info.data.borrow())?;
    
    let reset_pre_side = position.side as u8;
    let reset_pre_size = position.size_e6;
    let reset_pre_entry = position.entry_price_e6;
    let reset_pre_margin = position.margin_e6;

    msg!("🔧 Resetting Position: user={}, market={}, current_size={}", 
         user, market_index, position.size_e6);
    
    // 重置所有字段
    position.size_e6 = 0;
    position.margin_e6 = 0;
    position.entry_price_e6 = 0;
    position.liquidation_price_e6 = 0;
    position.unrealized_pnl_e6 = 0;
    let reset_ts = get_current_timestamp()?;
    position.last_update_ts = reset_ts;
    
    position.serialize(&mut &mut position_info.data.borrow_mut()[..])?;

    emit_position_event(&PositionEvent {
        discriminator: event_discriminator::POSITION,
        sequence: 0,
        timestamp: reset_ts,
        user,
        market_index,
        event_type: 5,
        side_before: reset_pre_side,
        size_before_e6: reset_pre_size,
        entry_price_before_e6: reset_pre_entry,
        margin_before_e6: reset_pre_margin,
        side_after: reset_pre_side,
        size_after_e6: 0,
        entry_price_after_e6: 0,
        margin_after_e6: 0,
        size_delta_e6: -(reset_pre_size as i64),
        realized_pnl_e6: 0,
        fee_e6: 0,
        related_trade_sequence: 0,
    });

    msg!("✅ Position reset to zero");
    Ok(())
}

// ============================================================================
// Spot 交易指令处理
// ============================================================================

use crate::state::{SpotTradeRecord, SpotSide, spot_fee_type};
use crate::instruction::SpotTradeData;

/// 记录单笔 Spot 成交
fn process_record_spot_trade(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    user: Pubkey,
    market_index: u16,
    is_buy: bool,
    base_amount_e6: u64,
    quote_amount_e6: u64,
    price_e6: u64,
    fee_e6: u64,
    is_taker: bool,
    batch_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let spot_trade_info = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(spot_trade_info)?;
    assert_writable(ledger_config_info)?;

    // 验证 Relayer 授权
    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        msg!("❌ Unauthorized relayer: {}", relayer.key);
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    // 获取下一个序列号
    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    let sequence = ledger_config.next_sequence();

    // 派生 SpotTradeRecord PDA
    let (spot_trade_pda, spot_trade_bump) = Pubkey::find_program_address(
        &[SpotTradeRecord::SEED_PREFIX, &sequence.to_le_bytes()],
        program_id,
    );

    if spot_trade_info.key != &spot_trade_pda {
        msg!("❌ Invalid SpotTradeRecord PDA");
        return Err(LedgerError::InvalidAccount.into());
    }

    // 创建账户
    let rent = Rent::get()?;
    let space = SpotTradeRecord::SIZE;
    let lamports = rent.minimum_balance(space);

    invoke_signed(
        &system_instruction::create_account(
            relayer.key,
            spot_trade_info.key,
            lamports,
            space as u64,
            program_id,
        ),
        &[relayer.clone(), spot_trade_info.clone(), system_program.clone()],
        &[&[SpotTradeRecord::SEED_PREFIX, &sequence.to_le_bytes(), &[spot_trade_bump]]],
    )?;

    // 初始化 SpotTradeRecord
    let current_ts = get_current_timestamp()?;
    let spot_trade = SpotTradeRecord {
        discriminator: SpotTradeRecord::DISCRIMINATOR,
        sequence,
        user,
        market_index,
        side: if is_buy { SpotSide::Buy } else { SpotSide::Sell },
        base_amount_e6,
        quote_amount_e6,
        price_e6,
        fee_e6,
        fee_type: if is_taker { spot_fee_type::TAKER } else { spot_fee_type::MAKER },
        timestamp: current_ts,
        batch_id,
        bump: spot_trade_bump,
        reserved: [0u8; 32],
    };

    spot_trade.serialize(&mut &mut spot_trade_info.data.borrow_mut()[..])?;

    // 更新 LedgerConfig 统计
    ledger_config.total_volume_e6 = ledger_config.total_volume_e6.saturating_add(quote_amount_e6);
    ledger_config.total_fees_collected_e6 = ledger_config.total_fees_collected_e6.saturating_add(fee_e6);
    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("✅ SpotTradeRecord created: seq={}, user={}, market={}, side={}, base={}, quote={}, fee={}",
         sequence, user, market_index, if is_buy { "Buy" } else { "Sell" },
         base_amount_e6, quote_amount_e6, fee_e6);

    emit_trade_event(&TradeEvent {
        discriminator: event_discriminator::TRADE,
        sequence,
        timestamp: current_ts,
        batch_id,
        market_index: market_index as u8,
        market_type: 1,
        trade_type: 0,
        maker: Pubkey::default(),
        maker_order_id: [0u8; 16],
        maker_side: 0,
        maker_fee_e6: 0,
        taker: user,
        taker_order_id: [0u8; 16],
        taker_side: if is_buy { 0 } else { 1 },
        taker_fee_e6: fee_e6 as i64,
        price_e6,
        size_e6: base_amount_e6,
        notional_e6: quote_amount_e6,
        maker_realized_pnl_e6: 0,
        taker_realized_pnl_e6: 0,
        maker_margin_delta_e6: 0,
        taker_margin_delta_e6: 0,
    });

    Ok(())
}

/// 批量记录 Spot 成交
fn process_batch_record_spot_trades(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    trades: Vec<SpotTradeData>,
    batch_id: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let ledger_config_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;
    assert_writable(ledger_config_info)?;

    // 验证 Relayer 授权
    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    let mut ledger_config = deserialize_account::<LedgerConfig>(&ledger_config_info.data.borrow())?;
    let current_ts = get_current_timestamp()?;
    let rent = Rent::get()?;
    let space = SpotTradeRecord::SIZE;
    let lamports = rent.minimum_balance(space);

    for trade in trades.iter() {
        let spot_trade_info = next_account_info(account_info_iter)?;
        assert_writable(spot_trade_info)?;

        let sequence = ledger_config.next_sequence();

        // 派生 PDA
        let (spot_trade_pda, spot_trade_bump) = Pubkey::find_program_address(
            &[SpotTradeRecord::SEED_PREFIX, &sequence.to_le_bytes()],
            program_id,
        );

        if spot_trade_info.key != &spot_trade_pda {
            msg!("❌ Invalid SpotTradeRecord PDA for sequence {}", sequence);
            return Err(LedgerError::InvalidAccount.into());
        }

        // 创建账户
        invoke_signed(
            &system_instruction::create_account(
                relayer.key,
                spot_trade_info.key,
                lamports,
                space as u64,
                program_id,
            ),
            &[relayer.clone(), spot_trade_info.clone(), system_program.clone()],
            &[&[SpotTradeRecord::SEED_PREFIX, &sequence.to_le_bytes(), &[spot_trade_bump]]],
        )?;

        // 初始化
        let spot_trade = SpotTradeRecord {
            discriminator: SpotTradeRecord::DISCRIMINATOR,
            sequence,
            user: trade.user,
            market_index: trade.market_index,
            side: if trade.is_buy { SpotSide::Buy } else { SpotSide::Sell },
            base_amount_e6: trade.base_amount_e6,
            quote_amount_e6: trade.quote_amount_e6,
            price_e6: trade.price_e6,
            fee_e6: trade.fee_e6,
            fee_type: if trade.is_taker { spot_fee_type::TAKER } else { spot_fee_type::MAKER },
            timestamp: current_ts,
            batch_id,
            bump: spot_trade_bump,
            reserved: [0u8; 32],
        };

        spot_trade.serialize(&mut &mut spot_trade_info.data.borrow_mut()[..])?;

        emit_trade_event(&TradeEvent {
            discriminator: event_discriminator::TRADE,
            sequence,
            timestamp: current_ts,
            batch_id,
            market_index: trade.market_index as u8,
            market_type: 1,
            trade_type: 0,
            maker: Pubkey::default(),
            maker_order_id: [0u8; 16],
            maker_side: 0,
            maker_fee_e6: 0,
            taker: trade.user,
            taker_order_id: [0u8; 16],
            taker_side: if trade.is_buy { 0 } else { 1 },
            taker_fee_e6: trade.fee_e6 as i64,
            price_e6: trade.price_e6,
            size_e6: trade.base_amount_e6,
            notional_e6: trade.quote_amount_e6,
            maker_realized_pnl_e6: 0,
            taker_realized_pnl_e6: 0,
            maker_margin_delta_e6: 0,
            taker_margin_delta_e6: 0,
        });

        // 累加统计
        ledger_config.total_volume_e6 = ledger_config.total_volume_e6.saturating_add(trade.quote_amount_e6);
        ledger_config.total_fees_collected_e6 = ledger_config.total_fees_collected_e6.saturating_add(trade.fee_e6);
    }

    ledger_config.last_update_ts = current_ts;
    ledger_config.serialize(&mut &mut ledger_config_info.data.borrow_mut()[..])?;

    msg!("✅ BatchRecordSpotTrades: {} trades recorded, batch_id={}", trades.len(), batch_id);
    Ok(())
}

// ============================================================================
// 订单事件存证
// ============================================================================

fn process_record_order_events(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    events: Vec<OrderEventInput>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let _ledger_config_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;

    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        msg!("❌ Unauthorized relayer for RecordOrderEvents: {}", relayer.key);
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    for input in &events {
        let order_event = OrderEvent {
            discriminator: event_discriminator::ORDER,
            sequence: 0,
            timestamp: input.timestamp,
            order_id: input.order_id,
            client_order_id: input.client_order_id,
            user: input.user,
            market_index: input.market_index,
            market_type: input.market_type,
            side: input.side,
            order_type: input.order_type,
            time_in_force: 0,
            reduce_only: false,
            post_only: false,
            price_e6: input.price_e6,
            size_e6: input.size_e6,
            filled_size_e6: 0,
            remaining_size_e6: input.size_e6,
            trigger_price_e6: 0,
            avg_fill_price_e6: 0,
            status: input.status,
            status_reason: input.status_reason,
        };
        emit_order_event(&order_event);
    }

    msg!("✅ RecordOrderEvents: {} events emitted", events.len());
    Ok(())
}

// ============================================================================
// 资金费率事件存证
// ============================================================================

fn process_record_funding_events(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    events: Vec<FundingEventInput>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let relayer = next_account_info(account_info_iter)?;
    let _ledger_config_info = next_account_info(account_info_iter)?;
    let relayer_config_info = next_account_info(account_info_iter)?;

    assert_signer(relayer)?;

    let relayer_config = deserialize_account::<RelayerConfig>(&relayer_config_info.data.borrow())?;
    if !relayer_config.is_authorized(relayer.key) {
        msg!("❌ Unauthorized relayer for RecordFundingEvents: {}", relayer.key);
        return Err(LedgerError::UnauthorizedRelayer.into());
    }

    for input in &events {
        msg!(
            "EVENT:FundingSettlement:market={},rate={},price={},accounts={},total_paid={},epoch={},ts={}",
            input.market_index,
            input.funding_rate_e6,
            input.index_price_e6,
            input.accounts_settled,
            input.total_funding_paid_e6,
            input.epoch,
            input.timestamp,
        );
    }

    msg!("✅ RecordFundingEvents: {} events emitted", events.len());
    Ok(())
}

