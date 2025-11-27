//! Ledger Program Utility Functions

use crate::error::LedgerError;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::Sysvar,
};

/// 验证账户是签名者
pub fn assert_signer(account: &AccountInfo) -> ProgramResult {
    if !account.is_signer {
        return Err(LedgerError::MissingRequiredSignature.into());
    }
    Ok(())
}

/// 验证账户可写
pub fn assert_writable(account: &AccountInfo) -> ProgramResult {
    if !account.is_writable {
        return Err(LedgerError::AccountNotWritable.into());
    }
    Ok(())
}

/// 验证账户所有者
pub fn assert_owned_by(account: &AccountInfo, owner: &Pubkey) -> ProgramResult {
    if account.owner != owner {
        return Err(LedgerError::InvalidAccount.into());
    }
    Ok(())
}

/// 验证 PDA 地址
pub fn assert_pda(
    account: &AccountInfo,
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<u8, ProgramError> {
    let (expected_pda, bump) = Pubkey::find_program_address(seeds, program_id);
    if account.key != &expected_pda {
        return Err(LedgerError::InvalidAccount.into());
    }
    Ok(bump)
}

/// 安全加法 (i64)
pub fn checked_add(a: i64, b: i64) -> Result<i64, ProgramError> {
    a.checked_add(b).ok_or(LedgerError::Overflow.into())
}

/// 安全减法 (i64)
pub fn checked_sub(a: i64, b: i64) -> Result<i64, ProgramError> {
    a.checked_sub(b).ok_or(LedgerError::Overflow.into())
}

/// 安全乘法 (i64)
pub fn checked_mul(a: i64, b: i64) -> Result<i64, ProgramError> {
    a.checked_mul(b).ok_or(LedgerError::Overflow.into())
}

/// 安全除法 (i64)
pub fn checked_div(a: i64, b: i64) -> Result<i64, ProgramError> {
    if b == 0 {
        return Err(LedgerError::Overflow.into());
    }
    a.checked_div(b).ok_or(LedgerError::Overflow.into())
}

/// 安全加法 (u64)
pub fn checked_add_u64(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(LedgerError::Overflow.into())
}

/// 安全减法 (u64)
pub fn checked_sub_u64(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_sub(b).ok_or(LedgerError::Overflow.into())
}

/// 安全乘法 (u64)
pub fn checked_mul_u64(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_mul(b).ok_or(LedgerError::Overflow.into())
}

/// 安全除法 (u64)
pub fn checked_div_u64(a: u64, b: u64) -> Result<u64, ProgramError> {
    if b == 0 {
        return Err(LedgerError::Overflow.into());
    }
    a.checked_div(b).ok_or(LedgerError::Overflow.into())
}

/// e6 精度乘法: (a * b) / 1_000_000
pub fn mul_e6(a: i64, b: i64) -> Result<i64, ProgramError> {
    let result = (a as i128)
        .checked_mul(b as i128)
        .ok_or(LedgerError::Overflow)?;
    let result = result.checked_div(1_000_000).ok_or(LedgerError::Overflow)?;
    i64::try_from(result).map_err(|_| LedgerError::Overflow.into())
}

/// e6 精度除法: (a * 1_000_000) / b
pub fn div_e6(a: i64, b: i64) -> Result<i64, ProgramError> {
    if b == 0 {
        return Err(LedgerError::Overflow.into());
    }
    let result = (a as i128)
        .checked_mul(1_000_000)
        .ok_or(LedgerError::Overflow)?;
    let result = result.checked_div(b as i128).ok_or(LedgerError::Overflow)?;
    i64::try_from(result).map_err(|_| LedgerError::Overflow.into())
}

/// 计算数据哈希 (SHA256)
/// 
/// 注意: 这是基础版本，仅用于简单的数据完整性校验。
/// 对于需要防重放攻击的场景，请使用 `compute_batch_hash`。
pub fn compute_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// 计算批次数据哈希 (SHA256 with domain separation)
/// 
/// 包含批次 ID 和程序 ID 作为 domain separator，防止重放攻击：
/// - 相同的交易数据在不同批次中产生不同的哈希
/// - 跨程序的哈希不能互用
/// 
/// Hash = SHA256(DOMAIN_PREFIX || program_id || batch_id || data)
pub fn compute_batch_hash(
    program_id: &Pubkey,
    batch_id: u64,
    data: &[u8],
) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    
    // Domain separator 防止跨协议重放
    const DOMAIN_PREFIX: &[u8] = b"1024_LEDGER_BATCH_V1";
    
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_PREFIX);
    hasher.update(program_id.as_ref()); // 32 bytes
    hasher.update(batch_id.to_le_bytes()); // 8 bytes
    hasher.update(data);
    hasher.finalize().into()
}

/// 验证批次数据哈希
pub fn verify_batch_hash(
    program_id: &Pubkey,
    batch_id: u64,
    data: &[u8],
    expected_hash: &[u8; 32],
) -> bool {
    let computed = compute_batch_hash(program_id, batch_id, data);
    // 使用常量时间比较防止时序攻击
    constant_time_compare(&computed, expected_hash)
}

/// 常量时间比较 (防止时序攻击)
fn constant_time_compare(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// 获取当前时间戳
pub fn get_current_timestamp() -> Result<i64, ProgramError> {
    Ok(solana_program::clock::Clock::get()?.unix_timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mul_e6() {
        // 100.5 * 2.0 = 201.0
        let a = 100_500_000i64; // 100.5 in e6
        let b = 2_000_000i64;   // 2.0 in e6
        let result = mul_e6(a, b).unwrap();
        assert_eq!(result, 201_000_000); // 201.0 in e6
    }

    #[test]
    fn test_div_e6() {
        // 100.0 / 2.0 = 50.0
        let a = 100_000_000i64; // 100.0 in e6
        let b = 2_000_000i64;   // 2.0 in e6
        let result = div_e6(a, b).unwrap();
        assert_eq!(result, 50_000_000); // 50.0 in e6
    }

    #[test]
    fn test_compute_hash() {
        let data = b"test data";
        let hash = compute_hash(data);
        assert_eq!(hash.len(), 32);
        // 同样的数据应该产生同样的哈希
        let hash2 = compute_hash(data);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_compute_batch_hash() {
        let program_id = Pubkey::new_unique();
        let batch_id = 12345u64;
        let data = b"trade data";
        
        let hash = compute_batch_hash(&program_id, batch_id, data);
        assert_eq!(hash.len(), 32);
        
        // 同样的数据应该产生同样的哈希
        let hash2 = compute_batch_hash(&program_id, batch_id, data);
        assert_eq!(hash, hash2);
        
        // 不同的 batch_id 应该产生不同的哈希 (防重放)
        let hash3 = compute_batch_hash(&program_id, batch_id + 1, data);
        assert_ne!(hash, hash3);
        
        // 不同的 program_id 应该产生不同的哈希 (跨程序隔离)
        let other_program = Pubkey::new_unique();
        let hash4 = compute_batch_hash(&other_program, batch_id, data);
        assert_ne!(hash, hash4);
    }

    #[test]
    fn test_verify_batch_hash() {
        let program_id = Pubkey::new_unique();
        let batch_id = 12345u64;
        let data = b"trade data";
        
        let hash = compute_batch_hash(&program_id, batch_id, data);
        
        // 验证正确的哈希
        assert!(verify_batch_hash(&program_id, batch_id, data, &hash));
        
        // 验证错误的 batch_id
        assert!(!verify_batch_hash(&program_id, batch_id + 1, data, &hash));
        
        // 验证错误的数据
        assert!(!verify_batch_hash(&program_id, batch_id, b"wrong data", &hash));
    }

    #[test]
    fn test_constant_time_compare() {
        let a = [1u8; 32];
        let b = [1u8; 32];
        let c = [2u8; 32];
        
        assert!(constant_time_compare(&a, &b));
        assert!(!constant_time_compare(&a, &c));
    }
}

