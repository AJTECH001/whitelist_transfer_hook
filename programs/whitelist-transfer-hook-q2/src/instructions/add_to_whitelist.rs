use anchor_lang::prelude::*;

use crate::state::{WhitelistConfig, WhitelistEntry};

// `user` is passed as an instruction argument so Anchor can use it in the
// seeds constraint below to derive the correct PDA address before executing.
#[derive(Accounts)]
#[instruction(user: Pubkey)]
pub struct AddToWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    // `has_one = admin` checks that config.admin == admin.key()
    // If not, Anchor rejects the instruction before our code runs.
    #[account(
        seeds = [b"whitelist-config"],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, WhitelistConfig>,
    // `init` creates this PDA for the first time.
    // If it already exists, Anchor will error — so duplicates are impossible.
    // The PDA address = program_id + ["whitelist", user_pubkey]
    #[account(
        init,
        payer = admin,
        space = 8 + WhitelistEntry::INIT_SPACE,
        seeds = [b"whitelist", user.as_ref()],
        bump,
    )]
    pub whitelist_entry: Account<'info, WhitelistEntry>,
    pub system_program: Program<'info, System>,
}

impl<'info> AddToWhitelist<'info> {
    pub fn add_to_whitelist(&mut self, bumps: AddToWhitelistBumps) -> Result<()> {
        // Store the bump so we can verify this PDA later in remove and transfer_hook.
        self.whitelist_entry.set_inner(WhitelistEntry {
            bump: bumps.whitelist_entry,
        });
        Ok(())
    }
}
