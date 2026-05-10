#![allow(unexpected_cfgs)]
#![allow(deprecated)]

pub mod instructions;
pub mod state;

pub use instructions::*;
pub use state::*;

use anchor_lang::prelude::*;
use spl_discriminator::SplDiscriminate;
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

declare_id!("EUkbfr6mqkXx4XFAdFaRQP79kw4ibQbEZwjmxUUkQxao");

#[program]
pub mod whitelist_transfer_hook_q2 {
    use super::*;

    pub fn initialize_whitelist(ctx: Context<InitializeWhitelist>) -> Result<()> {
        ctx.accounts.initialize_whitelist(ctx.bumps)
    }

    // `user` is the address to whitelist. Anchor uses it to derive the PDA before calling this.
    #[allow(unused_variables)]
    pub fn add_to_whitelist(ctx: Context<AddToWhitelist>, user: Pubkey) -> Result<()> {
        ctx.accounts.add_to_whitelist(ctx.bumps)
    }

    // `user` is the address to remove. Used to derive and close the right PDA.
    #[allow(unused_variables)]
    pub fn remove_from_whitelist(ctx: Context<RemoveFromWhitelist>, user: Pubkey) -> Result<()> {
        ctx.accounts.remove_from_whitelist()
    }

    pub fn initialize_transfer_hook(ctx: Context<InitializeExtraAccountMetaList>) -> Result<()> {
        ctx.accounts.initialize_transfer_hook()
    }

    #[instruction(discriminator = ExecuteInstruction::SPL_DISCRIMINATOR_SLICE)]
    pub fn transfer_hook(ctx: Context<TransferHook>, amount: u64) -> Result<()> {
        ctx.accounts.transfer_hook(amount)
    }
}
