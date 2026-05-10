use anchor_lang::prelude::*;

// Stores who controls the whitelist.
// Seeds: ["whitelist-config"] — one per program deployment.
#[account]
#[derive(InitSpace)]
pub struct WhitelistConfig {
    pub admin: Pubkey,
    pub bump: u8,
}

// A "proof of membership" account — its existence IS the whitelist entry.
// Seeds: ["whitelist", user_pubkey] — one per whitelisted address.
// Stores nothing but its own bump so we can verify the PDA later.
#[account]
#[derive(InitSpace)]
pub struct WhitelistEntry {
    pub bump: u8,
}
