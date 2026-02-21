#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, String,
};

use vision_records::{RecordType, Role, VisionRecordsContract, VisionRecordsContractClient};

fn setup() -> (
    Env,
    VisionRecordsContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(VisionRecordsContract, ());
    let client = VisionRecordsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let provider = Address::generate(&env);
    client.register_user(
        &admin,
        &provider,
        &Role::Optometrist,
        &String::from_str(&env, "Provider"),
    );

    let patient = Address::generate(&env);
    client.register_user(
        &admin,
        &patient,
        &Role::Patient,
        &String::from_str(&env, "Patient"),
    );

    (env, client, admin, provider, patient)
}

#[test]
fn test_record_history_tracks_versions_with_timestamps() {
    let (env, client, _admin, provider, patient) = setup();

    let first_hash = String::from_str(&env, "QmVersion1");
    let second_hash = String::from_str(&env, "QmVersion2");

    env.ledger().set_timestamp(100);
    let record_id = client.add_record(
        &provider,
        &patient,
        &provider,
        &RecordType::Examination,
        &first_hash,
    );

    assert_eq!(client.get_latest_record_version(&record_id), 1);

    env.ledger().set_timestamp(200);
    let next_version = client.update_record(&provider, &record_id, &second_hash);
    assert_eq!(next_version, 2);

    let history = client.get_record_history(&record_id);
    assert_eq!(history.len(), 2);

    let v1 = history.get(0).unwrap();
    let v2 = history.get(1).unwrap();

    assert_eq!(v1.version, 1);
    assert_eq!(v1.data_hash, first_hash);
    assert_eq!(v1.modified_by, provider);
    assert_eq!(v1.modified_at, 100);

    assert_eq!(v2.version, 2);
    assert_eq!(v2.data_hash, second_hash);
    assert_eq!(v2.modified_by, provider);
    assert_eq!(v2.modified_at, 200);
}

#[test]
fn test_version_comparison_reports_differences() {
    let (env, client, _admin, provider, patient) = setup();

    let first_hash = String::from_str(&env, "QmAlpha");
    let second_hash = String::from_str(&env, "QmBeta");

    let record_id = client.add_record(
        &provider,
        &patient,
        &provider,
        &RecordType::Diagnosis,
        &first_hash,
    );
    client.update_record(&provider, &record_id, &second_hash);

    let cmp = client.compare_record_versions(&record_id, &1, &2);
    assert!(cmp.changed);
    assert_eq!(cmp.from_data_hash, first_hash);
    assert_eq!(cmp.to_data_hash, second_hash);
}

#[test]
fn test_admin_can_rollback_to_previous_version() {
    let (env, client, admin, provider, patient) = setup();

    let first_hash = String::from_str(&env, "QmBefore");
    let second_hash = String::from_str(&env, "QmAfter");

    let record_id = client.add_record(
        &provider,
        &patient,
        &provider,
        &RecordType::Treatment,
        &first_hash,
    );
    client.update_record(&provider, &record_id, &second_hash);

    env.ledger().set_timestamp(300);
    let rollback_version = client.rollback_record(&admin, &record_id, &1);
    assert_eq!(rollback_version, 3);

    let current = client.get_record(&record_id);
    assert_eq!(current.data_hash, first_hash);

    let rolled = client.get_record_version(&record_id, &3);
    assert_eq!(rolled.data_hash, first_hash);
    assert_eq!(rolled.modified_by, admin);
    assert_eq!(rolled.modified_at, 300);
}

#[test]
fn test_non_admin_cannot_rollback() {
    let (env, client, _admin, provider, patient) = setup();

    let first_hash = String::from_str(&env, "QmStable");
    let second_hash = String::from_str(&env, "QmChanged");

    let record_id = client.add_record(
        &provider,
        &patient,
        &provider,
        &RecordType::Prescription,
        &first_hash,
    );
    client.update_record(&provider, &record_id, &second_hash);

    let result = client.try_rollback_record(&provider, &record_id, &1);
    match result {
        Ok(inner) => assert!(inner.is_err()),
        Err(_) => {}
    }
}

#[test]
fn test_version_query_by_number() {
    let (env, client, _admin, provider, patient) = setup();

    let first_hash = String::from_str(&env, "QmQuery1");
    let second_hash = String::from_str(&env, "QmQuery2");

    let record_id = client.add_record(
        &provider,
        &patient,
        &provider,
        &RecordType::LabResult,
        &first_hash,
    );
    client.update_record(&provider, &record_id, &second_hash);

    let version_one = client.get_record_version(&record_id, &1);
    let version_two = client.get_record_version(&record_id, &2);

    assert_eq!(version_one.data_hash, first_hash);
    assert_eq!(version_two.data_hash, second_hash);

    let missing = client.try_get_record_version(&record_id, &99);
    match missing {
        Ok(inner) => assert!(inner.is_err()),
        Err(_) => {}
    }
}
