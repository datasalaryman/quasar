//! Core macros for account definitions and runtime assertions.
//!
//! - `define_account!` — generates a `#[repr(transparent)]` account wrapper
//!   with check trait implementations and unchecked constructors for optimized
//!   parsing.
//! - `require!`, `require_eq!`, `require_keys_eq!` — constraint assertion
//!   macros that return early with a typed error on failure.
//! - `emit!` — emits an event via `sol_log_data` (~100 CU).

#[macro_export]
macro_rules! define_account {
    // With deref target: `pub struct Token => [checks::Owner]: TokenAccountState`
    // Generates everything from the base form plus Deref/DerefMut/ZeroCopyDeref/StaticView.
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident => [$($check:path),* $(,)?] : $target:ty
    ) => {
        $crate::define_account!($(#[$meta])* $vis struct $name => [$($check),*]);

        unsafe impl $crate::traits::StaticView for $name {}

        impl core::ops::Deref for $name {
            type Target = $target;

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                // SAFETY: AccountCheck validated data_len >= size_of::<$target>.
                // $target is #[repr(C)] with alignment 1.
                unsafe { &*(self.view.data_ptr() as *const $target) }
            }
        }

        impl core::ops::DerefMut for $name {
            #[inline(always)]
            fn deref_mut(&mut self) -> &mut Self::Target {
                // SAFETY: Same as Deref — length validated, alignment 1.
                unsafe { &mut *(self.view.data_mut_ptr() as *mut $target) }
            }
        }

        impl $crate::traits::ZeroCopyDeref for $name {
            type Target = $target;

            #[inline(always)]
            unsafe fn deref_from(view: &AccountView) -> &Self::Target {
                &*(view.data_ptr() as *const $target)
            }

            #[inline(always)]
            unsafe fn deref_from_mut(view: &mut AccountView) -> &mut Self::Target {
                &mut *(view.data_mut_ptr() as *mut $target)
            }
        }
    };

    // Base form: `pub struct Signer => [checks::Signer]`
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident => [$($check:path),* $(,)?]
    ) => {
        $(#[$meta])*
        #[repr(transparent)]
        $vis struct $name {
            view: AccountView,
        }

        $(impl $check for $name {})*

        impl AsAccountView for $name {
            #[inline(always)]
            fn to_account_view(&self) -> &AccountView {
                &self.view
            }
        }

        impl $name {
            /// # Safety
            /// Caller must ensure all check traits have been validated.
            #[inline(always)]
            pub unsafe fn from_account_view_unchecked(view: &AccountView) -> &Self {
                &*(view as *const AccountView as *const Self)
            }

            /// # Safety
            /// Caller must ensure all check traits and writability.
            #[inline(always)]
            pub unsafe fn from_account_view_unchecked_mut(view: &mut AccountView) -> &mut Self {
                &mut *(view as *mut AccountView as *mut Self)
            }
        }
    };
}

#[macro_export]
macro_rules! require {
    ($condition:expr, $error:expr) => {
        if !($condition) {
            return Err($error.into());
        }
    };
}

#[macro_export]
macro_rules! require_eq {
    ($left:expr, $right:expr, $error:expr) => {
        if $left != $right {
            return Err($error.into());
        }
    };
}

#[macro_export]
macro_rules! require_keys_eq {
    ($left:expr, $right:expr, $error:expr) => {
        if !$crate::keys_eq(&$left, &$right) {
            return Err($error.into());
        }
    };
}

#[macro_export]
macro_rules! emit {
    ($event:expr) => {
        $event.emit_log()
    };
}
