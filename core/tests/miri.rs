//! Miri UB tests for quasar-core unsafe code paths.
//!
//! These tests are designed to FIND undefined behavior, not confirm correct
//! output. Each test exercises a specific unsafe pattern under conditions
//! that would trigger Miri if the pattern is unsound.
//!
//! ## Run
//!
//! ```sh
//! MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-symbolic-alignment-check" \
//!   cargo +nightly miri test -p quasar-core --test miri
//! ```
//!
//! ## Flags
//!
//! - `-Zmiri-tree-borrows`: Tree Borrows model. The `& → &mut` cast in
//!   `from_account_view_mut` is instant UB under Stacked Borrows. Under Tree
//!   Borrows it is sound because the `&mut Account<T>` never writes to the
//!   AccountView memory itself — writes go through the raw pointer to a
//!   separate RuntimeAccount allocation. The retag creates a "Reserved" child
//!   that never transitions to "Active".
//! - `-Zmiri-symbolic-alignment-check`: Catch alignment issues that depend on
//!   allocation placement rather than happenstance.
//!
//! ## Findings
//!
//! | Pattern | Result |
//! |---------|--------|
//! | `& → &mut` cast (`from_account_view_mut`) | Sound under Tree Borrows |
//! | `& → &mut` cast (`Initialize`, `define_account!`) | Sound under Tree Borrows |
//! | DerefMut write + aliased read via &AccountView | Sound under Tree Borrows |
//! | Interleaved shared/mutable access | Sound under Tree Borrows |
//! | `copy_nonoverlapping` 3-byte flag extraction | Sound |
//! | MaybeUninit array init + assume_init | Sound |
//! | Event memcpy from repr(C) (no padding) | Sound |
//! | `assign` + `resize` + `close` raw pointer writes | Sound |
//! | `borrow_unchecked_mut` sequential borrows | Sound |
//! | CPI `create_account` data construction | Sound (was misaligned u32, fixed) |
//! | Boundary pointer subtraction (`data.as_ptr().sub(8)`) | Sound |
//! | Remaining accounts alignment rounding | **Provenance warning** — integer-to-pointer cast strips provenance. Fails under `-Zmiri-strict-provenance`. Not UB under default provenance model. |
//!
//! ## What Miri CANNOT test
//!
//! | Pattern | Why |
//! |---------|-----|
//! | `sol_invoke_signed_c` syscall | FFI, SBF-only |
//! | `sol_get_sysvar` syscall | FFI, SBF-only |
//! | Full dispatch loop | Requires SVM buffer from runtime |

use std::mem::{align_of, size_of, MaybeUninit};

use quasar_core::__internal::{
    AccountView, RuntimeAccount, MAX_PERMITTED_DATA_INCREASE, NOT_BORROWED,
};
use quasar_core::accounts::{Account, Initialize, Signer as SignerAccount, UncheckedAccount};
use quasar_core::cpi::{CpiCall, InstructionAccount};
use quasar_core::pod::*;
use quasar_core::remaining::RemainingAccounts;
use quasar_core::traits::*;
use solana_address::Address;
use solana_program_error::ProgramError;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// 8-byte-aligned buffer for constructing RuntimeAccount + data.
///
/// Uses `Vec<u64>` to guarantee alignment >= 8, which satisfies
/// RuntimeAccount's alignment requirement.
struct AccountBuffer {
    inner: Vec<u64>,
}

impl AccountBuffer {
    fn new(data_len: usize) -> Self {
        let byte_len =
            size_of::<RuntimeAccount>() + data_len + MAX_PERMITTED_DATA_INCREASE + size_of::<u64>();
        let u64_count = (byte_len + 7) / 8;
        Self {
            inner: vec![0; u64_count],
        }
    }

    /// Allocation with exact byte count (no extra slack beyond alignment padding).
    fn exact(byte_len: usize) -> Self {
        let u64_count = (byte_len + 7) / 8;
        Self {
            inner: vec![0; u64_count],
        }
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.inner.as_mut_ptr() as *mut u8
    }

    fn raw(&mut self) -> *mut RuntimeAccount {
        self.inner.as_mut_ptr() as *mut RuntimeAccount
    }

    fn init(
        &mut self,
        address: [u8; 32],
        owner: [u8; 32],
        lamports: u64,
        data_len: u64,
        is_signer: bool,
        is_writable: bool,
    ) {
        let raw = self.raw();
        unsafe {
            (*raw).borrow_state = NOT_BORROWED;
            (*raw).is_signer = is_signer as u8;
            (*raw).is_writable = is_writable as u8;
            (*raw).executable = 0;
            (*raw).resize_delta = 0;
            (*raw).address = Address::new_from_array(address);
            (*raw).owner = Address::new_from_array(owner);
            (*raw).lamports = lamports;
            (*raw).data_len = data_len;
        }
    }

    unsafe fn view(&mut self) -> AccountView {
        AccountView::new_unchecked(self.raw())
    }

    fn write_data(&mut self, data: &[u8]) {
        let data_start = size_of::<RuntimeAccount>();
        let dst = unsafe {
            std::slice::from_raw_parts_mut(self.as_mut_ptr().add(data_start), data.len())
        };
        dst.copy_from_slice(data);
    }
}

/// Multi-account buffer for remaining accounts tests.
struct MultiAccountBuffer {
    inner: Vec<u64>,
}

const ACCOUNT_HEADER: usize =
    size_of::<RuntimeAccount>() + MAX_PERMITTED_DATA_INCREASE + size_of::<u64>();

impl MultiAccountBuffer {
    fn new(accounts: &[MultiAccountEntry]) -> Self {
        let total_bytes: usize = accounts
            .iter()
            .map(|entry| match entry {
                MultiAccountEntry::Full {
                    data_len, data, ..
                } => {
                    let raw_len = ACCOUNT_HEADER + data.as_ref().map_or(*data_len, |d| d.len());
                    (raw_len + 7) & !7
                }
                MultiAccountEntry::Duplicate { .. } => size_of::<u64>(),
            })
            .sum();
        let u64_count = (total_bytes + 7) / 8;
        let mut buf = Self {
            inner: vec![0; u64_count],
        };
        buf.populate(accounts);
        buf
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.inner.as_mut_ptr() as *mut u8
    }

    fn boundary(&self) -> *const u8 {
        unsafe { (self.inner.as_ptr() as *const u8).add(self.inner.len() * size_of::<u64>()) }
    }

    fn populate(&mut self, accounts: &[MultiAccountEntry]) {
        let base = self.as_mut_ptr();
        let mut offset = 0usize;
        for entry in accounts {
            match entry {
                MultiAccountEntry::Full {
                    address,
                    owner,
                    lamports,
                    data_len,
                    data,
                    is_signer,
                    is_writable,
                } => {
                    let raw = unsafe { &mut *(base.add(offset) as *mut RuntimeAccount) };
                    raw.borrow_state = NOT_BORROWED;
                    raw.is_signer = *is_signer as u8;
                    raw.is_writable = *is_writable as u8;
                    raw.executable = 0;
                    raw.resize_delta = 0;
                    raw.address = Address::new_from_array(*address);
                    raw.owner = Address::new_from_array(*owner);
                    raw.lamports = *lamports;
                    let actual_data_len = data.as_ref().map_or(*data_len, |d| d.len());
                    raw.data_len = actual_data_len as u64;

                    if let Some(d) = data {
                        let data_start = offset + size_of::<RuntimeAccount>();
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                d.as_ptr(),
                                base.add(data_start),
                                d.len(),
                            );
                        }
                    }

                    let raw_len = ACCOUNT_HEADER + actual_data_len;
                    offset += (raw_len + 7) & !7;
                }
                MultiAccountEntry::Duplicate { original_index } => {
                    unsafe { *base.add(offset) = *original_index as u8 };
                    offset += size_of::<u64>();
                }
            }
        }
    }
}

enum MultiAccountEntry {
    Full {
        address: [u8; 32],
        owner: [u8; 32],
        lamports: u64,
        data_len: usize,
        data: Option<Vec<u8>>,
        is_signer: bool,
        is_writable: bool,
    },
    Duplicate {
        original_index: usize,
    },
}

impl MultiAccountEntry {
    fn account(address_byte: u8, data_len: usize) -> Self {
        MultiAccountEntry::Full {
            address: [address_byte; 32],
            owner: [0xAA; 32],
            lamports: 1_000_000,
            data_len,
            data: None,
            is_signer: false,
            is_writable: true,
        }
    }

    fn duplicate(original_index: usize) -> Self {
        MultiAccountEntry::Duplicate { original_index }
    }
}

// ---------------------------------------------------------------------------
// Test-only types for Account<T> transparent cast tests
// ---------------------------------------------------------------------------

#[repr(C)]
struct TestZcData {
    value: PodU64,
    flag: PodBool,
}

const _: () = assert!(align_of::<TestZcData>() == 1);
const _: () = assert!(size_of::<TestZcData>() == 9);

struct TestAccountType;

const TEST_OWNER: Address = Address::new_from_array([42u8; 32]);

impl Owner for TestAccountType {
    const OWNER: Address = TEST_OWNER;
}

impl AccountCheck for TestAccountType {
    fn check(_view: &AccountView) -> Result<(), ProgramError> {
        Ok(())
    }
}

impl ZeroCopyDeref for TestAccountType {
    type Target = TestZcData;
    const DATA_OFFSET: usize = 4; // discriminator length
}

// ===========================================================================
// 1. The & -> &mut cast (THE critical pattern)
//
// Account::from_account_view_mut takes &AccountView and returns &mut Self.
// This is the pattern every Solana framework uses. Under Stacked Borrows
// it's instant UB. Under Tree Borrows it MIGHT be sound because the &mut
// only touches the raw pointer value (never writes to it — writes go through
// the pointer to SVM memory). These tests probe whether Tree Borrows agrees.
// ===========================================================================

#[test]
fn shared_to_mut_cast_then_read_lamports() {
    // Probe: create &AccountView, cast to &mut Account<T>, read lamports
    // through the &mut path. The read goes through the raw pointer inside
    // AccountView to the RuntimeAccount buffer — a different allocation
    // from the AccountView itself.
    let mut buf = AccountBuffer::new(64);
    buf.init([1u8; 32], TEST_OWNER.to_bytes(), 500_000, 64, true, true);

    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // Read through the &mut Account<T> path
    assert_eq!(account.to_account_view().lamports(), 500_000);
}

#[test]
fn shared_to_mut_cast_then_write_lamports() {
    // Probe: cast to &mut, then WRITE through set_lamports.
    // set_lamports writes to the RuntimeAccount buffer (different allocation),
    // NOT to the AccountView's memory. Tree Borrows should allow this because
    // the &mut Account<T> never actually writes to the AccountView pointer value.
    let mut buf = AccountBuffer::new(64);
    buf.init([1u8; 32], TEST_OWNER.to_bytes(), 100, 64, true, true);

    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    account.to_account_view().set_lamports(999);
    assert_eq!(account.to_account_view().lamports(), 999);
}

#[test]
fn shared_to_mut_cast_then_read_original_view() {
    // Probe: cast to &mut Account<T>, THEN read through the original &AccountView.
    // This is the aliasing pattern: &view and &mut account point to the same
    // AccountView memory. Under Tree Borrows, reading through the parent (&view)
    // after creating a child (&mut account) may or may not be UB depending on
    // whether the child ever performed a "write" to that memory.
    let mut buf = AccountBuffer::new(64);
    buf.init([1u8; 32], TEST_OWNER.to_bytes(), 100, 64, true, true);

    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // Write through the &mut path (to RuntimeAccount, not AccountView)
    account.to_account_view().set_lamports(777);

    // Read through the ORIGINAL &view — does this alias conflict?
    assert_eq!(view.lamports(), 777);
}

#[test]
fn shared_to_mut_cast_interleaved_access() {
    // Probe: alternate reads between &view and &mut account.
    // This is the real instruction-handler pattern: you have both references
    // alive and use them interchangeably.
    let mut buf = AccountBuffer::new(64);
    buf.init([1u8; 32], TEST_OWNER.to_bytes(), 100, 64, true, true);

    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // Read through &mut
    let l1 = account.to_account_view().lamports();
    // Read through &
    let l2 = view.lamports();
    assert_eq!(l1, l2);

    // Write through &mut
    account.to_account_view().set_lamports(200);
    // Read through &
    assert_eq!(view.lamports(), 200);
    // Read through &mut
    assert_eq!(account.to_account_view().lamports(), 200);

    // Write through & (AccountView has interior mutability)
    view.set_lamports(300);
    // Read through &mut
    assert_eq!(account.to_account_view().lamports(), 300);
}

// ===========================================================================
// 2. DerefMut — zero-copy write through Account<T>
//
// Account<T>::deref_mut() does:
//   &mut *(self.data_ptr().add(DATA_OFFSET) as *mut T::Target)
// This creates a &mut to data INSIDE the SVM buffer. The pointer arithmetic
// and the cast to T::Target could be UB if alignment/bounds are wrong.
// ===========================================================================

fn make_zc_buffer() -> AccountBuffer {
    let disc_len = 4;
    let data_len = disc_len + size_of::<TestZcData>();
    let mut buf = AccountBuffer::new(data_len);
    buf.init(
        [1u8; 32],
        TEST_OWNER.to_bytes(),
        1_000_000,
        data_len as u64,
        true,
        true,
    );
    // Write discriminator
    let mut data = vec![0u8; data_len];
    data[..disc_len].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    data[disc_len..disc_len + 8].copy_from_slice(&42u64.to_le_bytes());
    data[disc_len + 8] = 1; // PodBool true
    buf.write_data(&data);
    buf
}

#[test]
fn deref_read_zc_fields() {
    // Baseline: Deref (read) through Account<T> to ZC fields.
    let mut buf = make_zc_buffer();
    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view(&view).unwrap();

    let zc: &TestZcData = &*account;
    assert_eq!(zc.value.get(), 42);
    assert!(zc.flag.get());
}

#[test]
fn deref_mut_write_zc_fields() {
    // Probe: DerefMut (write) through &mut Account<T> to ZC fields.
    // This creates &mut TestZcData pointing into the SVM buffer.
    let mut buf = make_zc_buffer();
    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    let zc: &mut TestZcData = &mut *account;
    zc.value = PodU64::from(999u64);
    zc.flag = PodBool::from(false);

    // Verify the write landed in the buffer
    assert_eq!(zc.value.get(), 999);
    assert!(!zc.flag.get());
}

#[test]
fn deref_mut_write_then_read_via_view() {
    // Probe: write through DerefMut, then read the same bytes through the
    // original AccountView's data pointer. Tests whether the write through
    // &mut TestZcData aliases with reads through &AccountView.
    let mut buf = make_zc_buffer();
    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // Write through DerefMut
    let zc: &mut TestZcData = &mut *account;
    zc.value = PodU64::from(12345u64);

    // Read the same bytes through view.borrow_unchecked()
    let data = unsafe { view.borrow_unchecked() };
    let written = u64::from_le_bytes(data[4..12].try_into().unwrap());
    assert_eq!(written, 12345);
}

#[test]
fn deref_mut_write_then_deref_read() {
    // Probe: write through DerefMut, drop the &mut, then Deref (read).
    // The &mut TestZcData and &TestZcData point to the same memory.
    let mut buf = make_zc_buffer();
    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // Write
    {
        let zc: &mut TestZcData = &mut *account;
        zc.value = PodU64::from(7777u64);
    }

    // Read via Deref (not DerefMut)
    let zc: &TestZcData = &*account;
    assert_eq!(zc.value.get(), 7777);
}

#[test]
fn multiple_deref_mut_calls() {
    // Probe: call deref_mut() multiple times on the same Account.
    // Each call creates a new &mut TestZcData. If Miri tracks the previous
    // &mut as still-live, this could trigger an aliasing violation.
    let mut buf = make_zc_buffer();
    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view_mut(&view).unwrap();

    // First DerefMut
    {
        let zc: &mut TestZcData = &mut *account;
        zc.value = PodU64::from(1u64);
    }
    // Second DerefMut
    {
        let zc: &mut TestZcData = &mut *account;
        assert_eq!(zc.value.get(), 1);
        zc.value = PodU64::from(2u64);
    }
    // Third DerefMut
    {
        let zc: &mut TestZcData = &mut *account;
        assert_eq!(zc.value.get(), 2);
    }
}

// ===========================================================================
// 3. Tight-buffer boundary conditions
//
// The previous tests used oversized buffers. These use exact-minimum-size
// buffers so any off-by-one in pointer arithmetic hits the allocation edge.
// ===========================================================================

#[test]
fn account_view_exact_size_buffer() {
    // Minimum buffer: RuntimeAccount header + data_len bytes.
    // No MAX_PERMITTED_DATA_INCREASE slack.
    let data_len = 16usize;
    let exact_size = size_of::<RuntimeAccount>() + data_len;
    let mut buf = AccountBuffer::exact(exact_size);
    buf.init([1u8; 32], [2u8; 32], 100, data_len as u64, false, true);

    let view = unsafe { buf.view() };

    // These reads must stay within the allocation
    assert_eq!(view.lamports(), 100);
    assert_eq!(view.data_len(), data_len);
    assert!(view.is_writable());
    assert_eq!(view.data_ptr(), unsafe {
        buf.as_mut_ptr().add(size_of::<RuntimeAccount>())
    });
}

#[test]
fn account_view_zero_data_len() {
    // data_len = 0: data_ptr() still valid (points to end of RuntimeAccount),
    // but borrow_unchecked() should return a zero-length slice.
    let mut buf = AccountBuffer::exact(size_of::<RuntimeAccount>());
    buf.init([0u8; 32], [0u8; 32], 0, 0, false, false);

    let view = unsafe { buf.view() };
    assert_eq!(view.data_len(), 0);

    let data = unsafe { view.borrow_unchecked() };
    assert_eq!(data.len(), 0);
}

#[test]
fn deref_exact_size_buffer() {
    // Buffer is exactly RuntimeAccount + discriminator + TestZcData.
    // No slack. The Deref pointer arithmetic must land exactly within bounds.
    let disc_len = 4;
    let data_len = disc_len + size_of::<TestZcData>();
    let exact_size = size_of::<RuntimeAccount>() + data_len;
    let mut buf = AccountBuffer::exact(exact_size);
    buf.init(
        [1u8; 32],
        TEST_OWNER.to_bytes(),
        100,
        data_len as u64,
        true,
        true,
    );
    let mut data = vec![0u8; data_len];
    data[..disc_len].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    data[disc_len..disc_len + 8].copy_from_slice(&99u64.to_le_bytes());
    data[disc_len + 8] = 1;
    buf.write_data(&data);

    let view = unsafe { buf.view() };
    let account = Account::<TestAccountType>::from_account_view(&view).unwrap();

    let zc: &TestZcData = &*account;
    assert_eq!(zc.value.get(), 99);
    assert!(zc.flag.get());
}

// ===========================================================================
// 4. CPI — RawCpiAccount::from_view via CpiCall::new
//
// from_view() does copy_nonoverlapping((raw as *const u8).add(1), &mut
// account.is_signer, 3) — copying 3 bytes from RuntimeAccount offset 1
// into the is_signer field of RawCpiAccount. The destination pointer is
// derived from &mut of a single u8 field but writes 3 bytes (into
// is_signer + is_writable + executable which are contiguous in repr(C)).
// ===========================================================================

#[test]
fn cpi_from_view_flag_extraction() {
    // Set specific flag patterns and verify the copy_nonoverlapping path
    // extracts them correctly.
    let mut buf = AccountBuffer::new(8);
    buf.init([1u8; 32], [2u8; 32], 100, 8, true, false);
    unsafe { (*buf.raw()).executable = 1 };

    let view = unsafe { buf.view() };
    let program_id = Address::new_from_array([0u8; 32]);

    // CpiCall::new calls RawCpiAccount::from_view internally.
    // If the 3-byte copy_nonoverlapping is UB, Miri catches it here.
    let _call: CpiCall<'_, 1, 1> = CpiCall::new(
        &program_id,
        [InstructionAccount::writable_signer(view.address())],
        [&view],
        [0u8],
    );

    // Construct with opposite flags to exercise different bit patterns
    let mut buf2 = AccountBuffer::new(0);
    buf2.init([2u8; 32], [3u8; 32], 0, 0, false, true);
    unsafe { (*buf2.raw()).executable = 0 };

    let view2 = unsafe { buf2.view() };
    let _call2: CpiCall<'_, 1, 1> = CpiCall::new(
        &program_id,
        [InstructionAccount::writable(view2.address())],
        [&view2],
        [0u8],
    );
}

#[test]
fn cpi_create_account_data_construction() {
    // create_account builds a 52-byte data buffer via MaybeUninit +
    // copy_nonoverlapping. Previously used a misaligned u32 write at offset 0.
    // Now uses copy_nonoverlapping for all fields. Verify no UB in the
    // data construction path.
    let mut from_buf = AccountBuffer::new(0);
    from_buf.init([1u8; 32], [0u8; 32], 1_000_000, 0, true, true);
    let mut to_buf = AccountBuffer::new(0);
    to_buf.init([2u8; 32], [0u8; 32], 0, 0, true, true);

    let from = unsafe { from_buf.view() };
    let to = unsafe { to_buf.view() };
    let owner = Address::new_from_array([0xAA; 32]);

    let _call = quasar_core::cpi::system::create_account(&from, &to, 500_000u64, 100, &owner);
}

#[test]
fn cpi_maybeuninit_multi_account() {
    // CpiCall::new with N accounts exercises the MaybeUninit loop:
    //   let mut arr = MaybeUninit::<[RawCpiAccount; N]>::uninit();
    //   for i in 0..N { ptr::write(ptr.add(i), from_view(views[i])) }
    //   arr.assume_init()
    // If any element is left uninitialized, Miri detects it at assume_init.
    let mut bufs: Vec<AccountBuffer> = (0..4)
        .map(|i| {
            let mut b = AccountBuffer::new(0);
            b.init([i as u8; 32], [0u8; 32], i as u64, 0, i % 2 == 0, i % 2 == 1);
            b
        })
        .collect();

    let views: Vec<AccountView> = bufs.iter_mut().map(|b| unsafe { b.view() }).collect();
    let program_id = Address::new_from_array([0u8; 32]);

    let _call: CpiCall<'_, 4, 1> = CpiCall::new(
        &program_id,
        [
            InstructionAccount::writable_signer(views[0].address()),
            InstructionAccount::writable(views[1].address()),
            InstructionAccount::readonly_signer(views[2].address()),
            InstructionAccount::readonly(views[3].address()),
        ],
        [&views[0], &views[1], &views[2], &views[3]],
        [0u8],
    );
}

// ===========================================================================
// 5. Remaining accounts — buffer walking with pointer arithmetic
//
// The walking code does:
//   ptr = ptr.add(ACCOUNT_HEADER + data_len)
//   ptr = ((ptr as usize + 7) & !7) as *mut u8  // align to 8
//
// The alignment rounding casts pointer → integer → pointer, which strips
// provenance. Miri warns about this but does not (currently) flag it as UB
// under default settings. Under -Zmiri-strict-provenance it WOULD fail.
// ===========================================================================

#[test]
fn remaining_walk_varied_data_lengths() {
    // Accounts with different data_len values exercise different pointer
    // advance distances. Non-8-aligned data_len values exercise the
    // alignment rounding path.
    let mut buf = MultiAccountBuffer::new(&[
        MultiAccountEntry::Full {
            address: [0x01; 32],
            owner: [0xAA; 32],
            lamports: 100,
            data_len: 1, // 1 byte — forces alignment padding
            data: Some(vec![0xFF]),
            is_signer: false,
            is_writable: true,
        },
        MultiAccountEntry::Full {
            address: [0x02; 32],
            owner: [0xBB; 32],
            lamports: 200,
            data_len: 7, // 7 bytes — misaligned, forces rounding
            data: Some(vec![0xEE; 7]),
            is_signer: true,
            is_writable: false,
        },
        MultiAccountEntry::Full {
            address: [0x03; 32],
            owner: [0xCC; 32],
            lamports: 300,
            data_len: 8, // exactly aligned
            data: Some(vec![0xDD; 8]),
            is_signer: false,
            is_writable: true,
        },
    ]);
    let remaining = RemainingAccounts::new(buf.as_mut_ptr(), buf.boundary(), &[]);

    let v0 = remaining.get(0).unwrap();
    assert_eq!(v0.lamports(), 100);
    assert_eq!(v0.data_len(), 1);

    let v1 = remaining.get(1).unwrap();
    assert_eq!(v1.lamports(), 200);
    assert_eq!(v1.data_len(), 7);

    let v2 = remaining.get(2).unwrap();
    assert_eq!(v2.lamports(), 300);
    assert_eq!(v2.data_len(), 8);

    assert!(remaining.get(3).is_none());
}

#[test]
fn remaining_iterator_varied_data_lengths() {
    // Same as above but through the iterator path, which uses its own
    // pointer arithmetic and MaybeUninit cache.
    let mut buf = MultiAccountBuffer::new(&[
        MultiAccountEntry::Full {
            address: [0x01; 32],
            owner: [0xAA; 32],
            lamports: 100,
            data_len: 3, // non-aligned
            data: Some(vec![0xFF; 3]),
            is_signer: false,
            is_writable: true,
        },
        MultiAccountEntry::Full {
            address: [0x02; 32],
            owner: [0xBB; 32],
            lamports: 200,
            data_len: 0, // zero
            data: None,
            is_signer: false,
            is_writable: true,
        },
        MultiAccountEntry::Full {
            address: [0x03; 32],
            owner: [0xCC; 32],
            lamports: 300,
            data_len: 15, // non-aligned
            data: Some(vec![0xDD; 15]),
            is_signer: false,
            is_writable: true,
        },
    ]);
    let remaining = RemainingAccounts::new(buf.as_mut_ptr(), buf.boundary(), &[]);

    let views: Vec<_> = remaining.iter().collect();
    assert_eq!(views.len(), 3);
    assert_eq!(views[0].data_len(), 3);
    assert_eq!(views[1].data_len(), 0);
    assert_eq!(views[2].data_len(), 15);
}

#[test]
fn remaining_duplicate_referencing_declared() {
    let mut declared_buf = AccountBuffer::new(0);
    declared_buf.init([0xDD; 32], [0xAA; 32], 777, 0, true, false);
    let declared_view = unsafe { declared_buf.view() };

    let mut buf = MultiAccountBuffer::new(&[
        MultiAccountEntry::account(0x01, 0),
        MultiAccountEntry::duplicate(0), // references declared[0]
    ]);
    let declared = [declared_view];
    let remaining = RemainingAccounts::new(buf.as_mut_ptr(), buf.boundary(), &declared);

    // get() path: resolve_dup_walk reads via ptr::read from declared slice
    let v1 = remaining.get(1).unwrap();
    assert_eq!(v1.address(), &Address::new_from_array([0xDD; 32]));
}

#[test]
fn remaining_iterator_dup_cache_resolution() {
    // Iterator: dup references an earlier remaining account (not declared).
    // This exercises the cache: ptr::write to cache on yield, ptr::read
    // from cache on dup resolution. The cache is MaybeUninit<[AccountView; 64]>.
    let mut buf = MultiAccountBuffer::new(&[
        MultiAccountEntry::account(0x01, 0),
        MultiAccountEntry::duplicate(0), // references remaining[0] via cache
    ]);
    let remaining = RemainingAccounts::new(buf.as_mut_ptr(), buf.boundary(), &[]);

    let views: Vec<_> = remaining.iter().collect();
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].address(), views[1].address());
}

#[test]
fn remaining_empty() {
    let mut buf: Vec<u64> = vec![0; 1];
    let ptr = buf.as_mut_ptr() as *mut u8;
    let boundary = ptr as *const u8;
    let remaining = RemainingAccounts::new(ptr, boundary, &[]);

    assert!(remaining.is_empty());
    assert!(remaining.get(0).is_none());
    assert_eq!(remaining.iter().count(), 0);
}

// ===========================================================================
// 6. MaybeUninit — verifying assume_init after full initialization
//
// These test the exact pattern from CpiCall::new and dispatch!:
//   MaybeUninit::<[T; N]>::uninit() → ptr::write each element → assume_init
// Miri flags assume_init on uninitialized memory, so these verify the loop
// actually writes every element.
// ===========================================================================

#[test]
fn maybeuninit_account_view_array() {
    const N: usize = 4;
    let mut bufs: Vec<AccountBuffer> = (0..N)
        .map(|i| {
            let mut buf = AccountBuffer::new(0);
            buf.init([i as u8; 32], [0u8; 32], i as u64 * 100, 0, false, false);
            buf
        })
        .collect();

    let views: [AccountView; N] = {
        let mut arr = MaybeUninit::<[AccountView; N]>::uninit();
        let ptr = arr.as_mut_ptr() as *mut AccountView;
        for i in 0..N {
            let view = unsafe { bufs[i].view() };
            unsafe { core::ptr::write(ptr.add(i), view) };
        }
        unsafe { arr.assume_init() }
    };

    for (i, view) in views.iter().enumerate() {
        assert_eq!(view.lamports(), i as u64 * 100);
    }
}

#[test]
fn maybeuninit_zero_length() {
    // Edge case: N=0 means assume_init on a zero-size array.
    let arr: [u8; 0] = {
        let arr = MaybeUninit::<[u8; 0]>::uninit();
        unsafe { arr.assume_init() }
    };
    assert_eq!(arr.len(), 0);
}

// ===========================================================================
// 7. Event serialization — copy_nonoverlapping on repr(C)
//
// Events are serialized by memcpy from a #[repr(C)] struct. If the struct
// has padding, the copy reads uninitialized padding bytes → UB. The compile-
// time size assertions prevent this, but Miri verifies at runtime.
// ===========================================================================

#[repr(C)]
struct TestEventWithPod {
    disc: [u8; 4],
    amount: PodU64,
    flag: PodBool,
}

const _: () = assert!(size_of::<TestEventWithPod>() == 13);
const _: () = assert!(align_of::<TestEventWithPod>() == 1);

#[test]
fn event_copy_reads_all_bytes_initialized() {
    // Construct event, copy via copy_nonoverlapping, verify every byte.
    // If the struct had padding, Miri would flag the copy as reading
    // uninitialized memory.
    let event = TestEventWithPod {
        disc: [0xDE, 0xAD, 0xBE, 0xEF],
        amount: PodU64::from(1_000_000u64),
        flag: PodBool::from(true),
    };

    let mut buf = [0u8; 13];
    unsafe {
        core::ptr::copy_nonoverlapping(
            &event as *const TestEventWithPod as *const u8,
            buf.as_mut_ptr(),
            size_of::<TestEventWithPod>(),
        );
    }

    assert_eq!(&buf[0..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(
        u64::from_le_bytes(buf[4..12].try_into().unwrap()),
        1_000_000
    );
    assert_eq!(buf[12], 1);
}

#[repr(C)]
struct WiderEvent {
    a: [u8; 32],
    b: PodU64,
    c: PodU32,
    d: PodU16,
    e: PodBool,
}

const _: () = assert!(size_of::<WiderEvent>() == 47);
const _: () = assert!(align_of::<WiderEvent>() == 1);

#[test]
fn event_copy_wider_struct_no_padding() {
    let event = WiderEvent {
        a: [0xAA; 32],
        b: PodU64::from(u64::MAX),
        c: PodU32::from(u32::MAX),
        d: PodU16::from(u16::MAX),
        e: PodBool::from(true),
    };

    let mut buf = [0u8; 47];
    unsafe {
        core::ptr::copy_nonoverlapping(
            &event as *const WiderEvent as *const u8,
            buf.as_mut_ptr(),
            47,
        );
    }

    // If any of the 47 bytes were padding (uninitialized), Miri flags it.
    assert!(buf[..32].iter().all(|&b| b == 0xAA));
    assert_eq!(u64::from_le_bytes(buf[32..40].try_into().unwrap()), u64::MAX);
    assert_eq!(u32::from_le_bytes(buf[40..44].try_into().unwrap()), u32::MAX);
    assert_eq!(u16::from_le_bytes(buf[44..46].try_into().unwrap()), u16::MAX);
    assert_eq!(buf[46], 1);
}

// ===========================================================================
// 8. Dispatch-style pointer patterns
//
// The dispatch! macro reads the discriminator and program_id via raw pointer
// casts from instruction data. These test the exact patterns.
// ===========================================================================

#[test]
fn discriminator_read_various_lengths() {
    let ix_data: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

    // 4-byte discriminator — most common
    let disc4: [u8; 4] = unsafe { *(ix_data.as_ptr() as *const [u8; 4]) };
    assert_eq!(disc4, [0xDE, 0xAD, 0xBE, 0xEF]);

    // 1-byte discriminator — minimum
    let disc1: [u8; 1] = unsafe { *(ix_data.as_ptr() as *const [u8; 1]) };
    assert_eq!(disc1, [0xDE]);

    // 8-byte discriminator — full width
    let disc8: [u8; 8] = unsafe { *(ix_data.as_ptr() as *const [u8; 8]) };
    assert_eq!(disc8, [0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn program_id_read_from_end_of_slice() {
    // In the SVM, program_id is appended after ix_data in the same allocation.
    // dispatch! reads it via: &*(ix_data.as_ptr().add(ix_data.len()) as *const [u8; 32])
    let mut combined = vec![0u8; 8 + 32];
    combined[8..].copy_from_slice(&[0x42; 32]);

    let ix_data = &combined[..8];
    let program_id: &[u8; 32] =
        unsafe { &*(ix_data.as_ptr().add(ix_data.len()) as *const [u8; 32]) };
    assert_eq!(program_id, &[0x42; 32]);
}

// ===========================================================================
// 9. Pod alignment sanity (minimal — no unsafe, just precondition verification)
// ===========================================================================

#[test]
fn pod_alignment_is_one() {
    // Pod types MUST have alignment 1. If they don't, every zero-copy Deref
    // in the framework is unsound (account data has alignment 1).
    assert_eq!(align_of::<PodU64>(), 1);
    assert_eq!(align_of::<PodU32>(), 1);
    assert_eq!(align_of::<PodU16>(), 1);
    assert_eq!(align_of::<PodU128>(), 1);
    assert_eq!(align_of::<PodI64>(), 1);
    assert_eq!(align_of::<PodI32>(), 1);
    assert_eq!(align_of::<PodI16>(), 1);
    assert_eq!(align_of::<PodI128>(), 1);
    assert_eq!(align_of::<PodBool>(), 1);
}

#[test]
fn transparent_wrapper_sizes() {
    // Account<T> and AccountView must be the same size/alignment.
    // This is the precondition for every transparent cast in the framework.
    assert_eq!(
        size_of::<Account<TestAccountType>>(),
        size_of::<AccountView>()
    );
    assert_eq!(
        align_of::<Account<TestAccountType>>(),
        align_of::<AccountView>()
    );
}

// ===========================================================================
// 10. Initialize<T> transparent cast
//
// Initialize<T> uses the same & → &mut pattern as Account<T> but has
// different trait bounds (QuasarAccount instead of Owner + AccountCheck)
// and a separate code path in initialize.rs.
// ===========================================================================

struct TestInitType;

impl Discriminator for TestInitType {
    const DISCRIMINATOR: &'static [u8] = &[0x01];
}

impl Space for TestInitType {
    const SPACE: usize = 8;
}

impl QuasarAccount for TestInitType {
    fn deserialize(_data: &[u8]) -> Result<Self, ProgramError> {
        Ok(Self)
    }
    fn serialize(&self, _data: &mut [u8]) -> Result<(), ProgramError> {
        Ok(())
    }
}

#[test]
fn initialize_shared_to_mut_cast() {
    let mut buf = AccountBuffer::new(16);
    buf.init([1u8; 32], [0u8; 32], 100, 16, false, true);

    let view = unsafe { buf.view() };
    let init = Initialize::<TestInitType>::from_account_view_mut(&view).unwrap();

    // Write through &mut Initialize path
    init.to_account_view().set_lamports(999);
    // Read through original &view — aliasing test
    assert_eq!(view.lamports(), 999);
}

#[test]
fn initialize_interleaved_access() {
    let mut buf = AccountBuffer::new(16);
    buf.init([1u8; 32], [0u8; 32], 100, 16, false, true);

    let view = unsafe { buf.view() };
    let init = Initialize::<TestInitType>::from_account_view_mut(&view).unwrap();

    // Interleave reads between &view and &mut Initialize
    let l1 = init.to_account_view().lamports();
    let l2 = view.lamports();
    assert_eq!(l1, l2);

    init.to_account_view().set_lamports(200);
    assert_eq!(view.lamports(), 200);

    view.set_lamports(300);
    assert_eq!(init.to_account_view().lamports(), 300);
}

// ===========================================================================
// 11. define_account! types (Signer, UncheckedAccount)
//
// These are generated by the define_account! macro, which has its own
// copy of the & → &mut transparent cast — third independent implementation.
// ===========================================================================

#[test]
fn unchecked_account_shared_to_mut_cast() {
    // UncheckedAccount has zero checks — the transparent cast is the only
    // unsafe operation. Test write-through-mut + read-through-shared aliasing.
    let mut buf = AccountBuffer::new(0);
    buf.init([1u8; 32], [0u8; 32], 500, 0, false, true);

    let view = unsafe { buf.view() };
    let unchecked = UncheckedAccount::from_account_view_mut(&view).unwrap();

    unchecked.to_account_view().set_lamports(123);
    assert_eq!(view.lamports(), 123);

    // Reverse: write through &view, read through &mut UncheckedAccount
    view.set_lamports(456);
    assert_eq!(unchecked.to_account_view().lamports(), 456);
}

#[test]
fn signer_shared_to_mut_cast() {
    // Signer checks is_signer flag before doing the transparent cast.
    let mut buf = AccountBuffer::new(0);
    buf.init([1u8; 32], [0u8; 32], 500, 0, true, true);

    let view = unsafe { buf.view() };
    let signer = SignerAccount::from_account_view_mut(&view).unwrap();

    signer.to_account_view().set_lamports(789);
    assert_eq!(view.lamports(), 789);

    view.set_lamports(101);
    assert_eq!(signer.to_account_view().lamports(), 101);
}

// ===========================================================================
// 12. Account::close() pattern
//
// close() does three unsafe operations on the same AccountView:
//   1. destination.set_lamports(destination.lamports() + view.lamports())
//   2. view.set_lamports(0)
//   3. view.assign(&SYSTEM_PROGRAM_ID)  — unsafe, raw pointer write to owner
//   4. view.resize(0)  — modifies data_len and resize_delta
//
// This tests the combined pattern with two AccountViews (source + dest).
// ===========================================================================

struct TestCloseableType;

impl Owner for TestCloseableType {
    const OWNER: Address = TEST_OWNER;
}

impl AccountCheck for TestCloseableType {
    fn check(_view: &AccountView) -> Result<(), ProgramError> {
        Ok(())
    }
}

impl Discriminator for TestCloseableType {
    const DISCRIMINATOR: &'static [u8] = &[0x01];
}

impl Space for TestCloseableType {
    const SPACE: usize = 8;
}

impl QuasarAccount for TestCloseableType {
    fn deserialize(_data: &[u8]) -> Result<Self, ProgramError> {
        Ok(Self)
    }
    fn serialize(&self, _data: &mut [u8]) -> Result<(), ProgramError> {
        Ok(())
    }
}

#[test]
fn close_transfers_lamports_and_zeroes_fields() {
    // Set up source account with data
    let data_len = 16usize;
    let mut src_buf = AccountBuffer::new(data_len);
    src_buf.init(
        [1u8; 32],
        TEST_OWNER.to_bytes(),
        1_000_000,
        data_len as u64,
        false,
        true,
    );
    // Write discriminator so AccountCheck passes
    let mut data = vec![0u8; data_len];
    data[0] = 0x01;
    src_buf.write_data(&data);

    // Set up destination account
    let mut dst_buf = AccountBuffer::new(0);
    dst_buf.init([2u8; 32], [0u8; 32], 500_000, 0, false, true);

    let src_view = unsafe { src_buf.view() };
    let dst_view = unsafe { dst_buf.view() };

    let account = Account::<TestCloseableType>::from_account_view(&src_view).unwrap();
    account.close(&dst_view).unwrap();

    // Source: lamports zeroed, owner changed, data_len zeroed
    assert_eq!(src_view.lamports(), 0);
    assert_eq!(src_view.data_len(), 0);
    assert!(src_view.owned_by(&Address::new_from_array([0u8; 32])));

    // Destination: received source's lamports
    assert_eq!(dst_view.lamports(), 1_500_000);
}

// ===========================================================================
// 13. assign + resize — individual unsafe operations
//
// Test the raw pointer writes that close() relies on, in isolation.
// ===========================================================================

#[test]
fn assign_changes_owner_through_raw_pointer() {
    // assign() does: write(&mut (*self.raw).owner, new_owner.clone())
    // This is an unsafe write to the owner field of RuntimeAccount.
    let mut buf = AccountBuffer::new(8);
    buf.init([1u8; 32], [0xAA; 32], 100, 8, false, true);

    let view = unsafe { buf.view() };
    assert!(view.owned_by(&Address::new_from_array([0xAA; 32])));

    let new_owner = Address::new_from_array([0xBB; 32]);
    unsafe { view.assign(&new_owner) };

    // Read back through the same view
    assert!(view.owned_by(&new_owner));
    assert!(!view.owned_by(&Address::new_from_array([0xAA; 32])));

    // Assign again to verify repeated writes are sound
    let third_owner = Address::new_from_array([0xCC; 32]);
    unsafe { view.assign(&third_owner) };
    assert!(view.owned_by(&third_owner));
}

#[test]
fn resize_grows_and_zeroes_extension() {
    // resize_unchecked() modifies data_len and resize_delta, then zero-extends
    // with write_bytes. Verify the zero-extension doesn't write out of bounds.
    let initial_data_len = 8usize;
    let mut buf = AccountBuffer::new(initial_data_len);
    buf.init(
        [1u8; 32],
        [0u8; 32],
        100,
        initial_data_len as u64,
        false,
        true,
    );
    // Fill data with non-zero bytes
    buf.write_data(&[0xFF; 8]);

    let view = unsafe { buf.view() };
    assert_eq!(view.data_len(), 8);

    // Grow to 16 bytes — the extension (bytes 8..16) must be zeroed
    view.resize(16).unwrap();
    assert_eq!(view.data_len(), 16);

    let data = unsafe { view.borrow_unchecked() };
    // Original data preserved
    assert!(data[..8].iter().all(|&b| b == 0xFF));
    // Extension zeroed
    assert!(data[8..16].iter().all(|&b| b == 0));

    // Shrink back
    view.resize(4).unwrap();
    assert_eq!(view.data_len(), 4);
}

// ===========================================================================
// 14. borrow_unchecked_mut write + read through other paths
//
// borrow_unchecked_mut creates &mut [u8] from the raw data pointer.
// These tests verify that writes through this path are visible when
// read through other raw-pointer-based paths (not through aliased refs).
// ===========================================================================

#[test]
fn borrow_unchecked_mut_write_then_read_via_data_ptr() {
    // Write through borrow_unchecked_mut, read through a fresh data_ptr.
    // Both paths derive independently from the raw pointer, so under Tree
    // Borrows, the read creates a new child tag that sees the write.
    let mut buf = AccountBuffer::new(16);
    buf.init([1u8; 32], [0u8; 32], 100, 16, false, true);

    let view = unsafe { buf.view() };

    // Write through borrow_unchecked_mut
    {
        let data = unsafe { view.borrow_unchecked_mut() };
        data[0..8].copy_from_slice(&42u64.to_le_bytes());
    }
    // borrow_unchecked_mut reference is dropped

    // Read through a fresh raw pointer path
    let val = unsafe { *(view.data_ptr() as *const u64) };
    assert_eq!(val, 42);
}

#[test]
fn borrow_unchecked_mut_sequential_borrows() {
    // Multiple sequential borrow_unchecked_mut calls — each creates a new
    // &mut [u8]. Verify previous writes persist across calls.
    let mut buf = AccountBuffer::new(16);
    buf.init([1u8; 32], [0u8; 32], 100, 16, false, true);

    let view = unsafe { buf.view() };

    // First borrow: write to bytes 0..8
    {
        let data = unsafe { view.borrow_unchecked_mut() };
        data[0..8].copy_from_slice(&100u64.to_le_bytes());
    }

    // Second borrow: write to bytes 8..16, verify bytes 0..8 persisted
    {
        let data = unsafe { view.borrow_unchecked_mut() };
        assert_eq!(u64::from_le_bytes(data[0..8].try_into().unwrap()), 100);
        data[8..16].copy_from_slice(&200u64.to_le_bytes());
    }

    // Third borrow: verify both writes persisted
    {
        let data = unsafe { view.borrow_unchecked() };
        assert_eq!(u64::from_le_bytes(data[0..8].try_into().unwrap()), 100);
        assert_eq!(u64::from_le_bytes(data[8..16].try_into().unwrap()), 200);
    }
}

// ===========================================================================
// 15. Boundary pointer subtraction
//
// Ctx::remaining_accounts() computes the boundary as:
//   self.data.as_ptr().sub(size_of::<u64>())
//
// This exercises pointer subtraction within a single allocation.
// If data.as_ptr() is at the start of the allocation, .sub(8) would be
// before the allocation → UB. Verify with the actual SVM buffer layout.
// ===========================================================================

#[test]
fn boundary_pointer_subtraction_within_allocation() {
    // Simulate SVM buffer layout:
    //   [remaining account data (ACCOUNT_HEADER bytes)]
    //   [instruction_data_len: u64 = 4]  ← boundary points here
    //   [instruction_data: 4 bytes]       ← data slice starts here
    //   [program_id: 32 bytes]
    //
    // The pointer subtraction data.as_ptr().sub(8) must stay within the
    // allocation. Uses Vec<u64> for 8-byte alignment (RuntimeAccount requires it).
    let remaining_size = ACCOUNT_HEADER + 8; // one account with 8 bytes data, aligned
    let remaining_aligned = (remaining_size + 7) & !7;
    let ix_data_len = 8usize; // use 8 to keep u64 alignment
    let total = remaining_aligned + size_of::<u64>() + ix_data_len + 32;
    let u64_count = (total + 7) / 8;

    let mut buffer: Vec<u64> = vec![0; u64_count];
    let base = buffer.as_mut_ptr() as *mut u8;

    // Set up the remaining account
    let raw = base as *mut RuntimeAccount;
    unsafe {
        (*raw).borrow_state = NOT_BORROWED;
        (*raw).is_signer = 0;
        (*raw).is_writable = 1;
        (*raw).executable = 0;
        (*raw).resize_delta = 0;
        (*raw).address = Address::new_from_array([0x01; 32]);
        (*raw).owner = Address::new_from_array([0xAA; 32]);
        (*raw).lamports = 100;
        (*raw).data_len = 8;
    }

    // Write instruction_data_len
    let ix_len_offset = remaining_aligned;
    unsafe {
        *(base.add(ix_len_offset) as *mut u64) = ix_data_len as u64;
    }

    // Write instruction data
    let ix_data_offset = ix_len_offset + size_of::<u64>();
    let ix_data = unsafe {
        std::slice::from_raw_parts(base.add(ix_data_offset), ix_data_len)
    };

    // Compute boundary the way Ctx::remaining_accounts() does:
    // boundary = data.as_ptr().sub(size_of::<u64>())
    let boundary = unsafe { ix_data.as_ptr().sub(size_of::<u64>()) };

    // The boundary must point to ix_len_offset
    assert_eq!(boundary, unsafe { base.add(ix_len_offset) as *const u8 });

    // Use the boundary with RemainingAccounts
    let remaining = RemainingAccounts::new(base, boundary, &[]);
    let v = remaining.get(0).unwrap();
    assert_eq!(v.lamports(), 100);
    assert!(remaining.get(1).is_none());
}

// ===========================================================================
// 16. Full parse simulation — MaybeUninit with partial reads during init
//
// This is the Pinocchio-equivalent full deserialization test. The dispatch
// macro + parse_accounts generated code does:
//   1. MaybeUninit::<[AccountView; N]>::uninit()
//   2. Walk SVM buffer, ptr::write each AccountView into the array
//   3. For duplicates, ptr::read from ALREADY-WRITTEN elements of the
//      same MaybeUninit array (before it's fully initialized)
//   4. assume_init()
//
// Step 3 is the critical pattern we haven't tested: reading from a
// partially-initialized MaybeUninit to resolve duplicates. Element 0
// is initialized, elements 1..N are still uninit, and we read element 0.
// ===========================================================================

#[test]
fn parse_simulation_dup_from_partially_initialized_buf() {
    // Build SVM-style buffer: [acct_count: u64][unique0][unique1][dup_of_0]
    let acct0_data_len = 8usize;
    let acct1_data_len = 0usize;
    let acct0_size = (ACCOUNT_HEADER + acct0_data_len + 7) & !7;
    let acct1_size = (ACCOUNT_HEADER + acct1_data_len + 7) & !7;
    let dup_size = size_of::<u64>();
    let total = size_of::<u64>() + acct0_size + acct1_size + dup_size;
    let u64_count = (total + 7) / 8;

    let mut buffer: Vec<u64> = vec![0; u64_count];
    let base = buffer.as_mut_ptr() as *mut u8;

    // Write account count
    unsafe { *(base as *mut u64) = 3 };

    let accounts_start = unsafe { base.add(size_of::<u64>()) };

    // Account 0: unique, 8 bytes data, lamports=100
    let raw0 = accounts_start as *mut RuntimeAccount;
    unsafe {
        (*raw0).borrow_state = NOT_BORROWED;
        (*raw0).is_signer = 1;
        (*raw0).is_writable = 1;
        (*raw0).executable = 0;
        (*raw0).resize_delta = 0;
        (*raw0).address = Address::new_from_array([0x01; 32]);
        (*raw0).owner = Address::new_from_array([0xAA; 32]);
        (*raw0).lamports = 100;
        (*raw0).data_len = acct0_data_len as u64;
    }

    // Account 1: unique, 0 bytes data, lamports=200
    let acct1_offset = acct0_size;
    let raw1 = unsafe { accounts_start.add(acct1_offset) as *mut RuntimeAccount };
    unsafe {
        (*raw1).borrow_state = NOT_BORROWED;
        (*raw1).is_signer = 0;
        (*raw1).is_writable = 1;
        (*raw1).executable = 0;
        (*raw1).resize_delta = 0;
        (*raw1).address = Address::new_from_array([0x02; 32]);
        (*raw1).owner = Address::new_from_array([0xBB; 32]);
        (*raw1).lamports = 200;
        (*raw1).data_len = acct1_data_len as u64;
    }

    // Account 2: duplicate of account 0 (borrow_state = 0, meaning index 0)
    let acct2_offset = acct0_size + acct1_size;
    unsafe { *accounts_start.add(acct2_offset) = 0u8 }; // original index = 0

    // Now simulate what dispatch + parse_accounts does:
    const N: usize = 3;
    let mut buf = MaybeUninit::<[AccountView; N]>::uninit();
    let arr_ptr = buf.as_mut_ptr() as *mut AccountView;
    let mut ptr = accounts_start;

    for i in 0..N {
        let raw = ptr as *mut RuntimeAccount;
        let borrow = unsafe { (*raw).borrow_state };

        if borrow == NOT_BORROWED {
            let view = unsafe { AccountView::new_unchecked(raw) };
            unsafe { core::ptr::write(arr_ptr.add(i), view) };
            unsafe {
                ptr = ptr.add(ACCOUNT_HEADER + (*raw).data_len as usize);
                ptr = ((ptr as usize + 7) & !7) as *mut u8;
            }
        } else {
            // THIS IS THE KEY PATTERN: ptr::read from a partially-initialized
            // MaybeUninit array. Element `borrow` is already written, but
            // elements after `i` are still uninitialized.
            let dup = unsafe { core::ptr::read(arr_ptr.add(borrow as usize)) };
            unsafe { core::ptr::write(arr_ptr.add(i), dup) };
            unsafe { ptr = ptr.add(size_of::<u64>()) };
        }
    }

    let accounts = unsafe { buf.assume_init() };

    assert_eq!(accounts[0].lamports(), 100);
    assert_eq!(accounts[1].lamports(), 200);
    // Account 2 is dup of 0
    assert_eq!(accounts[2].address(), accounts[0].address());
    assert_eq!(accounts[2].lamports(), 100);
}

// ===========================================================================
// 17. Duplicate AccountViews — two &mut Account<T> to same RuntimeAccount
//
// In real instruction handlers, the SVM can pass the same account twice.
// Both AccountViews share the same raw pointer. If both are cast to
// &mut Account<T> and both write set_lamports, we have two mutable
// wrappers writing to the same RuntimeAccount through raw pointers.
// Under Tree Borrows, the writes go through the raw pointer inside
// AccountView (not through the &mut reference itself), so this should
// be sound. Verify Miri agrees.
// ===========================================================================

#[test]
fn duplicate_account_views_two_mut_refs_write() {
    // Create ONE RuntimeAccount buffer
    let mut buf = AccountBuffer::new(64);
    buf.init(
        [1u8; 32],
        TEST_OWNER.to_bytes(),
        1_000_000,
        64,
        true,
        true,
    );

    // Create TWO AccountViews from the same buffer (simulating duplicates)
    let view_a = unsafe { buf.view() };
    let view_b = unsafe { AccountView::new_unchecked(buf.raw()) };

    // Cast both to &mut Account<T>
    let acct_a = Account::<TestAccountType>::from_account_view_mut(&view_a).unwrap();
    let acct_b = Account::<TestAccountType>::from_account_view_mut(&view_b).unwrap();

    // Write through acct_a
    acct_a.to_account_view().set_lamports(100);
    assert_eq!(acct_a.to_account_view().lamports(), 100);

    // Write through acct_b — same RuntimeAccount
    acct_b.to_account_view().set_lamports(200);
    assert_eq!(acct_b.to_account_view().lamports(), 200);

    // Read through acct_a — sees acct_b's write
    assert_eq!(acct_a.to_account_view().lamports(), 200);

    // Interleave writes
    acct_a.to_account_view().set_lamports(300);
    assert_eq!(acct_b.to_account_view().lamports(), 300);
    acct_b.to_account_view().set_lamports(400);
    assert_eq!(acct_a.to_account_view().lamports(), 400);
}

#[test]
fn duplicate_account_views_deref_mut_to_same_data() {
    // Same pattern but writing through DerefMut (to account data, not lamports).
    // Two &mut Account<T> → two &mut TestZcData → both write to same bytes.
    let disc_len = 4;
    let data_len = disc_len + size_of::<TestZcData>();
    let mut buf = AccountBuffer::new(data_len);
    buf.init(
        [1u8; 32],
        TEST_OWNER.to_bytes(),
        1_000_000,
        data_len as u64,
        true,
        true,
    );
    let mut data = vec![0u8; data_len];
    data[..disc_len].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    data[disc_len..disc_len + 8].copy_from_slice(&42u64.to_le_bytes());
    data[disc_len + 8] = 1;
    buf.write_data(&data);

    let view_a = unsafe { buf.view() };
    let view_b = unsafe { AccountView::new_unchecked(buf.raw()) };

    let acct_a = Account::<TestAccountType>::from_account_view_mut(&view_a).unwrap();
    let acct_b = Account::<TestAccountType>::from_account_view_mut(&view_b).unwrap();

    // Write through acct_a's DerefMut
    {
        let zc: &mut TestZcData = &mut *acct_a;
        zc.value = PodU64::from(111u64);
    }

    // Read through acct_b's Deref — sees acct_a's write
    {
        let zc: &TestZcData = &*acct_b;
        assert_eq!(zc.value.get(), 111);
    }

    // Write through acct_b's DerefMut
    {
        let zc: &mut TestZcData = &mut *acct_b;
        zc.value = PodU64::from(222u64);
    }

    // Read through acct_a's Deref
    {
        let zc: &TestZcData = &*acct_a;
        assert_eq!(zc.value.get(), 222);
    }
}

// ===========================================================================
// 18. Sysvar::get() on host — MaybeUninit + write_bytes + assume_init
//
// impl_sysvar_get! on non-SBF does:
//   MaybeUninit::<Self>::uninit()
//   var_addr.write_bytes(0, size_of::<Self>())
//   black_box(var_addr)
//   var.assume_init()
//
// This zero-initializes a MaybeUninit and calls assume_init. Sound only
// if all-zeros is a valid bit pattern for the type. For Rent (u64 + [u8; 8]),
// all-zeros is valid.
// ===========================================================================

#[test]
fn sysvar_get_maybeuninit_write_bytes_assume_init() {
    use quasar_core::sysvars::rent::Rent;

    // impl_sysvar_get! does: MaybeUninit::uninit() → write_bytes(0) → assume_init.
    // On host, Sysvar::get() returns Err (black_box ptr != 0), so we reproduce
    // the exact pattern manually. This verifies Miri accepts write_bytes as
    // full initialization for assume_init, and that all-zeros is valid for Rent.
    let rent: Rent = {
        let mut var = MaybeUninit::<Rent>::uninit();
        let var_addr = var.as_mut_ptr() as *mut u8;
        unsafe { var_addr.write_bytes(0, size_of::<Rent>()) };
        unsafe { var.assume_init() }
    };

    // Zero-initialized Rent: lamports_per_byte=0, exemption_threshold=[0;8]
    assert_eq!(rent.minimum_balance_unchecked(100), 0);
}
