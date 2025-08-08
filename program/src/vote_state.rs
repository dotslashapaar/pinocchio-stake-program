use alloc::vec::Vec;                 
use pinocchio::pubkey::Pubkey;

#[repr(C)]
#[derive(Clone, Debug)]
pub struct VoteState {
    pub node_pubkey: Pubkey,
    pub authorized_withdrawer: Pubkey,
    pub commission: u8,
    pub votes: [u8; 1600],           
    pub root_slot: Option<u64>,
    pub epoch_credits: Vec<(u64, u64, u64)>,
}


mod vote_program {
    pinocchio_pubkey::declare_id!("Vote111111111111111111111111111111111111111");
}

pub fn vote_program_id() -> Pubkey {
    vote_program::id()
}


pub fn parse_epoch_credits(data: &[u8]) -> Option<Vec<(u64, u64, u64)>> {
    if data.len() % 24 != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 24 <= data.len() {
        let e = u64::from_le_bytes(data[i..i + 8].try_into().ok()?);
        let c = u64::from_le_bytes(data[i + 8..i + 16].try_into().ok()?);
        let p = u64::from_le_bytes(data[i + 16..i + 24].try_into().ok()?);
        out.push((e, c, p));
        i += 24;
    }
    Some(out)
}