use crate::util::RelaxedCounter;

#[derive(Default, Debug)]
pub struct Stats {
    pub cache_size: u64,
    pub op_succeeded: OpStats,
    pub op_conflicted: OpStats,
    pub smo_succeeded: SmoStats,
    pub smo_conflicted: SmoStats,
    pub num_visit_pages: u64,
}

#[derive(Default)]
pub struct AtomicStats {
    pub op_succeeded: AtomicOpStats,
    pub op_conflicted: AtomicOpStats,
    pub smo_succeeded: AtomicSmoStats,
    pub smo_conflicted: AtomicSmoStats,
    pub num_visit_pages: RelaxedCounter,
}

#[derive(Default, Debug)]
pub struct OpStats {
    pub num_gets: u64,
    pub num_inserts: u64,
}

#[derive(Default)]
pub struct AtomicOpStats {
    pub num_gets: RelaxedCounter,
    pub num_inserts: RelaxedCounter,
}

impl AtomicOpStats {
    pub fn snapshot(&self) -> OpStats {
        OpStats {
            num_gets: self.num_gets.get(),
            num_inserts: self.num_inserts.get(),
        }
    }
}

#[derive(Default, Debug)]
pub struct SmoStats {
    pub num_data_splits: u64,
    pub num_data_consolidates: u64,
    pub num_index_splits: u64,
    pub num_index_consolidates: u64,
}

#[derive(Default)]
pub struct AtomicSmoStats {
    pub num_data_splits: RelaxedCounter,
    pub num_data_consolidates: RelaxedCounter,
    pub num_index_splits: RelaxedCounter,
    pub num_index_consolidates: RelaxedCounter,
}

impl AtomicSmoStats {
    pub fn snapshot(&self) -> SmoStats {
        SmoStats {
            num_data_splits: self.num_data_splits.get(),
            num_data_consolidates: self.num_data_consolidates.get(),
            num_index_splits: self.num_index_splits.get(),
            num_index_consolidates: self.num_index_consolidates.get(),
        }
    }
}
