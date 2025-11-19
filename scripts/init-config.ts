// apps/orchestrator/src/scripts/init-config.ts
import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { PublicKey, SystemProgram } from '@solana/web3.js';

import idl from '../../../target/idl/tipcoin.json' with { type: 'json' };
import { Tipcoin } from '../../../target/types/tipcoin.ts';

async function main() {
  const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID!);
  const TOKEN_MINT = new PublicKey(process.env.TOKEN_MINT_ADDRESS!);
  const RELAYER = new PublicKey(process.env.RELAYER_AUTHORITY!);
  const feeBpsEnv = process.env.FEE_BPS ?? '0';
  const FEE_BPS = Number.parseInt(feeBpsEnv, 10);

  if (!Number.isFinite(FEE_BPS) || FEE_BPS < 0) {
    throw new Error(`Invalid FEE_BPS value: ${feeBpsEnv}`);
  }

  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const programIdl = {
    ...(idl as anchor.Idl),
    address: PROGRAM_ID.toBase58(),
  } as anchor.Idl;
  const program = new anchor.Program(programIdl, provider) as Program<Tipcoin>;
  const [configPda] = PublicKey.findProgramAddressSync([Buffer.from('config')], PROGRAM_ID);

  console.log('Config PDA:', configPda.toBase58());

  await program.methods
    .initializeConfig({ relayer: RELAYER, tokenMint: TOKEN_MINT, feeBps: FEE_BPS })
    .accountsPartial({
      upgradeAuthority: provider.wallet.publicKey,
      config: configPda,
      systemProgram: SystemProgram.programId,
    })
    .rpc();

  console.log('Initialized config with feeBps=%d.', FEE_BPS);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
