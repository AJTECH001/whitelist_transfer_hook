use anchor_lang::prelude::*;

use crate::state::{WhitelistConfig, WhitelistEntry};

#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct RemoveFromWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        seeds = [b"whitelist-config"],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, WhitelistConfig>,
    // `close = admin` does three things automatically:
    //   1. Zeroes out the account data
    //   2. Transfers all lamports (rent) back to `admin`
    //   3. Sets the account owner back to the System Program
    // This is the clean way to "delete" a PDA — no manual lamport math needed.
    #[account(
        mut,
        close = admin,
        seeds = [b"whitelist", user.as_ref()],
        bump = whitelist_entry.bump,
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
}

impl<'info> RemoveFromWhitelist<'info> {
    pub fn remove_from_whitelist(&mut self) -> Result<()> {
        // Nothing to do here — `close = admin` in the constraint handles everything.
        Ok(())
    }
}
