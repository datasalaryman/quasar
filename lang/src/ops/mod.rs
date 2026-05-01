//! Op-dispatch for account lifecycle operations.
//!
//! The derive emits direct capability trait calls for validation, init
//! contribution, and exit actions. Structural ops (init, realloc, PDA
//! verification) use their own inherent methods.

pub mod close_program;
pub mod init;
pub mod pda;
pub mod realloc;

use solana_program_error::ProgramError;

/// Runtime context shared across all op calls within a single parse invocation.
///
/// Carries `program_id` (always available) and optionally pre-populated `Rent`.
pub struct OpCtx<'a> {
    pub program_id: &'a solana_address::Address,
    rent: Option<crate::sysvars::rent::Rent>,
}

impl<'a> OpCtx<'a> {
    #[inline(always)]
    pub fn new(program_id: &'a solana_address::Address) -> Self {
        Self {
            program_id,
            rent: None,
        }
    }

    /// Create with pre-populated rent (avoids syscall when Sysvar<Rent> is
    /// available).
    #[inline(always)]
    pub fn new_with_rent(
        program_id: &'a solana_address::Address,
        rent: crate::sysvars::rent::Rent,
    ) -> Self {
        Self {
            program_id,
            rent: Some(rent),
        }
    }

    /// Create with rent fetched from sysvar (when no Sysvar<Rent> field
    /// is available).
    #[inline(always)]
    pub fn new_fetch_rent(program_id: &'a solana_address::Address) -> Result<Self, ProgramError> {
        let rent = <crate::sysvars::rent::Rent as crate::sysvars::Sysvar>::get()?;
        Ok(Self {
            program_id,
            rent: Some(rent),
        })
    }

    /// Get rent. Always populated at construction time.
    #[inline(always)]
    pub fn rent(&self) -> Result<&crate::sysvars::rent::Rent, ProgramError> {
        match self.rent {
            Some(ref r) => Ok(r),
            None => unsafe { core::hint::unreachable_unchecked() },
        }
    }
}

/// Marker trait for account types that support realloc.
///
/// The `realloc::Op` requires `F: SupportsRealloc` to ensure only
/// realloc-capable accounts are used with `realloc(...)`.
pub trait SupportsRealloc {}
