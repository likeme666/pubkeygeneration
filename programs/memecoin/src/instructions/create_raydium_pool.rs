use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Token, Burn},
    token_2022::{self, transfer_checked as memecoin_transfer, TransferChecked, Token2022},
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use anchor_lang::{
    solana_program::system_instruction::transfer as lamports_transfer,
};
use raydium_cp_swap::{
    cpi,
    program::RaydiumCpSwap,
    states::{AmmConfig, OBSERVATION_SEED, POOL_LP_MINT_SEED, POOL_SEED, POOL_VAULT_SEED},
};
use crate::state::{MemecoinConfig, LaunchStatus, GlobalConfig, MEMECOIN_TOTAL_SUPPLY};
use crate::errors::ErrorCode;
use std::str::FromStr;
use crate::constants::WSOL_MINT_ADDRESS;

#[derive(Accounts)]
pub struct CreateRaydiumPool<'info> {
    #[account(
        mut,
        seeds = [memecoin_config.creator.key().as_ref(), &memecoin_config.creator_memecoin_index.to_le_bytes()],
        bump
    )]
    pub memecoin_config: Account<'info, MemecoinConfig>,

    #[account(
        seeds = [b"CONFIG"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfig>,

    pub cp_swap_program: Program<'info, RaydiumCpSwap>,

    /// Address paying to create the pool. Can be anyone
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Which config the pool belongs to.
    pub amm_config: Box<Account<'info, AmmConfig>>,

    /// CHECK: pool vault and lp mint authority
    #[account(
        seeds = [raydium_cp_swap::AUTH_SEED.as_bytes()],
        bump,
    )]
    pub authority: UncheckedAccount<'info>,

    /// CHECK: Initialize an account to store the pool state, init by cp-swap
    #[account(
        mut,
        seeds = [
            POOL_SEED.as_bytes(),
            amm_config.key().as_ref(),
            token_0_mint.key().as_ref(),
            token_1_mint.key().as_ref(),
        ],
        bump,
    )]
    pub pool_state: UncheckedAccount<'info>,

    /// Token_0 mint, the key must smaller than token_1 mint.
    #[account(
        constraint = token_0_mint.key() < token_1_mint.key(),
        mint::token_program = token_2022_program,
    )]
    pub token_0_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Token_1 mint, the key must grater then token_0 mint.
    #[account(
        mint::token_program = token_2022_program,
    )]
    pub token_1_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: pool lp mint, init by cp-swap
    #[account(
        mut,
        seeds = [
        POOL_LP_MINT_SEED.as_bytes(),
        pool_state.key().as_ref(),
        ],
        bump,
    )]
    pub lp_mint: UncheckedAccount<'info>,

    /// memecoin_config token0 account
    #[account(
        mut,
        token::mint = token_0_mint,
        token::authority = memecoin_config,
    )]
    pub memecoin_config_token_0: Box<InterfaceAccount<'info, TokenAccount>>,

    /// memecoin_config token1 account
    #[account(
        mut,
        token::mint = token_1_mint,
        token::authority = memecoin_config,
    )]
    pub memecoin_config_token_1: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        associated_token::mint = lp_mint,
        associated_token::authority = memecoin_config
    )]
    pub memecoin_config_lp_token: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Token_0 vault for the pool, init by cp-swap
    #[account(
        mut,
        seeds = [
        POOL_VAULT_SEED.as_bytes(),
        pool_state.key().as_ref(),
        token_0_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_0_vault: UncheckedAccount<'info>,

    /// CHECK: Token_1 vault for the pool, init by cp-swap
    #[account(
        mut,
        seeds = [
            POOL_VAULT_SEED.as_bytes(),
            pool_state.key().as_ref(),
            token_1_mint.key().as_ref()
        ],
        bump,
    )]
    pub token_1_vault: UncheckedAccount<'info>,

    /// create pool fee account
    #[account(
        mut,
        address= raydium_cp_swap::create_pool_fee_reveiver::id(),
    )]
    pub create_pool_fee: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: an account to store oracle observations, init by cp-swap
    #[account(
        mut,
        seeds = [
            OBSERVATION_SEED.as_bytes(),
            pool_state.key().as_ref(),
        ],
        bump,
    )]
    pub observation_state: UncheckedAccount<'info>,

    /// CHECK: checked by address constraint
    #[account(
        mut,
        address = global_config.launch_success_fee_receiver.key(),
    )]
    pub launch_success_fee_receiver: UncheckedAccount<'info>,

    /// Program to create mint account and mint tokens
    pub token_program: Program<'info, Token>,
    /// Spl token program or token program 2022
    pub token_2022_program: Interface<'info, TokenInterface>,
    /// Program to create an ATA for receiving position NFT
    pub associated_token_program: Program<'info, AssociatedToken>,
    /// To create a new program account
    pub system_program: Program<'info, System>,
    /// Sysvar for program account
    pub rent: Sysvar<'info, Rent>,
    /// Sysvar for clock account
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(
    ctx: Context<CreateRaydiumPool>,
) -> Result<()> {
    require!(ctx.accounts.memecoin_config.status == LaunchStatus::Succeed, ErrorCode::OnlyCreatePoolWhenLaunchSuccess);

    let total_funding_raise_amount = ctx.accounts.memecoin_config.funding_raise_tier.value();
    let launch_success_fee_bps = ctx.accounts.global_config.launch_success_fee_bps as u64;
    let launch_success_fee_sol_amount = total_funding_raise_amount
        .checked_mul(launch_success_fee_bps).ok_or_else(|| ErrorCode::CalculationError)?
        .checked_div(10000u64).ok_or_else(|| ErrorCode::CalculationError)?;

    let wsol_mint_pubkey = Pubkey::from_str(WSOL_MINT_ADDRESS).unwrap();
    let memecoin_decimal;
    let init_amount_0;
    let init_amount_1;
    let launch_success_fee_memecoin_amount;
    let memecoin_mint;
    if ctx.accounts.token_0_mint.key() == wsol_mint_pubkey {
        memecoin_decimal = ctx.accounts.token_1_mint.decimals;
        memecoin_mint = &ctx.accounts.token_1_mint;
        init_amount_0 = ctx.accounts.memecoin_config_token_0.amount;
        launch_success_fee_memecoin_amount = ctx.accounts.memecoin_config_token_1.amount
            .checked_mul(launch_success_fee_bps).ok_or_else(|| ErrorCode::CalculationError)?
            .checked_div(10000u64).ok_or_else(|| ErrorCode::CalculationError)?;
        init_amount_1 = ctx.accounts.memecoin_config_token_1.amount
            .checked_sub(launch_success_fee_memecoin_amount).ok_or_else(|| ErrorCode::CalculationError)?;
    } else {
        memecoin_decimal = ctx.accounts.token_0_mint.decimals;
        memecoin_mint = &ctx.accounts.token_0_mint;
        init_amount_1 = ctx.accounts.memecoin_config_token_1.amount;
        launch_success_fee_memecoin_amount = ctx.accounts.memecoin_config_token_0.amount
            .checked_mul(launch_success_fee_bps).ok_or_else(|| ErrorCode::CalculationError)?
            .checked_div(10000u64).ok_or_else(|| ErrorCode::CalculationError)?;
        init_amount_0 = ctx.accounts.memecoin_config_token_0.amount
            .checked_sub(launch_success_fee_memecoin_amount).ok_or_else(|| ErrorCode::CalculationError)?;
    }

    // Transfer sol fee

    lamports_transfer(
        &ctx.accounts.memecoin_config.key(),
        &ctx.accounts.launch_success_fee_receiver.key(),
        launch_success_fee_sol_amount
    );

    // Transfer memecoin fee
    let seeds = &[
        ctx.accounts.memecoin_config.creator.as_ref(),
        &ctx.accounts.memecoin_config.creator_memecoin_index.to_le_bytes(),
        &[ctx.bumps.memecoin_config]
    ];
    let signer = [&seeds[..]];
    memecoin_transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.memecoin_config.to_account_info(),
                mint: memecoin_mint.to_account_info(),
                to: ctx.accounts.launch_success_fee_receiver.to_account_info(),
                authority: ctx.accounts.memecoin_config.to_account_info(),
            },
            &signer,
        ),
        launch_success_fee_memecoin_amount,
        memecoin_decimal
    )?;

    // Create a raydium pool
    let initialize_cpi_accounts = cpi::accounts::Initialize {
        creator: ctx.accounts.memecoin_config.to_account_info(),
        amm_config: ctx.accounts.amm_config.to_account_info(),
        authority: ctx.accounts.authority.to_account_info(),
        pool_state: ctx.accounts.pool_state.to_account_info(),
        token_0_mint: ctx.accounts.token_0_mint.to_account_info(),
        token_1_mint: ctx.accounts.token_1_mint.to_account_info(),
        lp_mint: ctx.accounts.lp_mint.to_account_info(),
        creator_token_0: ctx.accounts.memecoin_config_token_0.to_account_info(),
        creator_token_1: ctx.accounts.memecoin_config_token_1.to_account_info(),
        creator_lp_token: ctx.accounts.memecoin_config_lp_token.to_account_info(),
        token_0_vault: ctx.accounts.token_0_vault.to_account_info(),
        token_1_vault: ctx.accounts.token_1_vault.to_account_info(),
        create_pool_fee: ctx.accounts.create_pool_fee.to_account_info(),
        observation_state: ctx.accounts.observation_state.to_account_info(),
        token_program: ctx.accounts.token_program.to_account_info(),
        token_0_program: ctx.accounts.token_2022_program.to_account_info(),
        token_1_program: ctx.accounts.token_2022_program.to_account_info(),
        associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        rent: ctx.accounts.rent.to_account_info(),
    };


    let initialize_cpi_context = CpiContext::new_with_signer(
        ctx.accounts.cp_swap_program.to_account_info(),
        initialize_cpi_accounts,
        &signer
    );
    let open_time = ctx.accounts.clock.unix_timestamp as u64;

    cpi::initialize(initialize_cpi_context, init_amount_0, init_amount_1, open_time)?;

    // Burn all the LP tokens in the MemecoinConfig account
    let burn_cpi_accounts = Burn {
        mint: ctx.accounts.lp_mint.to_account_info(),
        from: ctx.accounts.memecoin_config_lp_token.to_account_info(),
        authority: ctx.accounts.authority.to_account_info(),
    };
    let burn_cpi_program = ctx.accounts.token_program.to_account_info();
    let burn_cpi_ctx = CpiContext::new_with_signer(
        burn_cpi_program,
        burn_cpi_accounts,
        &signer
    );
    token::burn(burn_cpi_ctx, ctx.accounts.memecoin_config_lp_token.amount)?;

    Ok(())
}