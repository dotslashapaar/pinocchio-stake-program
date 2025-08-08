
#![cfg_attr(target_arch = "bpf", no_std)]

#[cfg(not(target_arch = "bpf"))]
extern crate std;

extern crate alloc;


#[cfg(not(target_arch = "bpf"))]
#[global_allocator]
static GLOBAL: std::alloc::System = std::alloc::System;


#[cfg(target_arch = "bpf")]
pinocchio::no_allocator!();

#[cfg(target_arch = "bpf")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(all(target_arch = "bpf", not(feature = "no-entrypoint")))]
mod entrypoint;

pub mod error;
pub mod helpers;
pub mod instruction;
pub mod state;
pub mod vote_state;

pinocchio_pubkey::declare_id!("Stake11111111111111111111111111111111111111");