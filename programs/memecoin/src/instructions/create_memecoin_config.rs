use anchor_lang::prelude::*;
use crate::state::*;
use anchor_lang::{
    solana_program::{
        clock::UnixTimestamp,
        sysvar::clock::Clock,
        system_instruction::transfer as lamports_transfer,
    }
};
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{create_metadata_accounts_v3, CreateMetadataAccountsV3, Metadata},
    token::{ Mint, Token, TokenAccount},
    token_2022::{mint_to, MintTo,}
};
use mpl_token_metadata::{ types::DataV2, accounts::{MasterEdition, Metadata as MetadataAccount }};
use crate::errors::ErrorCode;

#[derive(Accounts)]
#[instruction(
    memecoin_decimals: u8
)]
pub struct CreateMemecoinConfig<'info> {
    #[account(
        init_if_needed,
        payer = creator,
        space = CreatorMemecoinCounter::LEN,
        seeds = [b"COUNTER", creator.key().as_ref()],
        bump
    )]
    pub creator_memecoin_counter: Account<'info, CreatorMemecoinCounter>,

    #[account(
        init,
        payer = creator,
        space = MemecoinConfig::LEN,
        seeds = [creator.key().as_ref(), &creator_memecoin_counter.count.to_le_bytes()],
        bump
    )]
    pub memecoin_config: Account<'info, MemecoinConfig>,

    ///CHECK: Using "address" constraint to validate fee receiver address
    #[account(
        mut,
        address = global_config.create_memecoin_fee_receiver
    )]
    pub create_memecoin_fee_receiver: UncheckedAccount<'info>,

    #[account(mut)]
    pub creator: Signer<'info>,

    #[account(
        init,
        seeds = [b"mint", memecoin_config.key().as_ref()],
        bump,
        payer = creator,
        mint::decimals = memecoin_decimals,
        mint::authority = memecoin_config,
    )]
    pub mint: Account<'info, Mint>,

    /*
    ///CHECK: Using "address" constraint to validate metadata account address
    #[account(
        mut,
        address = MasterEdition::find_pda(&mint.key()).0
    )]
    pub metadata: UncheckedAccount<'info>,
     */

    #[account(
        init_if_needed,
        payer = creator,
        token::mint = mint,
        token::authority = memecoin_config,
        seeds=[b"MEME_COIN", mint.key().as_ref(), memecoin_config.key().as_ref()],
        bump
    )]
    pub destination: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"CONFIG"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfig>,

    pub rent: Sysvar<'info, Rent>,
    pub clock: Sysvar<'info, Clock>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    //pub token_metadata_program: Program<'info, Metadata>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[event]
pub struct MemecoinCreated {
    pub creator: Pubkey,
    pub created_time: u64,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub decimals: u8,
    pub funding_raise_tier: u8,
}

pub fn handler(
    ctx: Context<CreateMemecoinConfig>,
    memecoin_name: &str,
    memecoin_symbol: &str,
    memecoin_uri: &str,
    memecoin_decimals: u8,
    funding_raise_tier: u8
) -> Result<()> {
    let creator = &ctx.accounts.creator.key();
    let current_timestamp = ctx.accounts.clock.unix_timestamp as u64;

    // Charge for the create memecoin fee
    lamports_transfer(
        &ctx.accounts.creator.key(),
        &ctx.accounts.create_memecoin_fee_receiver.key(),
        ctx.accounts.global_config.create_memecoin_fee
    );

    let memecoin_config = &mut ctx.accounts.memecoin_config;
    let tier = match funding_raise_tier {
        0 => FundingRaiseTier::TwentySol,
        1 => FundingRaiseTier::FiftySol,
        2 => FundingRaiseTier::OneHundredSol,
        _ => return err!(ErrorCode::InvalidFundingRaiseTier),
    };
    memecoin_config.create_memecoin_config(
        creator,
        0,
        current_timestamp,
        tier
    )?;

    let creator_memecoin_counter = &mut ctx.accounts.creator_memecoin_counter;
    creator_memecoin_counter.increment();


    let seeds = &[
        ctx.accounts.memecoin_config.creator.as_ref(),
        &ctx.accounts.memecoin_config.creator_memecoin_index.to_le_bytes(),
        &[ctx.bumps.memecoin_config]
    ];
    let signer = [&seeds[..]];

    /*
    let token_data: DataV2 = DataV2 {
        name: memecoin_name.to_string(),
        symbol: memecoin_symbol.to_string(),
        uri: memecoin_uri.to_string(),
        seller_fee_basis_points: 0,
        creators: None,
        collection: None,
        uses: None,
    };

    let metadata_ctx = CpiContext::new_with_signer(
        ctx.accounts.token_metadata_program.to_account_info(),
        CreateMetadataAccountsV3 {
            payer: ctx.accounts.creator.to_account_info(),
            update_authority: ctx.accounts.memecoin_config.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
            mint_authority: ctx.accounts.memecoin_config.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        },
        &signer
    );

    create_metadata_accounts_v3(
        metadata_ctx,
        token_data,
        false,
        true,
        None,
    )?;
     */

    let quantity = MEMECOIN_TOTAL_SUPPLY
        .checked_mul(10_i32.pow(memecoin_decimals as u32) as u64)
        .ok_or_else(|| ErrorCode::CalculationError)?;
    mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                authority: ctx.accounts.memecoin_config.to_account_info(),
                to: ctx.accounts.destination.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
            &signer,
        ),
        quantity,
    )?;

    emit!(MemecoinCreated {
            creator: ctx.accounts.creator.key(),
            created_time: current_timestamp,
            name: memecoin_name.to_string(),
            symbol: memecoin_symbol.to_string(),
            uri: memecoin_uri.to_string(),
            decimals: memecoin_decimals,
            funding_raise_tier
        }
    );

    Ok(())
}