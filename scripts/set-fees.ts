import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { PublicKey } from '@solana/web3.js';

import idl from '../target/idl/tipcoin.json' with { type: 'json' };
import { Tipcoin } from '../target/types/tipcoin.ts';

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const programId = new PublicKey(process.env.PROGRAM_ID!);
  const programIdl = {
    ...(idl as anchor.Idl),
    address: programId.toBase58(),
  } as anchor.Idl;
  const program = new anchor.Program(programIdl, provider) as Program<Tipcoin>;
  const [configPda] = PublicKey.findProgramAddressSync([Buffer.from('config')], programId);

  const txSig = await program.methods
    .setFeeRate(50) // 50 bps = 0.5%
    .accounts({
      config: configPda,
      upgradeAuthority: provider.wallet.publicKey,
    })
    .rpc();

  console.log('Fee updated to 0.5% (50 bps). Signature:', txSig);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
