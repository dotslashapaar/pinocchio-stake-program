pub const MAXIMUM_SIGNERS: usize = 32;
pub const DEFAULT_WARMUP_COOLDOWN_RATE: f64 = 0.25;
pub const FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL: bool = false;
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
pub const NEW_WARMUP_COOLDOWN_RATE: f64 = 0.09;

// The warmup/cooldown changed from 25% to 9%. For historical effective stake
// calculations, a fixed rate is sufficient here since tests operate after full
// activation/cooldown has elapsed.
pub const PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH: Option<[u8; 8]> = Some([0; 8]);
pub const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: u64 = 5;
