import * as anchor from '@coral-xyz/anchor';
import type { Program } from '@coral-xyz/anchor';
import {
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
} from '@solana/web3.js';
import assert from 'assert';
import BN from 'bn.js';
import { createHash } from 'node:crypto';

import idl from '../../../target/idl/tipcoin.json' with { type: 'json' };
import type { Tipcoin } from '../../../target/types/tipcoin.js';

const TOKEN_PROGRAM_ID = new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA');
const MINT_SIZE = 82;
const TOKEN_ACCOUNT_SIZE = 165;

const createInitializeMintInstruction = (
  mint: PublicKey,
  decimals: number,
  mintAuthority: PublicKey,
  freezeAuthority?: PublicKey,
): TransactionInstruction => {
  const data = Buffer.alloc(1 + 1 + 32 + 1 + 32);
  data[0] = 0; // InitializeMint instruction
  data[1] = decimals;
  mintAuthority.toBuffer().copy(data, 2);

  if (freezeAuthority) {
    data[34] = 1;
    freezeAuthority.toBuffer().copy(data, 35);
  }

  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: mint, isSigner: false, isWritable: true },
      { pubkey: anchor.web3.SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false },
    ],
    data,
  });
};

const createInitializeAccountInstruction = (
  account: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
): TransactionInstruction =>
  new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: account, isSigner: false, isWritable: true },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: anchor.web3.SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });

const createMintToInstruction = (
  mint: PublicKey,
  destination: PublicKey,
  authority: PublicKey,
  amount: bigint,
): TransactionInstruction => {
  const data = Buffer.alloc(1 + 8);
  data[0] = 7; // MintTo instruction
  data.writeBigUInt64LE(amount, 1);

  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: mint, isSigner: false, isWritable: true },
      { pubkey: destination, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    data,
  });
};

const createTokenAccount = async (
  provider: anchor.AnchorProvider,
  payer: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
): Promise<PublicKey> => {
  const accountKeypair = Keypair.generate();
  const rent = await provider.connection.getMinimumBalanceForRentExemption(TOKEN_ACCOUNT_SIZE);
  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer,
      newAccountPubkey: accountKeypair.publicKey,
      lamports: rent,
      space: TOKEN_ACCOUNT_SIZE,
      programId: TOKEN_PROGRAM_ID,
    }),
    createInitializeAccountInstruction(accountKeypair.publicKey, mint, owner),
  );
  await provider.sendAndConfirm(tx, [accountKeypair]);
  return accountKeypair.publicKey;
};

describe('tipcoin program', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const connection = provider.connection;
  const wallet = provider.wallet as anchor.Wallet;

  const programId = new PublicKey(
    (idl as { address?: string; metadata?: { address?: string } }).metadata?.address ??
      'AE9Tr3DN15nsb3RFy2KERfF9n5mwaEuAey4g2idW3YmT',
  );
  const programIdl = { ...(idl as anchor.Idl), address: programId.toBase58() } as anchor.Idl;
  const program = new anchor.Program(programIdl, provider) as Program<Tipcoin>;

  it('initializes config, registers a user, and processes a deposit', async () => {
    const [configPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('config')],
      program.programId,
    );

    let tokenMint: PublicKey;
    const feeBps = 50;
    const relayerKeypair = Keypair.generate();
    await connection.confirmTransaction(
      await connection.requestAirdrop(relayerKeypair.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL),
      'confirmed',
    );
    const relayer = relayerKeypair.publicKey;

    const existingConfigInfo = await connection.getAccountInfo(configPda);

    if (!existingConfigInfo) {
      const mintKeypair = Keypair.generate();
      const mintRent = await connection.getMinimumBalanceForRentExemption(MINT_SIZE);

      const createMintTx = new Transaction().add(
        SystemProgram.createAccount({
          fromPubkey: wallet.publicKey,
          newAccountPubkey: mintKeypair.publicKey,
          lamports: mintRent,
          space: MINT_SIZE,
          programId: TOKEN_PROGRAM_ID,
        }),
        createInitializeMintInstruction(mintKeypair.publicKey, 6, wallet.publicKey),
      );

      await provider.sendAndConfirm(createMintTx, [mintKeypair]);
      tokenMint = mintKeypair.publicKey;

      await program.methods
        .initializeConfig({ relayer, tokenMint, feeBps })
        .accountsPartial({ config: configPda })
        .rpc();
    } else {
      const existingConfig = await program.account.config.fetch(configPda);
      tokenMint = existingConfig.tokenMint;
      if (!existingConfig.relayer.equals(relayer)) {
        await program.methods
          .setRelayer(relayer)
          .accountsPartial({
            config: configPda,
            upgradeAuthority: wallet.publicKey,
          })
          .rpc();
      }
      if (existingConfig.feeBps !== feeBps) {
        await program.methods
          .setFeeRate(feeBps)
          .accountsPartial({
            config: configPda,
          })
          .rpc();
      }
    }

    const configAccount = await program.account.config.fetch(configPda);
    assert.strictEqual(configAccount.upgradeAuthority.toBase58(), wallet.publicKey.toBase58());
    assert.strictEqual(configAccount.relayer.toBase58(), relayer.toBase58());
    assert.strictEqual(configAccount.tokenMint.toBase58(), tokenMint.toBase58());
    assert.strictEqual(configAccount.feeBps, feeBps);

    const hashedDiscordSeed = `test-user-${Date.now()}-${Math.random()}`;
    const hashedUserId = createHash('sha256').update(hashedDiscordSeed).digest();
    const hashedDiscordArray = Array.from(hashedUserId);

    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('vault'), hashedUserId],
      program.programId,
    );
    const [allowancePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('allowance'), hashedUserId],
      program.programId,
    );

    await program.methods
      .register(hashedDiscordArray as number[])
      .accountsPartial({
        config: configPda,
        vault: vaultPda,
        allowance: allowancePda,
        tokenMint,
      })
      .rpc();

    const vaultAccount = await program.account.vault.fetch(vaultPda);
    const allowanceAccount = await program.account.allowance.fetch(allowancePda);

    assert.strictEqual(vaultAccount.authority.toBase58(), wallet.publicKey.toBase58());
    assert.strictEqual(vaultAccount.tokenMint.toBase58(), tokenMint.toBase58());
    assert.strictEqual(allowanceAccount.authority.toBase58(), wallet.publicKey.toBase58());

    const userTokenAccount = await createTokenAccount(
      provider,
      wallet.publicKey,
      tokenMint,
      wallet.publicKey,
    );
    const vaultTokenAccount = await createTokenAccount(
      provider,
      wallet.publicKey,
      tokenMint,
      vaultPda,
    );

    const mintTx = new Transaction().add(
      createMintToInstruction(tokenMint, userTokenAccount, wallet.publicKey, 1_000_000n),
    );
    await provider.sendAndConfirm(mintTx);

    const depositAmount = new BN(400_000);

    const depositEvents: Array<{ event: unknown; slot: number }> = [];
    const listener = program.addEventListener('depositEvent', (event, slot) => {
      depositEvents.push({ event, slot });
    });

    try {
      await program.methods
        .deposit(depositAmount)
        .accountsPartial({
          config: configPda,
          vault: vaultPda,
          authorityTokenAccount: userTokenAccount,
          vaultTokenAccount: vaultTokenAccount,
        })
        .rpc();
    } finally {
      await program.removeEventListener(listener);
    }

    const matchingEvent = depositEvents
      .map(
        (record) =>
          record.event as unknown as { vault: PublicKey; amount: BN; hashedUserId: number[] },
      )
      .find((event) => event.vault.equals(vaultPda));

    assert.ok(matchingEvent, 'DepositEvent not emitted');
    assert.strictEqual(matchingEvent!.amount.toString(), depositAmount.toString());
    assert.deepStrictEqual(
      matchingEvent!.hashedUserId,
      Array.from(hashedUserId),
      'Event hashed discord id mismatch',
    );

    const userBalance = await connection.getTokenAccountBalance(userTokenAccount);
    const vaultBalance = await connection.getTokenAccountBalance(vaultTokenAccount);

    assert.strictEqual(userBalance.value.amount, (1_000_000 - 400_000).toString());
    assert.strictEqual(vaultBalance.value.amount, '400000');

    const newAllowanceAmount = new BN(750_000);

    await program.methods
      .approveAllowance(newAllowanceAmount)
      .accountsPartial({
        allowance: allowancePda,
      })
      .rpc();

    const updatedAllowanceAccount = await program.account.allowance.fetch(allowancePda);
    assert.strictEqual(updatedAllowanceAccount.cap.toString(), newAllowanceAmount.toString());
    assert.strictEqual(updatedAllowanceAccount.remaining.toString(), newAllowanceAmount.toString());

    const recipientHashedSeed = `recipient-${Date.now()}-${Math.random()}`;
    const recipientHashedId = createHash('sha256').update(recipientHashedSeed).digest();
    const [recipientVaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('vault'), recipientHashedId],
      program.programId,
    );

    const recipientVaultTokenAccount = await createTokenAccount(
      provider,
      wallet.publicKey,
      tokenMint,
      recipientVaultPda,
    );

    const tipAmount = new BN(150_000);
    const tipId = Buffer.alloc(32, 9);
    const feeAmount = tipAmount.muln(feeBps).addn(9_999).divn(10_000);
    const totalTipCost = tipAmount.add(feeAmount);

    const [feeVaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('fee_vault'), configPda.toBuffer()],
      program.programId,
    );
    const feeVaultTokenAccount = await createTokenAccount(
      provider,
      wallet.publicKey,
      tokenMint,
      feeVaultPda,
    );

    const tipEvents: Array<{ event: unknown; slot: number }> = [];
    const tipListener = program.addEventListener('tipEvent', (event, slot) => {
      tipEvents.push({ event, slot });
    });

    try {
      await program.methods
        .tip(
          tipAmount,
          Array.from(tipId) as number[],
          null,
          Array.from(recipientHashedId) as number[],
        )
        .accountsPartial({
          config: configPda,
          relayer: relayerKeypair.publicKey,
          senderVault: vaultPda,
          senderAllowance: allowancePda,
          recipientVault: recipientVaultPda,
          senderVaultTokenAccount: vaultTokenAccount,
          recipientVaultTokenAccount,
          feeVault: feeVaultPda,
          feeVaultTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([relayerKeypair])
        .rpc();
    } finally {
      await program.removeEventListener(tipListener);
    }

    const senderVaultBalanceAfter = await connection.getTokenAccountBalance(vaultTokenAccount);
    const recipientVaultBalanceAfter = await connection.getTokenAccountBalance(
      recipientVaultTokenAccount,
    );

    assert.strictEqual(
      senderVaultBalanceAfter.value.amount,
      new BN(400_000).sub(totalTipCost).toString(),
      'Sender vault balance mismatch after tip',
    );
    assert.strictEqual(
      recipientVaultBalanceAfter.value.amount,
      '150000',
      'Recipient vault balance mismatch after tip',
    );
    const feeVaultBalanceAfter = await connection.getTokenAccountBalance(feeVaultTokenAccount);
    assert.strictEqual(
      feeVaultBalanceAfter.value.amount,
      feeAmount.toString(),
      'Fee vault balance mismatch after tip',
    );

    const userBalanceBeforeFeeWithdraw = await connection.getTokenAccountBalance(userTokenAccount);

    await program.methods
      .withdrawFee(feeAmount)
      .accountsPartial({
        config: configPda,
        upgradeAuthority: wallet.publicKey,
        feeVault: feeVaultPda,
        feeVaultTokenAccount,
        destinationTokenAccount: userTokenAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const feeVaultBalanceAfterWithdraw =
      await connection.getTokenAccountBalance(feeVaultTokenAccount);
    assert.strictEqual(
      feeVaultBalanceAfterWithdraw.value.amount,
      '0',
      'Fee vault balance mismatch after withdraw_fee',
    );
    const userBalanceAfterFeeWithdraw = await connection.getTokenAccountBalance(userTokenAccount);
    const expectedUserBalanceAfterFeeWithdraw = (
      BigInt(userBalanceBeforeFeeWithdraw.value.amount) + BigInt(feeAmount.toString())
    ).toString();
    assert.strictEqual(
      userBalanceAfterFeeWithdraw.value.amount,
      expectedUserBalanceAfterFeeWithdraw,
      'User token account did not receive withdrawn fees',
    );

    const recipientVaultAccount = await program.account.vault.fetch(recipientVaultPda);
    assert.deepStrictEqual(
      recipientVaultAccount.hashedUserId,
      Array.from(recipientHashedId),
      'Recipient vault hashed discord id mismatch after tip',
    );
    assert.strictEqual(
      recipientVaultAccount.tokenMint.toBase58(),
      tokenMint.toBase58(),
      'Recipient vault token mint mismatch after tip',
    );

    const allowanceAfterTip = await program.account.allowance.fetch(allowancePda);
    const expectedRemaining = newAllowanceAmount.sub(totalTipCost);
    assert.strictEqual(
      allowanceAfterTip.remaining.toString(),
      expectedRemaining.toString(),
      'Allowance remaining mismatch after tip',
    );

    const parsedTipEvent = tipEvents
      .map(
        (record) =>
          record.event as unknown as {
            senderVault: PublicKey;
            recipientVault: PublicKey;
            amount: BN;
            allowanceRemaining: BN;
            tipId: number[];
            feeAmount: BN;
            totalAmount: BN;
            feeBps: number;
          },
      )
      .find((event) => event.senderVault.equals(vaultPda));

    assert.ok(parsedTipEvent, 'TipEvent not emitted');
    assert.strictEqual(parsedTipEvent!.recipientVault.toBase58(), recipientVaultPda.toBase58());
    assert.strictEqual(parsedTipEvent!.amount.toString(), tipAmount.toString());
    assert.strictEqual(parsedTipEvent!.allowanceRemaining.toString(), expectedRemaining.toString());
    assert.deepStrictEqual(parsedTipEvent!.tipId, Array.from(tipId));
    assert.strictEqual(parsedTipEvent!.feeAmount.toString(), feeAmount.toString());
    assert.strictEqual(parsedTipEvent!.totalAmount.toString(), totalTipCost.toString());
    assert.strictEqual(parsedTipEvent!.feeBps, feeBps);
  });
});
