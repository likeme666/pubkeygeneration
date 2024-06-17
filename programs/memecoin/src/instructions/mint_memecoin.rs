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
    metadata::{
        create_metadata_accounts_v3,
        CreateMetadataAccountsV3,
        Metadata,
        mpl_token_metadata::types::DataV2,
    },
    token::{Token, TokenAccount, mint_to, MintTo, Mint},
    //token_2022::{mint_to, MintTo},
    //token_interface::Mint,
};
use anchor_spl::token_interface::TokenInterface;
//use mpl_token_metadata::accounts::{MasterEdition, Metadata as MetadataAccount };
use mpl_token_metadata::pda::find_metadata_account;
use solana_program::program::invoke;
use crate::errors::ErrorCode;

#[derive(Accounts)]
pub struct MintMemecoin<'info> {
    #[account(
        mut,
        seeds = [creator.key().as_ref(), &memecoin_config.creator_memecoin_index.to_le_bytes()],
        bump
    )]
    pub memecoin_config: Box<Account<'info, MemecoinConfig>>,

    #[account(mut)]
    pub creator: Signer<'info>,

    #[account(
        init,
        seeds = [b"mint", memecoin_config.key().as_ref()],
        bump,
        payer = creator,
        mint::decimals = 6,
        mint::authority = memecoin_config,
    )]
    pub mint: Box<Account<'info, Mint>>,

    ///CHECK: Using "address" constraint to validate metadata account address
    #[account(
        mut,
        address=find_metadata_account(&mint.key()).0
    )]
    pub metadata: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = creator,
        token::mint = mint,
        token::authority = memecoin_config,
        seeds=[b"MEME_COIN", mint.key().as_ref(), memecoin_config.key().as_ref()],
        bump
    )]
    pub destination: Box<Account<'info, TokenAccount>>,

    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub token_metadata_program: Program<'info, Metadata>,
}

#[event]
pub struct MemecoinCreated {
    pub creator: Pubkey,
    pub created_time: u64,
    pub memecoin_config: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub description: String,
    pub decimal: u8,
    pub website: String,
    pub telegram: String,
    pub twitter: String,
    pub funding_raise_tier: u8,
}

pub fn handler(
    ctx: Context<MintMemecoin>,
    memecoin_name: &str,
    memecoin_symbol: &str,
    memecoin_uri: &str,
    memecoin_description: &str,
    memecoin_website: &str,
    memecoin_telegram: &str,
    memecoin_twitter: &str,
) -> Result<()> {

    let seeds = &[
        ctx.accounts.memecoin_config.creator.as_ref(),
        &ctx.accounts.memecoin_config.creator_memecoin_index.to_le_bytes(),
        &[ctx.bumps.memecoin_config]
    ];
    let signer = [&seeds[..]];

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
        true,
        true,
        None,
    )?;

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
        MEMECOIN_TOTAL_SUPPLY,
    )?;

    let tier = ctx.accounts.memecoin_config.funding_raise_tier;
    let funding_raise_tier = match tier {
        FundingRaiseTier::TwentySol => 0,
        FundingRaiseTier::FiftySol => 1,
        FundingRaiseTier::OneHundredSol => 2,
        _ => return err!(ErrorCode::InvalidFundingRaiseTier),
    };
    emit!(MemecoinCreated {
            creator: ctx.accounts.creator.key(),
            created_time: ctx.accounts.memecoin_config.created_time,
            memecoin_config: ctx.accounts.memecoin_config.key(),
            mint: ctx.accounts.mint.key(),
            destination: ctx.accounts.destination.key(),
            name: memecoin_name.to_string(),
            symbol: memecoin_symbol.to_string(),
            uri: memecoin_uri.to_string(),
            description: memecoin_description.to_string(),
            decimal: 6,
            website: memecoin_website.to_string(),
            telegram: memecoin_telegram.to_string(),
            twitter: memecoin_twitter.to_string(),
            funding_raise_tier,
        }
    );



    Ok(())
}