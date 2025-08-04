use pinocchio::sysvars::clock::Epoch;
// we do not provide Default because this requires the real current epoch
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StakeHistorySysvar(pub Epoch);

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
