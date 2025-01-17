use crate::util::atomic::Counter;

/// Statistics of a tree.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub success: TxnStats,
    pub restart: TxnStats,
}

#[derive(Default)]
pub(super) struct AtomicStats {
    pub(super) success: AtomicTxnStats,
    pub(super) restart: AtomicTxnStats,
}

impl AtomicStats {
    pub(super) fn snapshot(&self) -> Stats {
        Stats {
            success: self.success.snapshot(),
            restart: self.restart.snapshot(),
        }
    }
}

/// Statistics of tree transactions.
#[derive(Clone, Debug, Default)]
pub struct TxnStats {
    pub get: u64,
    pub write: u64,
    pub split_page: u64,
    pub consolidate_page: u64,
}

#[derive(Default)]
pub(super) struct AtomicTxnStats {
    pub(super) get: Counter,
    pub(super) write: Counter,
    pub(super) split_page: Counter,
    pub(super) consolidate_page: Counter,
}

impl AtomicTxnStats {
    pub(super) fn snapshot(&self) -> TxnStats {
        TxnStats {
            get: self.get.get(),
            write: self.write.get(),
            split_page: self.split_page.get(),
            consolidate_page: self.consolidate_page.get(),
        }
    }
}
