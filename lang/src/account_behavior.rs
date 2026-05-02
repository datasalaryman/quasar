use solana_program_error::ProgramError;

/// Protocol-owned account behavior attached via `#[account(my_behavior(...))]`.
///
/// # Writing a behavior module
///
/// A behavior group `#[account(foo(a = x, b = y))]` requires a module `foo`
/// exporting:
///
/// - `foo::Args` — the args struct
/// - `foo::Args::builder()` — returns an `ArgsBuilder` with `.a()`, `.b()`
///   setters and `build_init()`, `build_check()`, `build_exit()` methods
/// - `foo::Behavior` — a unit struct implementing `AccountBehavior<T>` for each
///   supported account wrapper type
///
/// # Lifecycle phases
///
/// Each phase is guarded by an associated const. The derive only emits code
/// for phases where the const is `true`.
///
/// ```text
/// Phase           Const             Builder      Trait method       When
/// ─────────────── ───────────────── ──────────── ────────────────── ──────────
/// set_init_param  SETS_INIT_PARAMS  build_init   set_init_param()  init fields
/// after_init      RUN_AFTER_INIT    build_init   after_init()      init fields
/// check           RUN_CHECK         build_check  check()           all fields
/// update          RUN_UPDATE        build_check  update()          mut fields
/// exit            RUN_EXIT          build_exit   exit()            mut fields (epilogue)
/// ```
///
/// Default methods are no-ops. Override only the methods your behavior needs.
pub trait AccountBehavior<A> {
    type Args<'a>;

    /// Whether `set_init_param` contributes init parameters for `A`.
    /// The derive asserts at most one attached behavior group per field has
    /// this set to `true`.
    const SETS_INIT_PARAMS: bool = false;

    /// Whether `after_init` runs after account creation.
    const RUN_AFTER_INIT: bool = false;

    /// Whether `check` runs after account load.
    const RUN_CHECK: bool = true;

    /// Whether `update` runs after validation (requires `#[account(mut)]`).
    const RUN_UPDATE: bool = false;

    /// Whether `exit` runs in the epilogue (requires `#[account(mut)]`).
    const RUN_EXIT: bool = false;

    /// Whether the target field must be mutable for this behavior.
    /// Defaults to `RUN_UPDATE || RUN_EXIT`.
    const REQUIRES_MUT: bool = Self::RUN_UPDATE || Self::RUN_EXIT;

    fn set_init_param<'a>(
        _params: &mut <A as crate::account_init::AccountInit>::InitParams<'a>,
        _args: &Self::Args<'a>,
    ) -> Result<(), ProgramError>
    where
        A: crate::account_init::AccountInit,
    {
        Ok(())
    }

    fn after_init<'a>(_account: &mut A, _args: &Self::Args<'a>) -> Result<(), ProgramError> {
        Ok(())
    }

    fn check<'a>(_account: &A, _args: &Self::Args<'a>) -> Result<(), ProgramError> {
        Ok(())
    }

    fn update<'a>(_account: &mut A, _args: &Self::Args<'a>) -> Result<(), ProgramError> {
        Ok(())
    }

    fn exit<'a>(_account: &mut A, _args: &Self::Args<'a>) -> Result<(), ProgramError> {
        Ok(())
    }
}
