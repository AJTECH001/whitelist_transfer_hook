use anchor_lang::{prelude::*, system_program};

use crate::state::Whitelist;

#[derive(Accounts)]
pub struct WhitelistOperations<'info> {
    #[account(
        mut,
        //address =
    )]
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [b"whitelist"],
        bump,
    )]
    pub whitelist: Account<'info, Whitelist>,
    pub system_program: Program<'info, System>,
}

impl<'info> WhitelistOperations<'info> {
    pub fn add_to_whitelist(&mut self, address: Pubkey) -> Result<()> {
        if !self.whitelist.address.contains(&address) {
            self.realloc_whitelist(true)?;
            self.whitelist.address.push(address);
        }
        Ok(())
    }

    pub fn remove_from_whitelist(&mut self, address: Pubkey) -> Result<()> {
        if let Some(pos) = self.whitelist.address.iter().position(|&x| x == address) {
            self.whitelist.address.remove(pos);
            self.realloc_whitelist(false)?;
        }
        Ok(())
    }

    pub fn realloc_whitelist(&self, is_adding: bool) -> Result<()> {
        // Get the account info for the whitelist
        let account_info = self.whitelist.to_account_info();

        if is_adding {
            // Adding to whitelist
            let new_account_size = account_info
                .data_len()
                .checked_add(std::mem::size_of::<Pubkey>())
                .ok_or(ProgramError::ArithmeticOverflow)?;
            // Calculate rent required for the new account size
            let lamports_required = (Rent::get()?).minimum_balance(new_account_size);
            // Determine additional rent required
            let rent_diff = lamports_required
                .checked_sub(account_info.lamports())
                .ok_or(ProgramError::ArithmeticOverflow)?;

            // Perform transfer of additional rent
            let cpi_program = self.system_program.key();
            let cpi_accounts = system_program::Transfer {
                from: self.admin.to_account_info(),
                to: account_info.clone(),
            };
            let cpi_context = CpiContext::new(cpi_program, cpi_accounts);
            system_program::transfer(cpi_context, rent_diff)?;

            // Reallocate the account
            account_info.resize(new_account_size)?;
            msg!("Account Size Updated: {}", account_info.data_len());
        } else {
            // Removing from whitelist
            let new_account_size = account_info
                .data_len()
                .checked_sub(std::mem::size_of::<Pubkey>())
                .ok_or(ProgramError::ArithmeticOverflow)?;
            // Calculate rent required for the new account size
            let lamports_required = (Rent::get()?).minimum_balance(new_account_size);
            // Determine additional rent to be refunded
            let rent_diff = account_info
                .lamports()
                .checked_sub(lamports_required)
                .ok_or(ProgramError::ArithmeticOverflow)?;

            // Reallocate the account
            account_info.resize(new_account_size)?;
            msg!("Account Size Downgraded: {}", account_info.data_len());

            // Perform transfer to refund additional rent
            let admin_info = self.admin.to_account_info();
            let whitelist_info = self.whitelist.to_account_info();
            let mut admin_lamports = admin_info.try_borrow_mut_lamports()?;
            let mut whitelist_lamports = whitelist_info.try_borrow_mut_lamports()?;
            **admin_lamports = (**admin_lamports)
                .checked_add(rent_diff)
                .ok_or(ProgramError::ArithmeticOverflow)?;
            **whitelist_lamports = (**whitelist_lamports)
                .checked_sub(rent_diff)
                .ok_or(ProgramError::ArithmeticOverflow)?;
        }

        Ok(())
    }
}
