use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;
use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};
use spl_transfer_hook_interface::instruction::ExecuteInstruction;

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    payer: Signer<'info>,

    /// CHECK: ExtraAccountMetaList Account, must use these seeds
    #[account(
        init,
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump,
        space = ExtraAccountMetaList::size_of(
            InitializeExtraAccountMetaList::extra_account_metas()?.len()
        ).unwrap(),
        payer = payer
    )]
    pub extra_account_meta_list: AccountInfo<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    pub system_program: Program<'info, System>,
}

impl<'info> InitializeExtraAccountMetaList<'info> {
    pub fn initialize_transfer_hook(&mut self) -> Result<()> {
        let extras = Self::extra_account_metas()?;
        ExtraAccountMetaList::init::<ExecuteInstruction>(
            &mut self.extra_account_meta_list.try_borrow_mut_data()?,
            &extras,
        )
        .unwrap();
        Ok(())
    }

    pub fn extra_account_metas() -> Result<Vec<ExtraAccountMeta>> {
        // Token-2022 calls the transfer hook CPI with these accounts in order:
        //   index 0: source token account
        //   index 1: mint
        //   index 2: destination token account
        //   index 3: source owner (the wallet sending tokens)
        //
        // We register one extra account whose address is computed at transfer
        // time from dynamic seeds: ["whitelist", <account at index 3>].
        // Token-2022 resolves this PDA for each transfer, so the hook always
        // gets the entry for the *actual* sender — not a fixed address.
        Ok(vec![ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal {
                    bytes: b"whitelist".to_vec(),
                },
                Seed::AccountKey { index: 3 },
            ],
            false, // is_signer
            false, // is_writable
        )
        .unwrap()])
    }
}
