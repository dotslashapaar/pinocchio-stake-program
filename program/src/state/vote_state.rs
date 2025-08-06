use pinocchio::pubkey::Pubkey;

// fully defined VoteState (no Solana dependency)
#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct VoteState {
    pub node_pubkey: Pubkey,
    pub authorized_voter: Pubkey,
    pub commission: u8,
    pub credits: u64,
}

impl Default for VoteState {
    fn default() -> Self {
        Self {
            node_pubkey: Pubkey::default(),
            authorized_voter: Pubkey::default(),
            commission: 0,
            credits: 0,
        }
    }
}

impl VoteState {
    pub fn credits(&self) -> u64 {
        self.credits
    }
}
