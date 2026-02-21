use soroban_sdk::{contracttype, symbol_short, Address, Env, String, Symbol, Vec};

const REC_HIST: Symbol = symbol_short!("REC_HIST");

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordVersion {
    pub record_id: u64,
    pub version: u32,
    pub data_hash: String,
    pub modified_by: Address,
    pub modified_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordComparison {
    pub record_id: u64,
    pub from_version: u32,
    pub to_version: u32,
    pub from_data_hash: String,
    pub to_data_hash: String,
    pub from_modified_at: u64,
    pub to_modified_at: u64,
    pub changed: bool,
}

fn history_key(record_id: u64) -> (Symbol, u64) {
    (REC_HIST, record_id)
}

pub fn get_history(env: &Env, record_id: u64) -> Vec<RecordVersion> {
    env.storage()
        .persistent()
        .get(&history_key(record_id))
        .unwrap_or(Vec::new(env))
}

pub fn latest_version(env: &Env, record_id: u64) -> Option<u32> {
    let history = get_history(env, record_id);
    if history.is_empty() {
        return None;
    }
    Some(history.len())
}

pub fn get_version(env: &Env, record_id: u64, version: u32) -> Option<RecordVersion> {
    let history = get_history(env, record_id);
    for item in history.iter() {
        if item.version == version {
            return Some(item);
        }
    }
    None
}

#[allow(clippy::arithmetic_side_effects)]
pub fn append_version(
    env: &Env,
    record_id: u64,
    data_hash: String,
    modified_by: Address,
    modified_at: u64,
) -> RecordVersion {
    let key = history_key(record_id);
    let mut history: Vec<RecordVersion> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));

    let next_version = history.len() + 1;
    let entry = RecordVersion {
        record_id,
        version: next_version,
        data_hash,
        modified_by,
        modified_at,
    };

    history.push_back(entry.clone());
    env.storage().persistent().set(&key, &history);

    entry
}

pub fn compare_versions(
    env: &Env,
    record_id: u64,
    from_version: u32,
    to_version: u32,
) -> Option<RecordComparison> {
    let from = get_version(env, record_id, from_version)?;
    let to = get_version(env, record_id, to_version)?;

    Some(RecordComparison {
        record_id,
        from_version,
        to_version,
        from_data_hash: from.data_hash.clone(),
        to_data_hash: to.data_hash.clone(),
        from_modified_at: from.modified_at,
        to_modified_at: to.modified_at,
        changed: from.data_hash != to.data_hash,
    })
}
