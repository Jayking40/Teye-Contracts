#![no_std]
pub mod events;
pub mod rbac;
pub mod versioning;

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec,
};

/// Storage keys for the contract
const ADMIN: Symbol = symbol_short!("ADMIN");
const INITIALIZED: Symbol = symbol_short!("INIT");

pub use rbac::{Permission, Role};
pub use versioning::{RecordComparison, RecordVersion};

/// Access levels for record sharing
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessLevel {
    None,
    Read,
    Write,
    Full,
}

/// Vision record types
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordType {
    Examination,
    Prescription,
    Diagnosis,
    Treatment,
    Surgery,
    LabResult,
}

/// User information structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct User {
    pub address: Address,
    pub role: Role,
    pub name: String,
    pub registered_at: u64,
    pub is_active: bool,
}

/// Vision record structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct VisionRecord {
    pub id: u64,
    pub patient: Address,
    pub provider: Address,
    pub record_type: RecordType,
    pub data_hash: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Access grant structure
#[contracttype]
#[derive(Clone, Debug)]
pub struct AccessGrant {
    pub patient: Address,
    pub grantee: Address,
    pub level: AccessLevel,
    pub granted_at: u64,
    pub expires_at: u64,
}

/// Contract errors
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    UserNotFound = 4,
    RecordNotFound = 5,
    InvalidInput = 6,
    AccessDenied = 7,
    Paused = 8,
    VersionNotFound = 9,
}

fn record_key(record_id: u64) -> (Symbol, u64) {
    (symbol_short!("RECORD"), record_id)
}

fn can_write_record(env: &Env, caller: &Address, provider: &Address) -> bool {
    if caller == provider {
        return rbac::has_permission(env, caller, &Permission::WriteRecord);
    }

    rbac::has_delegated_permission(env, provider, caller, &Permission::WriteRecord)
        || rbac::has_permission(env, caller, &Permission::SystemAdmin)
}

#[contract]
pub struct VisionRecordsContract;

#[contractimpl]
impl VisionRecordsContract {
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&INITIALIZED) {
            return Err(ContractError::AlreadyInitialized);
        }

        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&INITIALIZED, &true);
        rbac::assign_role(&env, admin.clone(), Role::Admin, 0);

        events::publish_initialized(&env, admin);

        Ok(())
    }

    /// Get the admin address
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&ADMIN)
            .ok_or(ContractError::NotInitialized)
    }

    /// Check if the contract is initialized
    pub fn is_initialized(env: Env) -> bool {
        env.storage().instance().has(&INITIALIZED)
    }

    /// Register a new user
    pub fn register_user(
        env: Env,
        caller: Address,
        user: Address,
        role: Role,
        name: String,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        if !rbac::has_permission(&env, &caller, &Permission::ManageUsers) {
            return Err(ContractError::Unauthorized);
        }

        let user_data = User {
            address: user.clone(),
            role: role.clone(),
            name: name.clone(),
            registered_at: env.ledger().timestamp(),
            is_active: true,
        };

        let key = (symbol_short!("USER"), user.clone());
        env.storage().persistent().set(&key, &user_data);
        rbac::assign_role(&env, user.clone(), role.clone(), 0);

        events::publish_user_registered(&env, user, role, name);

        Ok(())
    }

    /// Get user information
    pub fn get_user(env: Env, user: Address) -> Result<User, ContractError> {
        let key = (symbol_short!("USER"), user);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::UserNotFound)
    }

    /// Add a vision record
    #[allow(clippy::arithmetic_side_effects)]
    pub fn add_record(
        env: Env,
        caller: Address,
        patient: Address,
        provider: Address,
        record_type: RecordType,
        data_hash: String,
    ) -> Result<u64, ContractError> {
        caller.require_auth();

        if data_hash.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        if !can_write_record(&env, &caller, &provider) {
            return Err(ContractError::Unauthorized);
        }

        // Generate record ID
        let counter_key = symbol_short!("REC_CTR");
        let record_id: u64 = env.storage().instance().get(&counter_key).unwrap_or(0) + 1;
        env.storage().instance().set(&counter_key, &record_id);

        let record = VisionRecord {
            id: record_id,
            patient: patient.clone(),
            provider: provider.clone(),
            record_type: record_type.clone(),
            data_hash,
            created_at: env.ledger().timestamp(),
            updated_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&record_key(record_id), &record);

        // Add to patient's record list
        let patient_key = (symbol_short!("PAT_REC"), patient.clone());
        let mut patient_records: Vec<u64> = env
            .storage()
            .persistent()
            .get(&patient_key)
            .unwrap_or(Vec::new(&env));
        patient_records.push_back(record_id);
        env.storage()
            .persistent()
            .set(&patient_key, &patient_records);

        versioning::append_version(
            &env,
            record_id,
            record.data_hash.clone(),
            caller,
            env.ledger().timestamp(),
        );

        events::publish_record_added(&env, record_id, patient, provider, record_type);

        Ok(record_id)
    }

    /// Update an existing record, creating a new version entry.
    pub fn update_record(
        env: Env,
        caller: Address,
        record_id: u64,
        data_hash: String,
    ) -> Result<u32, ContractError> {
        caller.require_auth();

        if data_hash.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        let mut record: VisionRecord = env
            .storage()
            .persistent()
            .get(&record_key(record_id))
            .ok_or(ContractError::RecordNotFound)?;

        if !can_write_record(&env, &caller, &record.provider) {
            return Err(ContractError::Unauthorized);
        }

        record.data_hash = data_hash.clone();
        record.updated_at = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&record_key(record_id), &record);

        let version = versioning::append_version(
            &env,
            record_id,
            data_hash,
            caller,
            env.ledger().timestamp(),
        )
        .version;

        Ok(version)
    }

    /// Roll back record content to a previous version. Admin-only.
    pub fn rollback_record(
        env: Env,
        caller: Address,
        record_id: u64,
        target_version: u32,
    ) -> Result<u32, ContractError> {
        caller.require_auth();

        if !rbac::has_permission(&env, &caller, &Permission::SystemAdmin) {
            return Err(ContractError::Unauthorized);
        }

        let target = versioning::get_version(&env, record_id, target_version)
            .ok_or(ContractError::VersionNotFound)?;

        let mut record: VisionRecord = env
            .storage()
            .persistent()
            .get(&record_key(record_id))
            .ok_or(ContractError::RecordNotFound)?;

        record.data_hash = target.data_hash.clone();
        record.updated_at = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&record_key(record_id), &record);

        let new_version = versioning::append_version(
            &env,
            record_id,
            target.data_hash,
            caller,
            env.ledger().timestamp(),
        )
        .version;

        Ok(new_version)
    }

    /// Get a vision record by ID
    pub fn get_record(env: Env, record_id: u64) -> Result<VisionRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&record_key(record_id))
            .ok_or(ContractError::RecordNotFound)
    }

    /// Get all records for a patient
    pub fn get_patient_records(env: Env, patient: Address) -> Vec<u64> {
        let key = (symbol_short!("PAT_REC"), patient);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env))
    }

    /// Query all historical versions for a record.
    pub fn get_record_history(
        env: Env,
        record_id: u64,
    ) -> Result<Vec<RecordVersion>, ContractError> {
        if !env.storage().persistent().has(&record_key(record_id)) {
            return Err(ContractError::RecordNotFound);
        }

        Ok(versioning::get_history(&env, record_id))
    }

    /// Query a specific historical version for a record.
    pub fn get_record_version(
        env: Env,
        record_id: u64,
        version: u32,
    ) -> Result<RecordVersion, ContractError> {
        if !env.storage().persistent().has(&record_key(record_id)) {
            return Err(ContractError::RecordNotFound);
        }

        versioning::get_version(&env, record_id, version).ok_or(ContractError::VersionNotFound)
    }

    /// Query the latest version number of a record.
    pub fn get_latest_record_version(env: Env, record_id: u64) -> Result<u32, ContractError> {
        if !env.storage().persistent().has(&record_key(record_id)) {
            return Err(ContractError::RecordNotFound);
        }

        versioning::latest_version(&env, record_id).ok_or(ContractError::VersionNotFound)
    }

    /// Compare two versions of a record.
    pub fn compare_record_versions(
        env: Env,
        record_id: u64,
        from_version: u32,
        to_version: u32,
    ) -> Result<RecordComparison, ContractError> {
        if !env.storage().persistent().has(&record_key(record_id)) {
            return Err(ContractError::RecordNotFound);
        }

        versioning::compare_versions(&env, record_id, from_version, to_version)
            .ok_or(ContractError::VersionNotFound)
    }

    /// Grant access to a user
    #[allow(clippy::arithmetic_side_effects)]
    pub fn grant_access(
        env: Env,
        caller: Address,
        patient: Address,
        grantee: Address,
        level: AccessLevel,
        duration_seconds: u64,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let has_perm = if caller == patient {
            true // Patient manages own access
        } else {
            rbac::has_delegated_permission(&env, &patient, &caller, &Permission::ManageAccess)
                || rbac::has_permission(&env, &caller, &Permission::SystemAdmin)
        };

        if !has_perm {
            return Err(ContractError::Unauthorized);
        }

        let expires_at = env.ledger().timestamp() + duration_seconds;
        let grant = AccessGrant {
            patient: patient.clone(),
            grantee: grantee.clone(),
            level: level.clone(),
            granted_at: env.ledger().timestamp(),
            expires_at,
        };

        let key = (symbol_short!("ACCESS"), patient.clone(), grantee.clone());
        env.storage().persistent().set(&key, &grant);

        events::publish_access_granted(&env, patient, grantee, level, duration_seconds, expires_at);

        Ok(())
    }

    /// Check access level
    pub fn check_access(env: Env, patient: Address, grantee: Address) -> AccessLevel {
        let key = (symbol_short!("ACCESS"), patient, grantee);

        if let Some(grant) = env.storage().persistent().get::<_, AccessGrant>(&key) {
            if grant.expires_at > env.ledger().timestamp() {
                return grant.level;
            }
        }

        AccessLevel::None
    }

    /// Revoke access
    pub fn revoke_access(
        env: Env,
        patient: Address,
        grantee: Address,
    ) -> Result<(), ContractError> {
        patient.require_auth();

        let key = (symbol_short!("ACCESS"), patient.clone(), grantee.clone());
        env.storage().persistent().remove(&key);

        events::publish_access_revoked(&env, patient, grantee);

        Ok(())
    }

    /// Get the total number of records
    pub fn get_record_count(env: Env) -> u64 {
        let counter_key = symbol_short!("REC_CTR");
        env.storage().instance().get(&counter_key).unwrap_or(0)
    }

    /// Contract version
    pub fn version() -> u32 {
        1
    }

    // ======================== RBAC Endpoints ========================

    pub fn grant_custom_permission(
        env: Env,
        caller: Address,
        user: Address,
        permission: Permission,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if !rbac::has_permission(&env, &caller, &Permission::ManageUsers) {
            return Err(ContractError::Unauthorized);
        }
        rbac::grant_custom_permission(&env, user, permission)
            .map_err(|_| ContractError::UserNotFound)?;
        Ok(())
    }

    pub fn revoke_custom_permission(
        env: Env,
        caller: Address,
        user: Address,
        permission: Permission,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if !rbac::has_permission(&env, &caller, &Permission::ManageUsers) {
            return Err(ContractError::Unauthorized);
        }
        rbac::revoke_custom_permission(&env, user, permission)
            .map_err(|_| ContractError::UserNotFound)?;
        Ok(())
    }

    pub fn delegate_role(
        env: Env,
        delegator: Address,
        delegatee: Address,
        role: Role,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        delegator.require_auth();
        rbac::delegate_role(&env, delegator, delegatee, role, expires_at);
        Ok(())
    }

    pub fn check_permission(env: Env, user: Address, permission: Permission) -> bool {
        rbac::has_permission(&env, &user, &permission)
    }
}

#[cfg(test)]
mod test_rbac;
