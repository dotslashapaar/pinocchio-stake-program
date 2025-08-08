use core::mem::size_of;
use pinocchio::sysvars::clock::Epoch;

// we do not provide Default because this requires the real current epoch
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StakeHistorySysvar(pub Epoch);
pub const MAX_STAKE_HISTORY_ENTRIES: usize = 512;

#[repr(C)]
#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistoryEntry {
    pub effective: [u8; 8],    // effective stake at this epoch
    pub activating: [u8; 8],   // sum of portion of stakes not fully warmed up
    pub deactivating: [u8; 8], // requested to be cooled down, not fully deactivated yet
}

pub trait StakeHistoryGetEntry {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry>;
}

#[macro_export]
macro_rules! impl_sysvar_id {
    ($type:ty) => {
        impl $crate::state::stake_history::SysvarId for $type {
            fn id() -> Pubkey {
                id()
            }

            fn check_id(pubkey: &Pubkey) -> bool {
                check_id(pubkey)
            }
        }
    };
}

#[macro_export]
macro_rules! declare_sysvar_id {
    ($name:expr, $type:ty) => {
        pinocchio_pubkey::declare_id!($name);
        $crate::impl_sysvar_id!($type);
    };
}

impl StakeHistoryEntry {
    pub const fn size() -> usize {
        size_of::<StakeHistoryEntry>()
    }
    pub fn with_effective(effective: u64) -> Self {
        Self {
            effective: effective.to_le_bytes(),
            ..Self::default()
        }
    }

    pub fn with_effective_and_activating(effective: u64, activating: u64) -> Self {
        Self {
            effective: effective.to_le_bytes(),
            activating: activating.to_le_bytes(),
            ..Self::default()
        }
    }

    pub fn with_deactivating(deactivating: u64) -> Self {
        Self {
            effective: deactivating.to_le_bytes(),
            deactivating: deactivating.to_le_bytes(),
            ..Self::default()
        }
    }
}

/// Complete stake history with fixed-size array
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct StakeHistory {
    /// Fixed-size array of stake history entries
    pub entries: [StakeHistoryEntry; MAX_STAKE_HISTORY_ENTRIES],
    /// Number of valid entries in the array
    pub len: usize,
}

impl StakeHistory {
    pub fn new() -> Self {
        Self {
            entries: core::array::from_fn(|_| StakeHistoryEntry {
                effective: [0u8; 8],
                activating: [0u8; 8],
                deactivating: [0u8; 8],
            }),
            len: 0,
        }
    }

    pub fn push(&mut self, entry: StakeHistoryEntry) -> Result<(), &'static str> {
        if self.len >= MAX_STAKE_HISTORY_ENTRIES {
            return Err("StakeHistory is full");
        }
        self.entries[self.len] = entry;
        self.len += 1;
        Ok(())
    }

    pub fn get(&self, index: usize) -> Option<&StakeHistoryEntry> {
        if index < self.len {
            Some(&self.entries[index])
        } else {
            None
        }
    }
}
