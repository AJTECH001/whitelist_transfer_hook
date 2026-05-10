use anchor_lang::prelude::*;

use crate::state::WhitelistConfig;

// This instruction runs once to set up the whitelist authority.
// It creates the WhitelistConfig PDA that future add/remove instructions
// use to verify the caller is actually the admin.
#[derive(Accounts)]
pub struct InitializeWhitelist<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + WhitelistConfig::INIT_SPACE,
        seeds = [b"whitelist-config"],
        bump
    )]
    pub config: Account<'info, WhitelistConfig>,
    pub system_program: Program<'info, System>,
}

impl<'info> InitializeWhitelist<'info> {
    pub fn initialize_whitelist(&mut self, bumps: InitializeWhitelistBumps) -> Result<()> {
        self.config.set_inner(WhitelistConfig {
            admin: self.admin.key(),
            bump: bumps.config,
        });
        Ok(())
    }
}
