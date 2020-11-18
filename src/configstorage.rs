use crate::kbucket;
use libp2p::kad::*;
use libp2p_core::PeerId;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::{hash_map, hash_set, HashMap, HashSet};
use std::iter;

pub struct MemoryStore {
    local_key: kbucket::Key<PeerId>,
    config: MemoryStoreConfig,
    records: HashMap<Key, Record>,
    providers: HashMap<Key, SmallVec<[ProviderRecord; K_VALUE.get()]>>,
    provided: HashSet<ProviderRecord>,
}

pub struct MemoryStoreConfig {
    pub max_records: usize,
    pub max_value_bytes: usize,
    pub max_providers_per_key: usize,
    pub max_provided_keys: usize,
}

impl Default for MemoryStoreConfig {
    fn default() -> Self {
        Self {
            max_records: 1024,
            max_value_bytes: 65 * 1024,
            max_provided_keys: 1024,
            max_providers_per_key: K_VALUE.get(),
        }
    }
}

impl MemoryStore {
    pub fn new(local_id: PeerId) -> Self {
        Self::with_config(local_id, Default::default())
    }

    pub fn with_config(local_id: PeerId, config: MemoryStoreConfig) -> Self {
        MemoryStore {
            local_key: kbucket::Key::new(local_id),
            config,
            records: HashMap::default(),
            provided: HashSet::default(),
            providers: HashMap::default(),
        }
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Key, &mut Record) -> bool,
    {
        self.records.retain(f);
    }
}

impl<'a> RecordStore<'a> for MemoryStore {
    type RecordsIter =
        iter::Map<hash_map::Values<'a, Key, Record>, fn(&'a Record) -> Cow<'a, Record>>;

    type ProvidedIter = iter::Map<
        hash_set::Iter<'a, ProviderRecord>,
        fn(&'a ProviderRecord) -> Cow<'a, ProviderRecord>,
    >;

    fn get(&'a self, k: &Key) -> Option<Cow<'_, Record>> {
        self.records.get(k).map(Cow::Borrowed)
    }

    fn put(&'a mut self, r: Record) -> Result<()> {
        if r.value.len() >= self.config.max_value_bytes {
            return Err(Error::ValueTooLarge);
        }

        let num_records = self.records.len();

        match self.records.entry(r.key.clone()) {
            hash_map::Entry::Occupied(mut e) => {
                e.insert(r);
            }
            hash_map::Entry::Vacant(e) => {
                if num_records >= self.config.max_records {
                    return Err(Error::MaxRecords);
                }
                e.insert(r);
            }
        }

        Ok(())
    }

    fn remove(&'a mut self, k: &Key) {
        self.records.remove(k);
    }

    fn records(&'a self) -> Self::RecordsIter {
        self.records.values().map(Cow::Borrowed)
    }

    fn add_provider(&'a mut self, record: ProviderRecord) -> Result<()> {
        let num_keys = self.providers.len();

        // Obtain the entry
        let providers = match self.providers.entry(record.key.clone()) {
            e @ hash_map::Entry::Occupied(_) => e,
            e @ hash_map::Entry::Vacant(_) => {
                if self.config.max_provided_keys == num_keys {
                    return Err(Error::MaxProvidedKeys);
                }
                e
            }
        }
        .or_insert_with(Default::default);

        if let Some(i) = providers.iter().position(|p| p.provider == record.provider) {
            providers.as_mut()[i] = record;
        } else {
            let local_key = self.local_key.clone();
            let key = kbucket::Key::new(record.key.clone());
            let provider = kbucket::Key::new(record.provider.clone());
            if let Some(i) = providers.iter().position(|p| {
                let pk = kbucket::Key::new(p.provider.clone());
                provider.distance(&key) < pk.distance(&key)
            }) {
                if local_key.preimage() == &record.provider {
                    self.provided.insert(record.clone());
                }
                providers.insert(i, record);
                // Remove the excess provider, if any.
                if providers.len() > self.config.max_providers_per_key {
                    if let Some(p) = providers.pop() {
                        self.provided.remove(&p);
                    }
                }
            } else if providers.len() < self.config.max_providers_per_key {
                if local_key.preimage() == &record.provider {
                    self.provided.insert(record.clone());
                }
                providers.push(record);
            }
        }
        Ok(())
    }

    fn providers(&'a self, key: &Key) -> Vec<ProviderRecord> {
        self.providers
            .get(key)
            .map_or_else(Vec::new, |ps| ps.clone().into_vec())
    }

    fn provided(&'a self) -> Self::ProvidedIter {
        self.provided.iter().map(Cow::Borrowed)
    }

    fn remove_provider(&'a mut self, key: &Key, provider: &PeerId) {
        if let hash_map::Entry::Occupied(mut e) = self.providers.entry(key.clone()) {
            let providers = e.get_mut();
            if let Some(i) = providers.iter().position(|p| &p.provider == provider) {
                let p = providers.remove(i);
                self.provided.remove(&p);
            }
            if providers.len() == 0 {
                e.remove();
            }
        }
    }
}
