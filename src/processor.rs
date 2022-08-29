//! Program state processor

use crate::constraints::{SwapConstraints, SWAP_CONSTRAINTS};
use crate::{
    swap::{
        base::SwapCurve,
        fees::Fees,
    },
    error::SwapError,
    instruction::{
        DepositAllTokenTypes, Initialize, Swap,
        SwapInstruction, WithdrawAllTokenTypes,
    },
    state::{SwapV1, SwapVersion},
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::{ProgramError},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};
use std::convert::TryInto;

/// Program state handler.
pub struct Processor {}
impl Processor {
    /// Unpacks a spl_token `Account`.
    pub fn unpack_token_account(
        account_info: &AccountInfo,
        token_program_id: &Pubkey,
    ) -> Result<spl_token::state::Account, SwapError> {
        if account_info.owner != token_program_id {
            Err(SwapError::IncorrectTokenProgramId)
        } else {
            spl_token::state::Account::unpack(&account_info.data.borrow())
                .map_err(|_| SwapError::ExpectedAccount)
        }
    }

    /// Unpacks a spl_token `Mint`.
    pub fn unpack_mint(
        account_info: &AccountInfo,
        token_program_id: &Pubkey,
    ) -> Result<spl_token::state::Mint, SwapError> {
        if account_info.owner != token_program_id {
            Err(SwapError::IncorrectTokenProgramId)
        } else {
            spl_token::state::Mint::unpack(&account_info.data.borrow())
                .map_err(|_| SwapError::ExpectedMint)
        }
    }

    /// Calculates the authority id by generating a program address.
    pub fn authority_id(
        program_id: &Pubkey,
        my_info: &Pubkey,
        bump_seed: u8,
    ) -> Result<Pubkey, SwapError> {
        let (_key, _bump_seed) = Pubkey::find_program_address(&[&my_info.to_bytes()[..32]], program_id);
        if bump_seed != _bump_seed
        {
            return Err(SwapError::InvalidProgramAddress.into());
        }

        return Ok(_key)
    }

    /// Issue a spl_token `Burn` instruction.
    pub fn token_burn<'a>(
        swap: &Pubkey,
        token_program: AccountInfo<'a>,
        burn_account: AccountInfo<'a>,
        mint: AccountInfo<'a>,
        authority: AccountInfo<'a>,
        bump_seed: u8,
        amount: u64,
    ) -> Result<(), ProgramError> {
        let swap_bytes = swap.to_bytes();
        let authority_signature_seeds = [&swap_bytes[..32], &[bump_seed]];
        let signers = &[&authority_signature_seeds[..]];

        let ix = spl_token::instruction::burn(
            token_program.key,
            burn_account.key,
            mint.key,
            authority.key,
            &[],
            amount,
        )?;

        invoke_signed(
            &ix,
            &[burn_account, mint, authority, token_program],
            signers,
        )
    }

    /// Issue a spl_token `MintTo` instruction.
    pub fn token_mint_to<'a>(
        swap: &Pubkey,
        token_program: AccountInfo<'a>,
        mint: AccountInfo<'a>,
        destination: AccountInfo<'a>,
        authority: AccountInfo<'a>,
        bump_seed: u8,
        amount: u64,
    ) -> Result<(), ProgramError> {
        let swap_bytes = swap.to_bytes();
        let authority_signature_seeds = [&swap_bytes[..32], &[bump_seed]];
        let signers = &[&authority_signature_seeds[..]];
        let ix = spl_token::instruction::mint_to(
            token_program.key,
            mint.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?;

        invoke_signed(&ix, &[mint, destination, authority, token_program], signers)
    }

    /// Issue a spl_token `Transfer` instruction.
    pub fn token_transfer<'a>(
        swap: &Pubkey,
        token_program: AccountInfo<'a>,
        source: AccountInfo<'a>,
        destination: AccountInfo<'a>,
        authority: AccountInfo<'a>,
        bump_seed: u8,
        amount: u64,
    ) -> Result<(), ProgramError> {
        let swap_bytes = swap.to_bytes();
        let authority_signature_seeds = [&swap_bytes[..32], &[bump_seed]];
        let signers = &[&authority_signature_seeds[..]];
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke_signed(
            &ix,
            &[source, destination, authority, token_program],
            signers,
        )
    }

    /// Processes an [Initialize](enum.Instruction.html).
    pub fn process_initialize(
        program_id: &Pubkey,
        fees: Fees,
        swap_curve: SwapCurve,
        accounts: &[AccountInfo],
        swap_constraints: &Option<SwapConstraints>,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let swap_info = next_account_info(account_info_iter)?;
        let authority_info = next_account_info(account_info_iter)?;
        let token_a_info = next_account_info(account_info_iter)?;
        let token_b_info = next_account_info(account_info_iter)?;
        let pool_mint_info = next_account_info(account_info_iter)?;
        let fee_account_info = next_account_info(account_info_iter)?;
        let destination_info = next_account_info(account_info_iter)?;
        let token_program_info = next_account_info(account_info_iter)?;

        let token_program_id = *token_program_info.key;
        if SwapVersion::is_initialized(&swap_info.data.borrow()) {
            return Err(SwapError::AlreadyInUse.into());
        }

        let (swap_authority, bump_seed) =
            Pubkey::find_program_address(&[&swap_info.key.to_bytes()], program_id);
        if *authority_info.key != swap_authority {
            return Err(SwapError::InvalidProgramAddress.into());
        }
        let token_a = Self::unpack_token_account(token_a_info, &token_program_id)?;
        let token_b = Self::unpack_token_account(token_b_info, &token_program_id)?;
        let fee_account = Self::unpack_token_account(fee_account_info, &token_program_id)?;
        let destination = Self::unpack_token_account(destination_info, &token_program_id)?;
        let pool_mint = Self::unpack_mint(pool_mint_info, &token_program_id)?;
        if *authority_info.key != token_a.owner {
            return Err(SwapError::InvalidOwner.into());
        }
        if *authority_info.key != token_b.owner {
            return Err(SwapError::InvalidOwner.into());
        }
        if *authority_info.key == destination.owner {
            return Err(SwapError::InvalidOutputOwner.into());
        }
        if *authority_info.key == fee_account.owner {
            return Err(SwapError::InvalidOutputOwner.into());
        }
        if COption::Some(*authority_info.key) != pool_mint.mint_authority {
            return Err(SwapError::InvalidOwner.into());
        }

        if token_a.mint == token_b.mint {
            return Err(SwapError::RepeatedMint.into());
        }
        swap_curve
            .calculator
            .validate_supply(token_a.amount, token_b.amount)?;
        if token_a.delegate.is_some() {
            return Err(SwapError::InvalidDelegate.into());
        }
        if token_b.delegate.is_some() {
            return Err(SwapError::InvalidDelegate.into());
        }
        if token_a.close_authority.is_some() {
            return Err(SwapError::InvalidCloseAuthority.into());
        }
        if token_b.close_authority.is_some() {
            return Err(SwapError::InvalidCloseAuthority.into());
        }

        if pool_mint.supply != 0 {
            return Err(SwapError::InvalidSupply.into());
        }
        if pool_mint.freeze_authority.is_some() {
            return Err(SwapError::InvalidFreezeAuthority.into());
        }
        if *pool_mint_info.key != fee_account.mint {
            return Err(SwapError::IncorrectPoolMint.into());
        }

        if let Some(swap_constraints) = swap_constraints {
            let owner_key = swap_constraints
                .owner_key
                .parse::<Pubkey>()
                .map_err(|_| SwapError::InvalidOwner)?;
            if fee_account.owner != owner_key {
                return Err(SwapError::InvalidOwner.into());
            }
            swap_constraints.validate_curve(&swap_curve)?;
            swap_constraints.validate_fees(&fees)?;
        }
        fees.validate()?;
        swap_curve.calculator.validate()?;

        let initial_amount = swap_curve.calculator.new_pool_supply();

        Self::token_mint_to(
            swap_info.key,
            token_program_info.clone(),
            pool_mint_info.clone(),
            destination_info.clone(),
            authority_info.clone(),
            bump_seed,
            to_u64(initial_amount)?,
        )?;

        let obj = SwapVersion::SwapV1(SwapV1 {
            is_initialized: true,
            bump_seed,
            token_program_id,
            token_a: *token_a_info.key,
            token_b: *token_b_info.key,
            pool_mint: *pool_mint_info.key,
            token_a_mint: token_a.mint,
            token_b_mint: token_b.mint,
            pool_fee_account: *fee_account_info.key,
            fees,
            swap_curve,
        });
        SwapVersion::pack(obj, &mut swap_info.data.borrow_mut())?;
        Ok(())
    }

    /// Processes an [Swap](enum.Instruction.html).
    pub fn process_swap(
        _program_id: &Pubkey,
        _amount_in: u64,
        _minimum_amount_out: u64,
        _accounts: &[AccountInfo],
    ) -> ProgramResult {
        Ok(())
    }

    /// Processes an [DepositAllTokenTypes](enum.Instruction.html).
    pub fn process_deposit_all_token_types(
        _program_id: &Pubkey,
        _pool_token_amount: u64,
        _maximum_token_a_amount: u64,
        _maximum_token_b_amount: u64,
        _accounts: &[AccountInfo],
    ) -> ProgramResult {
        Ok(())
    }

    /// Processes an [WithdrawAllTokenTypes](enum.Instruction.html).
    pub fn process_withdraw_all_token_types(
        _program_id: &Pubkey,
        _pool_token_amount: u64,
        _minimum_token_a_amount: u64,
        _minimum_token_b_amount: u64,
        _accounts: &[AccountInfo],
    ) -> ProgramResult {
        Ok(())
    }


    /// Processes an [Instruction](enum.Instruction.html).
    pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
        Self::process_with_constraints(program_id, accounts, input, &SWAP_CONSTRAINTS)
    }

    /// Processes an instruction given extra constraint
    pub fn process_with_constraints(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        input: &[u8],
        swap_constraints: &Option<SwapConstraints>,
    ) -> ProgramResult {
        let instruction = SwapInstruction::unpack(input)?;
        match instruction {
            SwapInstruction::Initialize(Initialize { fees, swap_curve }) => {
                msg!("Instruction: Init");
                Self::process_initialize(program_id, fees, swap_curve, accounts, swap_constraints)
            }
            SwapInstruction::Swap(Swap {
                amount_in,
                minimum_amount_out,
            }) => {
                msg!("Instruction: Swap");
                Self::process_swap(program_id, amount_in, minimum_amount_out, accounts)
            }
            SwapInstruction::DepositAllTokenTypes(DepositAllTokenTypes {
                pool_token_amount,
                maximum_token_a_amount,
                maximum_token_b_amount,
            }) => {
                msg!("Instruction: DepositAllTokenTypes");
                Self::process_deposit_all_token_types(
                    program_id,
                    pool_token_amount,
                    maximum_token_a_amount,
                    maximum_token_b_amount,
                    accounts,
                )
            }
            SwapInstruction::WithdrawAllTokenTypes(WithdrawAllTokenTypes {
                pool_token_amount,
                minimum_token_a_amount,
                minimum_token_b_amount,
            }) => {
                msg!("Instruction: WithdrawAllTokenTypes");
                Self::process_withdraw_all_token_types(
                    program_id,
                    pool_token_amount,
                    minimum_token_a_amount,
                    minimum_token_b_amount,
                    accounts,
                )
            }
        }
    }
}

fn to_u64(val: u128) -> Result<u64, SwapError> {
    val.try_into().map_err(|_| SwapError::ConversionFailure)
}
