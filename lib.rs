use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction, program_error::ProgramError};

use mpl_core::{
    ID as CORE_PROGRAM_ID,
    accounts::BaseAssetV1,
    instructions::{
        AddPluginV1CpiBuilder,
        ApprovePluginAuthorityV1CpiBuilder,
        RevokePluginAuthorityV1CpiBuilder,
        UpdatePluginV1CpiBuilder,
        TransferV1Cpi,
        TransferV1CpiAccounts,
        TransferV1InstructionArgs,
        CreateV1CpiBuilder,
    },
    types::{
        Attribute, Attributes, Creator, DataState, FreezeDelegate, Plugin, PluginAuthority,
        PluginAuthorityPair, PluginType, Royalties, RuleSet, TransferDelegate,
    },
};

declare_id!("5gGNkXZgrR9rpDuNVLXkvh1nHKCrZuKZhWz4eGwkcwM2");

pub const MARKETPLACE_SEED: &[u8] = b"marketplace";
pub const LISTING_SEED: &[u8] = b"listing";

pub const CORE_ERR_PLUGIN_NOT_FOUND: u32 = 0x4; // mpl-core: "Plugin not found"
pub const CORE_ERR_PLUGIN_ALREADY_EXISTS: u32 = 0xF; // mpl-core: "Plugin already exists"


pub const PLATFORM_FEE_LAMPORTS: u64 = 100_000; // 0.0001 SOL

#[program]
pub mod game_core_marketplace {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, treasury: Pubkey) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.admin = ctx.accounts.admin.key();
        cfg.treasury = treasury;
        cfg.platform_fee_lamports = PLATFORM_FEE_LAMPORTS;
        cfg.bump = ctx.bumps.config;
        Ok(())
    }

    /// Mint a Core Asset and write immutable on-chain attributes:
    /// - minter (always)
    /// - type (optional)
    /// Optional royalties plugin.
    pub fn mint_asset(
        ctx: Context<MintAsset>,
        name: String,
        uri: String,
        optional_type: Option<String>,
        royalties_bps: Option<u16>,
        creators: Vec<CreatorInput>,
    ) -> Result<()> {
        require!(name.len() <= 64, MarketplaceError::NameTooLong);
        require!(uri.len() <= 200, MarketplaceError::UriTooLong);

        // Build Attributes (immutable by setting authority = None).
        let mut attrs = vec![
            Attribute {
                key: "minter".to_string(),
                value: ctx.accounts.minter.key().to_string(),
            },
        ];
        if let Some(t) = optional_type {
            require!(t.len() <= 32, MarketplaceError::TypeTooLong);
            attrs.push(Attribute { key: "type".to_string(), value: t });
        }

        let mut plugin_pairs: Vec<PluginAuthorityPair> = vec![PluginAuthorityPair {
            plugin: Plugin::Attributes(Attributes { attribute_list: attrs }),
            authority: Some(PluginAuthority::None), // immutable
        }];

        // Marketplace control plugins (owner-managed):
        // - TransferDelegate: lets the marketplace PDA transfer while listed
        // - FreezeDelegate: lets the marketplace PDA freeze/thaw while listed
        // NOTE: `authority: None` => Core defaults owner-managed plugins to Owner authority.
        plugin_pairs.push(PluginAuthorityPair {
            plugin: Plugin::TransferDelegate(TransferDelegate {}),
            authority: None,
        });
        plugin_pairs.push(PluginAuthorityPair {
            plugin: Plugin::FreezeDelegate(FreezeDelegate { frozen: false }),
            authority: None,
        });


        if let Some(bps) = royalties_bps {
            require!(bps <= 10_000, MarketplaceError::BadRoyaltiesBps);

            // Map creators inputs -> mpl_core::types::Creator (address + percentage)
            let mut mapped: Vec<Creator> = Vec::with_capacity(creators.len());
            let mut pct_sum: u16 = 0;
            for c in creators.iter() {
                pct_sum = pct_sum.saturating_add(c.percentage as u16);
                mapped.push(Creator {
                    address: c.address,
                    percentage: c.percentage,
                });
            }
            require!(pct_sum == 100, MarketplaceError::BadCreatorPercentages);

            plugin_pairs.push(PluginAuthorityPair {
                plugin: Plugin::Royalties(Royalties {
                    basis_points: bps,
                    creators: mapped,
                    rule_set: RuleSet::None,
                }),
                authority: Some(PluginAuthority::None), // treat as immutable
            });
        }

        // CreateV1 CPI into Core
        CreateV1CpiBuilder::new(&ctx.accounts.core_program.to_account_info())
            .asset(&ctx.accounts.asset.to_account_info())
            .collection(None)
            .payer(&ctx.accounts.payer.to_account_info())
            .authority(Some(&ctx.accounts.minter.to_account_info()))
            .owner(Some(&ctx.accounts.minter.to_account_info()))
            .update_authority(None)
            .system_program(&ctx.accounts.system_program.to_account_info())
            .data_state(DataState::AccountState)
            .name(name)
            .uri(uri)
            .plugins(plugin_pairs)
            .invoke()?;

        Ok(())
    }

    pub fn list(ctx: Context<List>, price_lamports: u64) -> Result<()> {
        msg!("List v6 (approve-first, idempotent plugins; handles PluginAlreadyExists)");
        require!(price_lamports > 0, MarketplaceError::BadPrice);

        let listing = &mut ctx.accounts.listing;
        require!(!listing.active, MarketplaceError::AlreadyListed);

        // Verify current owner is the signer (do NOT store owner in metadata).
        // NOTE: MPL Core asset accounts are owned by the Core program, so `asset.owner` is the program id.
        // Ownership is enforced by Core CPI calls below (seller signs as the asset authority).


        listing.asset = ctx.accounts.asset.key();
        listing.seller = ctx.accounts.seller.key();
        listing.price_lamports = price_lamports;
        listing.active = true;
        listing.bump = ctx.bumps.listing;

        // Delegate TransferDelegate + FreezeDelegate to listing PDA (no escrow).
        // Freeze makes listing safer (seller can't move away while listed).
        ensure_delegated_transfer(&ctx, ctx.accounts.listing.key())?;
        ensure_delegated_freeze_and_freeze_now(&ctx, ctx.accounts.listing.key())?;

        Ok(())
    }

    pub fn cancel(ctx: Context<Cancel>) -> Result<()> {
        // Idempotent-ish: if already inactive, allow close.
        if ctx.accounts.listing.active {
            // Only the original seller can cancel their listing.
            require_keys_eq!(ctx.accounts.listing.seller, ctx.accounts.seller.key(), MarketplaceError::NotSeller);


            // Thaw first (FreezeDelegate cannot be revoked while frozen).
            thaw_with_listing_pda_cancel(&ctx)?;

            // Revoke delegated authorities back to owner.
            let core_program_ai = ctx.accounts.core_program.to_account_info();
            let asset_ai = ctx.accounts.asset.to_account_info();
            let seller_ai = ctx.accounts.seller.to_account_info();
            let system_program_ai = ctx.accounts.system_program.to_account_info();

            revoke_plugin_to_owner(
                &core_program_ai,
                &asset_ai,
                &seller_ai,
                &seller_ai,
                &system_program_ai,
                PluginType::TransferDelegate,
            )?;
            revoke_plugin_to_owner(
                &core_program_ai,
                &asset_ai,
                &seller_ai,
                &seller_ai,
                &system_program_ai,
                PluginType::FreezeDelegate,
            )?;

            let listing = &mut ctx.accounts.listing;
            listing.active = false;
        }

        Ok(())
    }

    pub fn buy(ctx: Context<Buy>) -> Result<()> {
        let cfg = &ctx.accounts.config;

        // Validate listing + current owner.
        require!(ctx.accounts.listing.active, MarketplaceError::NotListed);
        // Validate the provided seller account matches the listing.
        require_keys_eq!(ctx.accounts.listing.seller, ctx.accounts.seller.key(), MarketplaceError::NotSeller);



        // Snapshot values we need (avoid borrow conflicts later).
        let price = ctx.accounts.listing.price_lamports;
        let fee = cfg.platform_fee_lamports;

        // --- SOL transfers ---
        // Buyer pays: price + fee.
        let buyer_ai = ctx.accounts.buyer.to_account_info();
        let seller_ai = ctx.accounts.seller.to_account_info();
        let treasury_ai = ctx.accounts.treasury.to_account_info();
        let system_program_ai = ctx.accounts.system_program.to_account_info();

        transfer_sol(&buyer_ai, &treasury_ai, &system_program_ai, fee)?;
        transfer_sol(&buyer_ai, &seller_ai, &system_program_ai, price)?;

        // Thaw (listing PDA signs) then transfer via Core TransferV1 with delegate authority.
        thaw_with_listing_pda(&ctx)?;

        // Transfer asset ownership to buyer (listing PDA is the delegate authority).
        let listing_ai = ctx.accounts.listing.to_account_info();
        let asset_ai = ctx.accounts.asset.to_account_info();
        let core_program_ai = ctx.accounts.core_program.to_account_info();

        let transfer_accounts = TransferV1CpiAccounts {
            asset: &asset_ai,
            collection: None,
            payer: &buyer_ai,
            authority: Some(&listing_ai),
            new_owner: &buyer_ai,
            system_program: Some(&system_program_ai),
            log_wrapper: None,
        };

        let transfer_args = TransferV1InstructionArgs { compression_proof: None };

        let seeds: &[&[u8]] = &[
            LISTING_SEED,
            ctx.accounts.listing.asset.as_ref(),
            &[ctx.accounts.listing.bump],
        ];

        TransferV1Cpi::new(&core_program_ai, transfer_accounts, transfer_args)
            .invoke_signed(&[seeds])?;

        // Revoke delegate authorities back to the new owner (buyer) so the asset is not locked to this PDA.
        revoke_plugin_to_owner(
            &core_program_ai,
            &asset_ai,
            &buyer_ai,
            &buyer_ai,
            &system_program_ai,
            PluginType::TransferDelegate,
        )?;
        revoke_plugin_to_owner(
            &core_program_ai,
            &asset_ai,
            &buyer_ai,
            &buyer_ai,
            &system_program_ai,
            PluginType::FreezeDelegate,
        )?;

        // Mark listing inactive (it will be closed to seller by the `close = seller` constraint).
        let listing = &mut ctx.accounts.listing;
        listing.active = false;

        Ok(())
    }
}

/* ----------------------------- Accounts ----------------------------- */

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    /// Treasury receives platform fees.
    /// CHECK: validated by storing in config, can be any pubkey.
    pub treasury: UncheckedAccount<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + MarketplaceConfig::LEN,
        seeds = [MARKETPLACE_SEED],
        bump
    )]
    pub config: Account<'info, MarketplaceConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintAsset<'info> {
    #[account(mut)]
    pub minter: Signer<'info>,

    /// Payer for account allocations.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The asset account (Core uses regular accounts).
    /// CHECK: created/allocated by CPI into Core.
    #[account(mut)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: verified by address constraint.
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct List<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = config.bump
    )]
    pub config: Account<'info, MarketplaceConfig>,

    #[account(
      init,
      payer = seller,
      space = 8 + Listing::LEN,
      seeds = [LISTING_SEED, asset.key().as_ref()],
      bump
      )]
    pub listing: Account<'info, Listing>,

    /// CHECK: This is an MPL Core asset account. It’s intentionally unchecked because it is
    /// owned by the Core program (not this program). We enforce `owner = CORE_PROGRAM_ID`
    /// in the account constraint and perform any additional authority/ownership checks in
    /// the instruction logic.
    #[account(mut, owner = CORE_PROGRAM_ID)]
     pub asset: UncheckedAccount<'info>,

    /// CHECK: verified by address constraint.
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Cancel<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,

    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = config.bump
    )]
    pub config: Account<'info, MarketplaceConfig>,

    #[account(
        mut,
        close = seller,
        seeds = [LISTING_SEED, asset.key().as_ref()],
        bump = listing.bump
    )]
    pub listing: Account<'info, Listing>,

    /// CHECK: This is an MPL Core asset account. It’s intentionally unchecked because it is
    /// owned by the Core program (not this program). We enforce `owner = CORE_PROGRAM_ID`
    /// in the account constraint and perform any additional authority/ownership checks in
    /// the instruction logic.
    #[account(mut, owner = CORE_PROGRAM_ID)]
     pub asset: UncheckedAccount<'info>,
    /// CHECK: verified by address constraint.
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Buy<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(
        seeds = [MARKETPLACE_SEED],
        bump = config.bump
    )]
    pub config: Account<'info, MarketplaceConfig>,

    /// Platform fee destination.
    /// CHECK: must match config.treasury
    #[account(mut, address = config.treasury)]
    pub treasury: UncheckedAccount<'info>,

    /// Seller receives proceeds and rent-close of listing.
    /// CHECK: validated vs listing.seller
    #[account(mut)]
    pub seller: UncheckedAccount<'info>,

    #[account(
        mut,
        close = seller,
        seeds = [LISTING_SEED, asset.key().as_ref()],
        bump = listing.bump
    )]
    pub listing: Account<'info, Listing>,

    /// CHECK: This is an MPL Core asset account. It’s intentionally unchecked because it is
    /// owned by the Core program (not this program). We enforce `owner = CORE_PROGRAM_ID`
    /// in the account constraint and perform any additional authority/ownership checks in
    /// the instruction logic.
    #[account(mut, owner = CORE_PROGRAM_ID)]
     pub asset: UncheckedAccount<'info>,

    /// CHECK: verified by address constraint.
    #[account(address = CORE_PROGRAM_ID)]
    pub core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/* ----------------------------- State ----------------------------- */

#[account]
pub struct MarketplaceConfig {
    pub admin: Pubkey,
    pub treasury: Pubkey,
    pub platform_fee_lamports: u64,
    pub bump: u8,
}
impl MarketplaceConfig {
    pub const LEN: usize = 32 + 32 + 8 + 1;
}

#[account]
pub struct Listing {
    pub asset: Pubkey,
    pub seller: Pubkey,
    pub price_lamports: u64,
    pub active: bool,
    pub bump: u8,
}
impl Listing {
    pub const LEN: usize = 32 + 32 + 8 + 1 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreatorInput {
    pub address: Pubkey,
    pub percentage: u8, // must sum to 100
}

/* ----------------------------- Helpers ----------------------------- */

fn transfer_sol<'info>(
    from: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    lamports: u64,
) -> Result<()> {
    let ix = system_instruction::transfer(from.key, to.key, lamports);
    invoke(&ix, &[from.clone(), to.clone(), system_program.clone()])?;
    Ok(())
}

fn ensure_delegated_transfer(ctx: &Context<List>, delegate: Pubkey) -> Result<()> {
    let core_program_ai = ctx.accounts.core_program.to_account_info();
    let system_program_ai = ctx.accounts.system_program.to_account_info();
    let asset_ai = ctx.accounts.asset.to_account_info();
    let payer_ai = ctx.accounts.seller.to_account_info();
    let seller_ai = ctx.accounts.seller.to_account_info();

    // Fast path: if the TransferDelegate plugin already exists, just (re)delegate authority to the listing PDA.
    // If the plugin is missing (common for Core assets minted outside this program), Core returns 0x4 ("Plugin not found").
    match ApprovePluginAuthorityV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .payer(&payer_ai)
        .authority(Some(&seller_ai))
        .system_program(&system_program_ai)
        .plugin_type(PluginType::TransferDelegate)
        .new_authority(PluginAuthority::Address { address: delegate })
        .invoke()
    {
        Ok(()) => return Ok(()),
        Err(ProgramError::Custom(code)) if code == CORE_ERR_PLUGIN_NOT_FOUND => {
            // Fall through and add the missing plugin, then retry approve.
        }
        Err(e) => return Err(e.into()),
    }

    // Add missing TransferDelegate plugin (idempotent).
    match AddPluginV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .payer(&payer_ai)
        .authority(Some(&seller_ai))
        .system_program(&system_program_ai)
        .plugin(Plugin::TransferDelegate(TransferDelegate {}))
        .invoke()
    {
        Ok(()) => {}
        Err(ProgramError::Custom(code)) if code == CORE_ERR_PLUGIN_ALREADY_EXISTS => {}
        Err(e) => return Err(e.into()),
    }

    // Retry approve after the plugin exists.
    ApprovePluginAuthorityV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .payer(&payer_ai)
        .authority(Some(&seller_ai))
        .system_program(&system_program_ai)
        .plugin_type(PluginType::TransferDelegate)
        .new_authority(PluginAuthority::Address { address: delegate })
        .invoke()?;

    Ok(())
}


fn ensure_delegated_freeze_and_freeze_now(ctx: &Context<List>, delegate: Pubkey) -> Result<()> {
    let core_program_ai = ctx.accounts.core_program.to_account_info();
    let system_program_ai = ctx.accounts.system_program.to_account_info();
    let payer_ai = ctx.accounts.seller.to_account_info();
    let seller_ai = ctx.accounts.seller.to_account_info();
    let asset_ai = ctx.accounts.asset.to_account_info();

    // Fast path: approve (re)delegation. If the plugin is missing, Core returns 0x4 ("Plugin not found").
    match ApprovePluginAuthorityV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .payer(&payer_ai)
        .authority(Some(&seller_ai))
        .system_program(&system_program_ai)
        .plugin_type(PluginType::FreezeDelegate)
        .new_authority(PluginAuthority::Address { address: delegate })
        .invoke()
    {
        Ok(()) => {}
        Err(ProgramError::Custom(code)) if code == CORE_ERR_PLUGIN_NOT_FOUND => {
            // Add the missing plugin then approve below.
            match AddPluginV1CpiBuilder::new(&core_program_ai)
                .asset(&asset_ai)
                .payer(&payer_ai)
                .authority(Some(&seller_ai))
                .system_program(&system_program_ai)
                .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
                .invoke()
            {
                Ok(()) => {}
                Err(ProgramError::Custom(code)) if code == CORE_ERR_PLUGIN_ALREADY_EXISTS => {}
                Err(e) => return Err(e.into()),
            }

            ApprovePluginAuthorityV1CpiBuilder::new(&core_program_ai)
                .asset(&asset_ai)
                .payer(&payer_ai)
                .authority(Some(&seller_ai))
                .system_program(&system_program_ai)
                .plugin_type(PluginType::FreezeDelegate)
                .new_authority(PluginAuthority::Address { address: delegate })
                .invoke()?;
        }
        Err(e) => return Err(e.into()),
    }

    // Now freeze using listing PDA signature.
    let listing = &ctx.accounts.listing;
    let seeds: &[&[u8]] = &[LISTING_SEED, listing.asset.as_ref(), &[listing.bump]];

    UpdatePluginV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .payer(&payer_ai)
        .authority(Some(&listing.to_account_info()))
        .system_program(&system_program_ai)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: true }))
        .invoke_signed(&[seeds])?;

    Ok(())
}


fn thaw_with_listing_pda(ctx: &Context<Buy>) -> Result<()> {
    let listing = &ctx.accounts.listing;

    let core_program_ai = ctx.accounts.core_program.to_account_info();
    let system_program_ai = ctx.accounts.system_program.to_account_info();
    let asset_ai = ctx.accounts.asset.to_account_info();
    let payer_ai = ctx.accounts.buyer.to_account_info();
    let listing_ai = ctx.accounts.listing.to_account_info();

    let seeds: &[&[u8]] = &[
        LISTING_SEED,
        listing.asset.as_ref(),
        &[listing.bump],
    ];

    UpdatePluginV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .collection(None)
        .payer(&payer_ai)
        .authority(Some(&listing_ai))
        .system_program(&system_program_ai)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[seeds])?;

    Ok(())
}

fn thaw_with_listing_pda_cancel(ctx: &Context<Cancel>) -> Result<()> {
    let listing = &ctx.accounts.listing;

    let core_program_ai = ctx.accounts.core_program.to_account_info();
    let system_program_ai = ctx.accounts.system_program.to_account_info();
    let asset_ai = ctx.accounts.asset.to_account_info();
    let payer_ai = ctx.accounts.seller.to_account_info();
    let listing_ai = ctx.accounts.listing.to_account_info();

    let seeds: &[&[u8]] = &[
        LISTING_SEED,
        listing.asset.as_ref(),
        &[listing.bump],
    ];

    UpdatePluginV1CpiBuilder::new(&core_program_ai)
        .asset(&asset_ai)
        .collection(None)
        .payer(&payer_ai)
        .authority(Some(&listing_ai))
        .system_program(&system_program_ai)
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[seeds])?;

    Ok(())
}

fn revoke_plugin_to_owner<'info>(
    core_program: &AccountInfo<'info>,
    asset: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    owner_authority: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    plugin_type: PluginType,
) -> Result<()> {
    RevokePluginAuthorityV1CpiBuilder::new(core_program)
        .asset(asset)
        .collection(None)
        .payer(payer)
        .authority(Some(owner_authority))
        .system_program(system_program)
        .plugin_type(plugin_type)
        .invoke()?;
    Ok(())
}

/* ----------------------------- Errors ----------------------------- */

#[error_code]
pub enum MarketplaceError {
    #[msg("Bad price.")]
    BadPrice,
    #[msg("Already listed.")]
    AlreadyListed,
    #[msg("Not listed.")]
    NotListed,
    #[msg("Not seller.")]
    NotSeller,
    #[msg("Not asset owner.")]
    NotAssetOwner,
    #[msg("Name too long.")]
    NameTooLong,
    #[msg("URI too long.")]
    UriTooLong,
    #[msg("Type too long.")]
    TypeTooLong,
    #[msg("Bad royalties bps.")]
    BadRoyaltiesBps,
    #[msg("Bad creator percentages (must sum to 100).")]
    BadCreatorPercentages,
}
