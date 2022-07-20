#[derive(Debug)]
pub struct Page {
    header: PageHeader,
    content: PageContent,
}

impl Page {
    pub fn link(&mut self, next: PageRef<'_>) {
        self.header = PageHeader::with_next(next);
    }

    pub fn unlink(&mut self) {
        self.header.next = 0;
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        let mut next = self.header.next;
        while let Some(mut page) = PageBuf::from_usize(next) {
            if page.content.is_removed() {
                break;
            }
            next = page.header.next;
            page.header.next = 0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct PageHeader {
    len: usize,
    next: usize,
    epoch: u64,
}

impl PageHeader {
    fn new() -> Self {
        Self {
            len: 1,
            next: 0,
            epoch: 0,
        }
    }

    fn with_epoch(epoch: u64) -> Self {
        Self {
            len: 1,
            next: 0,
            epoch,
        }
    }

    fn with_next(next: PageRef<'_>) -> Self {
        let mut header = next.header().clone();
        header.len += 1;
        header.next = next.into_usize();
        header
    }

    fn with_next_epoch(next: PageRef<'_>) -> Self {
        let mut header = Self::with_next(next);
        header.epoch += 1;
        header
    }
}

#[derive(Debug)]
pub enum PageContent {
    BaseData(BaseData),
    DeltaData(DeltaData),
    SplitData(SplitNode),
    MergeData(MergeNode),
    RemoveData,
    BaseIndex(BaseIndex),
    DeltaIndex(DeltaIndex),
    SplitIndex(SplitNode),
    MergeIndex(MergeNode),
    RemoveIndex,
}

impl PageContent {
    pub fn is_data(&self) -> bool {
        match self {
            PageContent::BaseData(_)
            | PageContent::DeltaData(_)
            | PageContent::SplitData(_)
            | PageContent::MergeData(_)
            | PageContent::RemoveData => true,
            _ => false,
        }
    }

    pub fn is_removed(&self) -> bool {
        match self {
            PageContent::RemoveData | PageContent::RemoveIndex => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BaseData {
    size: usize,
    lowest: Vec<u8>,
    highest: Vec<u8>,
    records: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl BaseData {
    pub fn new() -> Self {
        Self {
            size: 0,
            lowest: Vec::new(),
            highest: Vec::new(),
            records: BTreeMap::new(),
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn lowest(&self) -> &[u8] {
        &self.lowest
    }

    pub fn highest(&self) -> &[u8] {
        &self.highest
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.records.get(key).map(|v| v.as_slice())
    }

    pub fn apply(&mut self, delta: DeltaData) {
        for (key, value) in delta.records {
            if let Some(value) = value {
                self.size += key.len() + value.len();
                if let Some(old_value) = self.records.insert(key, value) {
                    self.size -= old_value.len();
                }
            } else {
                if let Some(old_value) = self.records.remove(&key) {
                    self.size -= key.len() + old_value.len();
                }
            }
        }
    }

    pub fn split(&self) -> Option<BaseData> {
        let nth = (self.records.len() + 1) / 2;
        if let Some(key) = self.records.keys().nth(nth) {
            let mut right = BaseData::new();
            right.lowest = key.to_vec();
            right.highest = self.highest.clone();
            for (key, value) in self.records.iter().skip(nth) {
                right.size += key.len() + value.len();
                right.records.insert(key.clone(), value.clone());
            }
            Some(right)
        } else {
            None
        }
    }

    pub fn retain(&mut self, lowest: &[u8], highest: &[u8]) {
        self.lowest = lowest.to_vec();
        self.highest = highest.to_vec();
        self.records.retain(|k, _| {
            k.as_slice() >= lowest && (k.as_slice() < highest || highest.is_empty())
        });
        self.size = self
            .records
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + v.len());
    }
}

#[derive(Clone, Debug)]
pub struct DeltaData {
    records: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl DeltaData {
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Option<&[u8]>> {
        self.records
            .get(key)
            .map(|v| v.as_ref().map(|v| v.as_slice()))
    }

    pub fn add(&mut self, key: Vec<u8>, value: Option<Vec<u8>>) {
        self.records.insert(key, value);
    }

    pub fn merge(&mut self, other: DeltaData) {
        for (key, value) in other.records {
            self.records.entry(key).or_insert(value);
        }
    }
}

#[derive(Clone, Debug)]
pub struct PageIndex {
    pub lowest: Vec<u8>,
    pub highest: Vec<u8>,
    pub handle: PageHandle,
}

#[derive(Clone, Debug)]
pub struct PageHandle {
    pub id: PageId,
    pub epoch: u64,
}

#[derive(Clone, Debug)]
pub struct BaseIndex {
    size: usize,
    lowest: Vec<u8>,
    highest: Vec<u8>,
    children: BTreeMap<Vec<u8>, PageHandle>,
}

impl BaseIndex {
    pub fn new() -> Self {
        Self {
            size: 0,
            lowest: Vec::new(),
            highest: Vec::new(),
            children: BTreeMap::new(),
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn lowest(&self) -> &[u8] {
        &self.lowest
    }

    pub fn highest(&self) -> &[u8] {
        &self.highest
    }

    pub fn get(&self, key: &[u8]) -> Option<PageHandle> {
        self.children
            .range(..=key.to_owned())
            .next_back()
            .map(|(_, v)| v.clone())
    }

    pub fn add(&mut self, key: Vec<u8>, value: PageHandle) {
        self.size += key.len() + size_of_val(&value);
        if let Some(old_value) = self.children.insert(key, value) {
            self.size -= size_of_val(&old_value);
        }
    }

    pub fn apply(&mut self, delta: DeltaIndex) {
        for index in delta.children.into_iter().rev() {
            // Inserts the new index or merges with the previous one if possible.
            if let Some(handle) = self
                .children
                .range_mut(..=index.lowest.clone())
                .next_back()
                .map(|(_, v)| v)
            {
                if handle.id == index.handle.id {
                    handle.epoch = index.handle.epoch;
                } else {
                    self.children.insert(index.lowest.clone(), index.handle);
                }
            } else {
                self.children.insert(index.lowest.clone(), index.handle);
            }
            // Removes range (lowest, highest)
            self.children.retain(|k, _| {
                k <= &index.lowest || (k >= &index.highest && !index.highest.is_empty())
            });
        }
        self.size = self
            .children
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + size_of_val(v));
    }

    pub fn split(&self) -> Option<BaseIndex> {
        let nth = (self.children.len() + 1) / 2;
        if let Some(key) = self.children.keys().nth(nth) {
            let mut right = BaseIndex::new();
            right.lowest = key.to_vec();
            right.highest = self.highest.clone();
            for (key, value) in self.children.iter().skip(nth - 1) {
                right.size += key.len() + size_of_val(value);
                right.children.insert(key.clone(), value.clone());
            }
            Some(right)
        } else {
            None
        }
    }

    pub fn retain(&mut self, lowest: &[u8], highest: &[u8]) {
        self.lowest = lowest.to_vec();
        self.highest = highest.to_vec();
        self.children.retain(|k, _| {
            k.as_slice() >= lowest && (k.as_slice() < highest || highest.is_empty())
        });
        self.size = self
            .children
            .iter()
            .fold(0, |acc, (k, v)| acc + k.len() + size_of_val(v));
    }
}

#[derive(Clone, Debug)]
pub struct DeltaIndex {
    children: Vec<PageIndex>,
}

impl DeltaIndex {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<PageHandle> {
        for index in &self.children {
            if key >= &index.lowest && (key < &index.highest || index.highest.is_empty()) {
                return Some(index.handle.clone());
            }
        }
        None
    }

    pub fn add(&mut self, index: PageIndex) {
        self.children.push(index);
    }

    pub fn merge(&mut self, other: DeltaIndex) {
        self.children.extend(other.children);
    }
}

#[derive(Clone, Debug)]
pub struct SplitNode {
    pub lowest: Vec<u8>,
    pub middle: Vec<u8>,
    pub highest: Vec<u8>,
    pub right_page: PageHandle,
}

impl SplitNode {
    pub fn covers(&self, key: &[u8]) -> Option<PageHandle> {
        if key >= &self.middle && (key < &self.highest || self.highest.is_empty()) {
            Some(self.right_page.clone())
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct MergeNode {
    pub lowest: Vec<u8>,
    pub highest: Vec<u8>,
    pub right_page: PageBuf,
}