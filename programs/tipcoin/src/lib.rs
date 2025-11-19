use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

const MAX_FEE_BPS: u16 = 100;

declare_id!("4E9E74RpVCrtJXt7uaMYxVU1VG2yJBdvDYctGVcYRpGY");

#[program]
pub mod tipcoin {
    use super::*;

    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        args: InitializeConfigArgs,
    ) -> Result<()> {
        require!(args.relayer != Pubkey::default(), TipError::InvalidRelayer);

        require!(
            args.token_mint != Pubkey::default(),
            TipError::InvalidTokenMint
        );

        require!(args.fee_bps <= MAX_FEE_BPS, TipError::InvalidFeeBps);

        let config = &mut ctx.accounts.config;

        config.upgrade_authority = ctx.accounts.upgrade_authority.key();
        config.relayer = args.relayer;
        config.token_mint = args.token_mint;
        config.fee_bps = args.fee_bps;

        Ok(())
    }

    pub fn register(ctx: Context<Register>, hashed_discord_id: [u8; 32]) -> Result<()> {
        require!(
            hashed_discord_id.iter().any(|byte| *byte != 0),
            TipError::InvalidHashedDiscordId
        );

        let authority_key = ctx.accounts.authority.key();
        let config_mint = ctx.accounts.config.token_mint;

        let vault = &mut ctx.accounts.vault;
        vault.authority = authority_key;
        vault.hashed_discord_id = hashed_discord_id;
        vault.token_mint = config_mint;

        let allowance = &mut ctx.accounts.allowance;
        allowance.authority = authority_key;
        allowance.hashed_discord_id = hashed_discord_id;
        allowance.cap = 0;
        allowance.remaining = 0;

        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(amount > 0, TipError::InvalidDepositAmount);

        require_keys_eq!(
            ctx.accounts.vault.token_mint,
            ctx.accounts.config.token_mint,
            TipError::InvalidTokenMint
        );

        let cpi_accounts = Transfer {
            from: ctx.accounts.authority_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        let vault_bump = ctx.bumps.vault;

        emit!(DepositEvent {
            authority: ctx.accounts.authority.key(),
            vault: ctx.accounts.vault.key(),
            vault_bump,
            hashed_discord_id: ctx.accounts.vault.hashed_discord_id,
            amount,
        });

        Ok(())
    }

    pub fn approve_allowance(ctx: Context<ApproveAllowance>, amount: u64) -> Result<()> {
        let allowance = &mut ctx.accounts.allowance;

        allowance.cap = amount;
        allowance.remaining = amount;

        let (vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", allowance.hashed_discord_id.as_ref()],
            ctx.program_id,
        );

        emit!(AllowanceUpdated {
            authority: allowance.authority,
            vault,
            vault_bump,
            hashed_discord_id: allowance.hashed_discord_id,
            cap: allowance.cap,
            remaining: allowance.remaining,
        });

        Ok(())
    }

    pub fn revoke_allowance(ctx: Context<RevokeAllowance>) -> Result<()> {
        let allowance = &mut ctx.accounts.allowance;
        allowance.cap = 0;
        allowance.remaining = 0;

        let (vault, vault_bump) = Pubkey::find_program_address(
            &[b"vault", allowance.hashed_discord_id.as_ref()],
            ctx.program_id,
        );

        emit!(AllowanceUpdated {
            authority: allowance.authority,
            vault,
            vault_bump,
            hashed_discord_id: allowance.hashed_discord_id,
            cap: allowance.cap,
            remaining: allowance.remaining,
        });

        Ok(())
    }

    pub fn tip(
        ctx: Context<Tip>,
        amount: u64,
        tip_id: [u8; 32],
        memo: Option<String>,
        recipient_hashed_discord_id: [u8; 32],
    ) -> Result<()> {
        require!(amount > 0, TipError::InvalidTipAmount);
        require_keys_eq!(
            ctx.accounts.relayer.key(),
            ctx.accounts.config.relayer,
            TipError::InvalidRelayer
        );
        require!(
            recipient_hashed_discord_id.iter().any(|byte| *byte != 0),
            TipError::InvalidHashedDiscordId
        );

        let config_token_mint = ctx.accounts.config.token_mint;
        let sender_vault = &mut ctx.accounts.sender_vault;
        let sender_allowance = &mut ctx.accounts.sender_allowance;
        let recipient_vault = &mut ctx.accounts.recipient_vault;
        let fee_vault = &mut ctx.accounts.fee_vault;

        require_keys_eq!(
            sender_vault.token_mint,
            config_token_mint,
            TipError::InvalidTokenMint
        );

        require_keys_eq!(
            sender_vault.authority,
            sender_allowance.authority,
            TipError::InvalidAuthority
        );

        require!(
            sender_vault.hashed_discord_id == sender_allowance.hashed_discord_id,
            TipError::InvalidSenderPda
        );

        if recipient_vault.hashed_discord_id == [0u8; 32]
            && recipient_vault.token_mint == Pubkey::default()
        {
            recipient_vault.authority = Pubkey::default();
            recipient_vault.hashed_discord_id = recipient_hashed_discord_id;
            recipient_vault.token_mint = config_token_mint;
        } else {
            require!(
                recipient_vault.hashed_discord_id == recipient_hashed_discord_id,
                TipError::InvalidRecipientPda
            );
            require_keys_eq!(
                recipient_vault.token_mint,
                config_token_mint,
                TipError::InvalidTokenMint
            );
        }

        let sender_hash = sender_vault.hashed_discord_id;
        let recipient_hash = recipient_vault.hashed_discord_id;

        let (expected_sender_vault, sender_vault_bump) =
            Pubkey::find_program_address(&[b"vault", sender_hash.as_ref()], ctx.program_id);
        require_keys_eq!(
            expected_sender_vault,
            sender_vault.key(),
            TipError::InvalidSenderPda
        );

        let (expected_recipient_vault, recipient_vault_bump) =
            Pubkey::find_program_address(&[b"vault", recipient_hash.as_ref()], ctx.program_id);
        require_keys_eq!(
            expected_recipient_vault,
            recipient_vault.key(),
            TipError::InvalidRecipientPda
        );

        if fee_vault.config == Pubkey::default() {
            fee_vault.config = ctx.accounts.config.key();
            fee_vault.token_mint = config_token_mint;
            fee_vault.bump = ctx.bumps.fee_vault;
        } else {
            require_keys_eq!(
                fee_vault.config,
                ctx.accounts.config.key(),
                TipError::InvalidFeeVault
            );
            require_keys_eq!(
                fee_vault.token_mint,
                config_token_mint,
                TipError::InvalidTokenMint
            );
        }

        let fee_bps = ctx.accounts.config.fee_bps;
        let fee_amount = calculate_fee(amount, fee_bps)?;
        let total_amount = amount
            .checked_add(fee_amount)
            .ok_or(TipError::FeeCalculationOverflow)?;

        require!(
            sender_allowance.remaining >= total_amount,
            TipError::AllowanceExceeded
        );

        let sender_hash_slice = sender_hash.as_ref();
        let sender_vault_bump_seed = [sender_vault_bump];
        let sender_vault_seeds: [&[u8]; 3] =
            [b"vault", sender_hash_slice, sender_vault_bump_seed.as_ref()];
        let signer_seeds: [&[&[u8]]; 1] = [&sender_vault_seeds];

        let transfer_accounts = Transfer {
            from: ctx.accounts.sender_vault_token_account.to_account_info(),
            to: ctx.accounts.recipient_vault_token_account.to_account_info(),
            authority: sender_vault.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_ctx, amount)?;

        if fee_amount > 0 {
            let fee_transfer_accounts = Transfer {
                from: ctx.accounts.sender_vault_token_account.to_account_info(),
                to: ctx.accounts.fee_vault_token_account.to_account_info(),
                authority: sender_vault.to_account_info(),
            };

            let fee_cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                fee_transfer_accounts,
                &signer_seeds,
            );

            token::transfer(fee_cpi_ctx, fee_amount)?;
        }

        sender_allowance.remaining = sender_allowance
            .remaining
            .checked_sub(total_amount)
            .ok_or(TipError::AllowanceExceeded)?;

        emit!(TipEvent {
            relayer: ctx.accounts.relayer.key(),
            sender_vault: sender_vault.key(),
            sender_vault_bump,
            recipient_vault: recipient_vault.key(),
            recipient_vault_bump,
            sender_hashed_discord_id: sender_hash,
            recipient_hashed_discord_id: recipient_hash,
            amount,
            allowance_remaining: sender_allowance.remaining,
            tip_id,
            fee_vault: fee_vault.key(),
            fee_vault_bump: fee_vault.bump,
            fee_amount,
            fee_bps,
            total_amount,
        });

        let _ = memo;

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, TipError::InvalidWithdrawAmount);

        require_keys_eq!(
            ctx.accounts.vault.authority,
            ctx.accounts.authority.key(),
            TipError::InvalidVaultAuthority
        );

        require_keys_eq!(
            ctx.accounts.vault.token_mint,
            ctx.accounts.config.token_mint,
            TipError::InvalidTokenMint
        );

        require!(
            ctx.accounts.vault_token_account.amount >= amount,
            TipError::InsufficientVaultBalance
        );

        let vault_bump = ctx.bumps.vault;
        let vault_seeds: [&[u8]; 3] = [
            b"vault",
            ctx.accounts.vault.hashed_discord_id.as_ref(),
            &[vault_bump],
        ];
        let signer_seeds: [&[&[u8]]; 1] = [&vault_seeds];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.destination_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_ctx, amount)?;

        emit!(WithdrawEvent {
            authority: ctx.accounts.authority.key(),
            vault: ctx.accounts.vault.key(),
            vault_bump,
            hashed_discord_id: ctx.accounts.vault.hashed_discord_id,
            destination: ctx.accounts.destination_token_account.owner,
            destination_token_account: ctx.accounts.destination_token_account.key(),
            amount,
        });

        Ok(())
    }

    pub fn withdraw_fee(ctx: Context<WithdrawFee>, amount: u64) -> Result<()> {
        require!(amount > 0, TipError::InvalidWithdrawAmount);

        require!(
            ctx.accounts.fee_vault_token_account.amount >= amount,
            TipError::InsufficientVaultBalance
        );

        let fee_vault_bump = ctx.accounts.fee_vault.bump;
        let config_key = ctx.accounts.config.key();
        let fee_vault_seeds: [&[u8]; 3] =
            [b"fee_vault", config_key.as_ref(), &[fee_vault_bump]];
        let signer_seeds: [&[&[u8]]; 1] = [&fee_vault_seeds];

        let cpi_accounts = Transfer {
            from: ctx.accounts.fee_vault_token_account.to_account_info(),
            to: ctx.accounts.destination_token_account.to_account_info(),
            authority: ctx.accounts.fee_vault.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds,
        );

        token::transfer(cpi_ctx, amount)?;

        Ok(())
    }

    pub fn set_relayer(ctx: Context<SetRelayer>, new_relayer: Pubkey) -> Result<()> {
        ctx.accounts.config.relayer = new_relayer;
        Ok(())
    }

    pub fn set_fee_rate(ctx: Context<SetFeeRate>, fee_bps: u16) -> Result<()> {
        require!(fee_bps <= MAX_FEE_BPS, TipError::InvalidFeeBps);
        ctx.accounts.config.fee_bps = fee_bps;
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InitializeConfigArgs {
    pub relayer: Pubkey,
    pub token_mint: Pubkey,
    pub fee_bps: u16,
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub upgrade_authority: Signer<'info>,
    #[account(
        init,
        payer = upgrade_authority,
        space = Config::SPACE,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(hashed_discord_id: [u8; 32])]
pub struct Register<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [b"config"],
        bump,
        constraint = config.token_mint != Pubkey::default() @ TipError::InvalidTokenMint
    )]
    pub config: Account<'info, Config>,
    #[account(
        init_if_needed,
        payer = authority,
        space = Vault::SPACE,
        seeds = [b"vault", hashed_discord_id.as_ref()],
        bump
    )]
    pub vault: Account<'info, Vault>,
    #[account(
        init_if_needed,
        payer = authority,
        space = Allowance::SPACE,
        seeds = [b"allowance", hashed_discord_id.as_ref()],
        bump
    )]
    pub allowance: Account<'info, Allowance>,
    pub system_program: Program<'info, System>,
    #[account(address = sysvar::rent::ID)]
    pub rent: Sysvar<'info, Rent>,
    #[account(constraint = token_mint.key() == config.token_mint @ TipError::InvalidTokenMint)]
    pub token_mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.hashed_discord_id.as_ref()],
        bump,
        has_one = authority
    )]
    pub vault: Account<'info, Vault>,
    #[account(
        mut,
        constraint = authority_token_account.owner == authority.key() @ TipError::InvalidAuthority,
        constraint = authority_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub authority_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = vault_token_account.owner == vault.key() @ TipError::InvalidVaultAuthority,
        constraint = vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ApproveAllowance<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"allowance", allowance.hashed_discord_id.as_ref()],
        bump,
        has_one = authority
    )]
    pub allowance: Account<'info, Allowance>,
}

#[derive(Accounts)]
pub struct RevokeAllowance<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"allowance", allowance.hashed_discord_id.as_ref()],
        bump,
        has_one = authority
    )]
    pub allowance: Account<'info, Allowance>,
}

#[derive(Accounts)]
#[instruction(
    amount: u64,
    tip_id: [u8; 32],
    memo: Option<String>,
    recipient_hashed_discord_id: [u8; 32]
)]
pub struct Tip<'info> {
    #[account(mut, seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub relayer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", sender_vault.hashed_discord_id.as_ref()],
        bump
    )]
    pub sender_vault: Account<'info, Vault>,
    #[account(
        mut,
        seeds = [b"allowance", sender_allowance.hashed_discord_id.as_ref()],
        bump
    )]
    pub sender_allowance: Account<'info, Allowance>,
    #[account(
        init_if_needed,
        payer = relayer,
        space = Vault::SPACE,
        seeds = [b"vault", recipient_hashed_discord_id.as_ref()],
        bump
    )]
    pub recipient_vault: Account<'info, Vault>,
    #[account(
        init_if_needed,
        payer = relayer,
        space = FeeVault::SPACE,
        seeds = [b"fee_vault", config.key().as_ref()],
        bump
    )]
    pub fee_vault: Account<'info, FeeVault>,
    #[account(
        mut,
        constraint = sender_vault_token_account.owner == sender_vault.key() @ TipError::InvalidVaultAuthority,
        constraint = sender_vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub sender_vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = recipient_vault_token_account.owner == recipient_vault.key() @ TipError::InvalidVaultAuthority,
        constraint = recipient_vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub recipient_vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = fee_vault_token_account.owner == fee_vault.key() @ TipError::InvalidVaultAuthority,
        constraint = fee_vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub fee_vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(seeds = [b"config"], bump)]
    pub config: Account<'info, Config>,
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.hashed_discord_id.as_ref()],
        bump,
        has_one = authority
    )]
    pub vault: Account<'info, Vault>,
    #[account(
        mut,
        constraint = vault_token_account.owner == vault.key() @ TipError::InvalidVaultAuthority,
        constraint = vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = destination_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub destination_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawFee<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        has_one = upgrade_authority @ TipError::InvalidAuthority
    )]
    pub config: Account<'info, Config>,
    pub upgrade_authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"fee_vault", config.key().as_ref()],
        bump = fee_vault.bump,
        constraint = fee_vault.config == config.key() @ TipError::InvalidFeeVault,
        constraint = fee_vault.token_mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub fee_vault: Account<'info, FeeVault>,
    #[account(
        mut,
        constraint = fee_vault_token_account.owner == fee_vault.key() @ TipError::InvalidVaultAuthority,
        constraint = fee_vault_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub fee_vault_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = destination_token_account.mint == config.token_mint @ TipError::InvalidTokenMint
    )]
    pub destination_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SetRelayer<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        has_one = upgrade_authority @ TipError::InvalidAuthority
    )]
    pub config: Account<'info, Config>,
    pub upgrade_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetFeeRate<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump,
        has_one = upgrade_authority @ TipError::InvalidAuthority
    )]
    pub config: Account<'info, Config>,
    pub upgrade_authority: Signer<'info>,
}

#[account]
pub struct Config {
    pub upgrade_authority: Pubkey,
    pub relayer: Pubkey,
    pub token_mint: Pubkey,
    pub fee_bps: u16,
}

impl Config {
    pub const SPACE: usize = 8 + 32 + 32 + 32 + 2;
}

#[account]
pub struct Vault {
    pub authority: Pubkey,
    pub hashed_discord_id: [u8; 32],
    pub token_mint: Pubkey,
}

impl Vault {
    pub const SPACE: usize = 8 + 32 + 32 + 32;
}

#[account]
pub struct FeeVault {
    pub config: Pubkey,
    pub token_mint: Pubkey,
    pub bump: u8,
}

impl FeeVault {
    pub const SPACE: usize = 8 + 32 + 32 + 1;
}

#[account]
pub struct Allowance {
    pub authority: Pubkey,
    pub hashed_discord_id: [u8; 32],
    pub cap: u64,
    pub remaining: u64,
}

impl Allowance {
    pub const SPACE: usize = 8 + 32 + 32 + 8 + 8;
}

fn calculate_fee(amount: u64, fee_bps: u16) -> Result<u64> {
    if fee_bps == 0 {
        return Ok(0);
    }

    let numerator = (amount as u128)
        .checked_mul(fee_bps as u128)
        .ok_or(TipError::FeeCalculationOverflow)?;
    let mut fee = numerator / 10_000;

    if numerator % 10_000 != 0 {
        fee = fee.checked_add(1).ok_or(TipError::FeeCalculationOverflow)?;
    }

    if fee > u64::MAX as u128 {
        return Err(TipError::FeeCalculationOverflow.into());
    }

    Ok(fee as u64)
}

#[error_code]
pub enum TipError {
    #[msg("Invalid relayer authority")]
    InvalidRelayer,
    #[msg("Invalid upgrade authority")]
    InvalidAuthority,
    #[msg("Vault authority does not match signer")]
    InvalidVaultAuthority,
    #[msg("Token mint mismatch")]
    InvalidTokenMint,
    #[msg("Fee rate exceeds allowed maximum")]
    InvalidFeeBps,
    #[msg("Fee vault mismatch")]
    InvalidFeeVault,
    #[msg("Deposit amount must be greater than zero")]
    InvalidDepositAmount,
    #[msg("Invalid hashed Discord identifier")]
    InvalidHashedDiscordId,
    #[msg("Tip amount must be greater than zero")]
    InvalidTipAmount,
    #[msg("Allowance remaining is insufficient for this tip")]
    AllowanceExceeded,
    #[msg("Sender hashed Discord account mismatch")]
    InvalidSenderPda,
    #[msg("Recipient hashed Discord account mismatch")]
    InvalidRecipientPda,
    #[msg("Withdraw amount must be greater than zero")]
    InvalidWithdrawAmount,
    #[msg("Vault balance is insufficient for withdrawal")]
    InsufficientVaultBalance,
    #[msg("Fee calculation overflowed")]
    FeeCalculationOverflow,
}

#[event]
pub struct DepositEvent {
    pub authority: Pubkey,
    pub vault: Pubkey,
    pub vault_bump: u8,
    pub hashed_discord_id: [u8; 32],
    pub amount: u64,
}

#[event]
pub struct AllowanceUpdated {
    pub authority: Pubkey,
    pub vault: Pubkey,
    pub vault_bump: u8,
    pub hashed_discord_id: [u8; 32],
    pub cap: u64,
    pub remaining: u64,
}

#[event]
pub struct TipEvent {
    pub relayer: Pubkey,
    pub sender_vault: Pubkey,
    pub sender_vault_bump: u8,
    pub recipient_vault: Pubkey,
    pub recipient_vault_bump: u8,
    pub sender_hashed_discord_id: [u8; 32],
    pub recipient_hashed_discord_id: [u8; 32],
    pub amount: u64,
    pub allowance_remaining: u64,
    pub tip_id: [u8; 32],
    pub fee_vault: Pubkey,
    pub fee_vault_bump: u8,
    pub fee_amount: u64,
    pub fee_bps: u16,
    pub total_amount: u64,
}

#[event]
pub struct WithdrawEvent {
    pub authority: Pubkey,
    pub vault: Pubkey,
    pub vault_bump: u8,
    pub hashed_discord_id: [u8; 32],
    pub destination: Pubkey,
    pub destination_token_account: Pubkey,
    pub amount: u64,
}
