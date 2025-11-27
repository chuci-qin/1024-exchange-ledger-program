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
pub fn compute_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
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
}

