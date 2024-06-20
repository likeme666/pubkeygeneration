use anchor_lang::prelude::*;
use crate::state::*;
use anchor_lang::{
    solana_program::{
        clock::UnixTimestamp,
        sysvar::clock::Clock,
        program::invoke_signed,
    },
    system_program::{Transfer as LamportsTransfer, transfer as lamports_transfer},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{create_metadata_accounts_v3, CreateMetadataAccountsV3, Metadata},
    token::{transfer as memecoin_transfer, Burn, Mint, Token, TokenAccount, Transfer as MemecoinTransfer},
    //token_2022::{self, transfer_checked as memecoin_transfer, TransferChecked, Token2022},
};
use anchor_spl::token_interface::TokenInterface;
use solana_program::lamports;
use solana_program::program::invoke;
use crate::errors::ErrorCode;

#[derive(Accounts)]
pub struct ClaimLamports<'info> {
    #[account(
        mut,
        seeds = [memecoin_config.creator.key().as_ref(), &memecoin_config.creator_memecoin_index.to_le_bytes()],
        bump
    )]
    pub memecoin_config: Account<'info, MemecoinConfig>,

    #[account(
        mut,
        seeds = [b"mint", memecoin_config.key().as_ref()],
        bump,
    )]
    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub claimer: Signer<'info>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = claimer
    )]
    pub claimer_token: Account<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = memecoin_config,
        seeds=[b"MEME_COIN", mint.key().as_ref(), memecoin_config.key().as_ref()],
        bump
    )]
    pub memecoin_config_token: Account<'info, TokenAccount>,

    pub clock: Sysvar<'info, Clock>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[event]
pub struct LamportsClaimed {
    pub claimer: Pubkey,
    pub claim_amount: u64,
    pub mint: Pubkey,
    pub token_price: u64,
}

pub fn handler(
    ctx: Context<ClaimLamports>,
    claim_amount: u64,
) -> Result<()> {
    let memecoin_config_token_balance = ctx.accounts.memecoin_config_token.amount;
    let sold_amount = MEMECOIN_TOTAL_SUPPLY
        .checked_sub(memecoin_config_token_balance)
        .ok_or_else(|| ErrorCode::CalculationError)?;

    let current_timestamp = ctx.accounts.clock.unix_timestamp as u64;
    if current_timestamp >= ctx.accounts.memecoin_config.created_time + 3600 {
        let memecoin_config = &mut ctx.accounts.memecoin_config;
        if sold_amount == MEMECOIN_TOTAL_SUPPLY / 2 {
            memecoin_config.set_memecoin_status(
                LaunchStatus::Succeed
            )?;
            return err!(ErrorCode::CannotClaimWhenLaunchSuccess);
        } else {
            memecoin_config.set_memecoin_status(
                LaunchStatus::Failed
            )?;
        }
    } else {
        return err!(ErrorCode::CannotClaimWhenNotEnd);
    }

    // User send the memecoin back
    memecoin_transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            MemecoinTransfer {
                from: ctx.accounts.claimer_token.to_account_info(),
                to: ctx.accounts.memecoin_config_token.to_account_info(),
                authority: ctx.accounts.claimer.to_account_info(),
            },
        ),
        claim_amount,
    )?;

    // Transfer the lamports back to claimer
    let token_price = ctx.accounts.memecoin_config.token_price()?;
    let total_lamports = claim_amount.checked_mul(token_price).ok_or_else(|| ErrorCode::CalculationError)?
        .checked_div(MEMECOIN_DECIMAL).ok_or_else(|| ErrorCode::CalculationError)?;
    if total_lamports == 0 {
        return err!(ErrorCode::ClaimAmountTooSmall);
    }
    ctx.accounts.memecoin_config.sub_lamports(total_lamports)?;
    ctx.accounts.claimer.add_lamports(total_lamports)?;


    emit!(LamportsClaimed {
            claimer: ctx.accounts.claimer.key(),
            claim_amount,
            mint: ctx.accounts.mint.key(),
            token_price,
        }
    );

    Ok(())
}