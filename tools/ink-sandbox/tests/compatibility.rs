//! Executes production ink WASM, never ink's cfg(test) offchain environment.
use frame_support::{assert_ok, traits::fungibles::Inspect, weights::Weight};
use ink_v5_compatibility::{
    Account, Assets, Contracts, GAS, RuntimeOrigin, checkpoint::Checkpoint, new_externalities,
};
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism, ExecReturnValue};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use sp_core::hashing::{blake2_128, blake2_256, sha2_256};
use std::{fs, path::Path};

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn selector(name: &str) -> [u8; 4] {
    blake2_256(name.as_bytes())[..4].try_into().unwrap()
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut data = selector(name).to_vec();
    data.extend(args.encode());
    data
}
fn unhex(value: &str) -> Vec<u8> {
    value
        .strip_prefix("0x")
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn artifact(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}.contract"));
    let bytes = fs::read(path).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/manifest.json")).unwrap();
    let expected = manifest["contracts"][name]["artifactSha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        sha2_256(&bytes).to_vec(),
        unhex(&format!("0x{expected}")),
        "{name}: immutable artifact hash"
    );
    let lock = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/build-locks")
            .join(format!("{name}.lock")),
    )
    .unwrap();
    let expected = manifest["contracts"][name]["lockSha256"].as_str().unwrap();
    assert_eq!(
        sha2_256(&lock).to_vec(),
        unhex(&format!("0x{expected}")),
        "{name}: build lock hash"
    );
    let bundle: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bundle["source"]["language"], "ink! 5.1.1");
    let code = unhex(bundle["source"]["wasm"].as_str().unwrap());
    assert_eq!(
        blake2_256(&code).to_vec(),
        unhex(bundle["source"]["hash"].as_str().unwrap())
    );
    code
}
fn instantiate(name: &str, caller: &Account, args: impl Encode) -> Account {
    let result = Contracts::bare_instantiate(
        caller.clone(),
        0,
        GAS,
        None,
        Code::Upload(artifact(name)),
        input("new", args),
        name.as_bytes().to_vec(),
        DebugInfo::UnsafeDebug,
        CollectEvents::Skip,
    );
    let value = result.result.unwrap_or_else(|e| {
        panic!(
            "{name}: {e:?}; {}",
            String::from_utf8_lossy(&result.debug_message)
        )
    });
    assert!(!value.result.did_revert());
    value.account_id
}
fn call(caller: &Account, dest: &Account, name: &str, args: impl Encode) -> ExecReturnValue {
    let result = Contracts::bare_call(
        caller.clone(),
        dest.clone(),
        0,
        GAS,
        None,
        input(name, args),
        DebugInfo::UnsafeDebug,
        CollectEvents::Skip,
        Determinism::Enforced,
    );
    result.result.unwrap_or_else(|e| {
        panic!(
            "{name}: {e:?}; {}",
            String::from_utf8_lossy(&result.debug_message)
        )
    })
}
fn unit(caller: &Account, dest: &Account, name: &str, args: impl Encode) {
    let output = call(caller, dest, name, args);
    assert!(!output.did_revert(), "{name}: {:?}", output.data);
    assert_eq!(output.data, [0, 0], "{name}"); // ink MessageResult<domain Result<()>>
}
fn query<T: Decode>(caller: &Account, dest: &Account, name: &str, args: impl Encode) -> T {
    let output = call(caller, dest, name, args);
    assert!(!output.did_revert());
    Result::<T, u8>::decode_all(&mut &output.data[..])
        .unwrap()
        .unwrap()
}

#[test]
fn production_wasm_cross_calls_assets_reverts_and_full_restart() {
    let owner = account(1);
    let client = account(2);
    let coordinator = account(3);
    let mut ext =
        new_externalities(vec![owner.clone(), client.clone(), coordinator.clone()]).unwrap();
    let (calendar, ratings, project) = ext.execute_with(|| {
        assert_ok!(Assets::force_create(
            RuntimeOrigin::root(),
            1,
            owner.clone(),
            true,
            1
        ));
        assert_ok!(Assets::mint(
            RuntimeOrigin::signed(owner.clone()),
            1,
            client.clone(),
            50
        ));
        let ratings = instantiate("ratings", &owner, ());
        let calendar = instantiate("calendar", &owner, Some(ratings.clone()));
        unit(&owner, &calendar, "register_worker", coordinator.clone());
        // This succeeds only when Calendar's nested call really registered in Ratings.
        assert_eq!(
            query::<Vec<Account>>(&owner, &ratings, "get_registered_workers", ()),
            vec![coordinator.clone()]
        );
        unit(&coordinator, &calendar, "set_availability", 2_u8); // FullTime
        let project = instantiate(
            "projects",
            &owner,
            (
                "Compatibility".to_string(),
                client.clone(),
                owner.clone(),
                Some(calendar.clone()),
                Some(ratings.clone()),
            ),
        );
        let selected = call(&owner, &project, "assign_coordinator", ());
        assert!(!selected.did_revert());
        assert_eq!(selected.data, (0_u8, 0_u8, coordinator.clone()).encode());
        // Existing legacy scope API is only a compatibility fixture, not app behavior.
        unit(
            &coordinator,
            &project,
            "propose_scope",
            (
                vec![(1_u8, (0_u8, 1_u8), 100_u128, Vec::<u8>::new())],
                50_u8,
                [1_u8; 32],
            ),
        );
        let before = call(&client, &project, "get_scope_info", ()).data;
        // Preserve forbids spending the final asset unit. Failure follows task mutations.
        let failure = call(&client, &project, "approve_scope", vec![1_u8]);
        assert!(failure.did_revert());
        assert_eq!(&failure.data[..2], &[0, 1]); // language Ok, domain Err
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &client), 50);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &project), 0);
        assert_eq!(call(&client, &project, "get_scope_info", ()).data, before);
        assert_ok!(Assets::mint(
            RuntimeOrigin::signed(owner.clone()),
            1,
            client.clone(),
            51
        ));
        unit(&client, &project, "approve_scope", vec![1_u8]);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &client), 51);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &project), 50);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &owner), 0); // verified caller is payer, not factory
        (calendar, ratings, project)
    });

    let directory = std::env::temp_dir().join(format!(
        "ink-v5-proof-{}-{:x}",
        std::process::id(),
        u128::from_le_bytes(blake2_128(project.as_ref()))
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("runtime.scale");
    fs::write(&path, Checkpoint::capture(ext).unwrap().encode()).unwrap();
    let mut restored = Checkpoint::decode(&fs::read(&path).unwrap())
        .unwrap()
        .restore();
    fs::remove_dir_all(directory).unwrap();
    restored.execute_with(|| {
        // Contract Mapping data lives in child tries: top-level-only persistence fails here.
        assert_eq!(
            query::<u8>(
                &coordinator,
                &calendar,
                "get_availability_hours",
                coordinator.clone()
            ),
            40
        );
        assert_eq!(
            query::<Vec<Account>>(&owner, &ratings, "get_registered_workers", ()),
            vec![coordinator.clone()]
        );
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &project), 50);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &client), 51);
        let team = call(&coordinator, &project, "assign_team", 1_u8);
        assert!(!team.did_revert());
        unit(&coordinator, &project, "submit_task_for_review", 1_u8);
        unit(&client, &project, "complete_task", 1_u8);
        // Freeze outgoing escrow transfers after the advance. A later payout failure
        // must undo the successful remaining deposit AND earlier nested rating writes.
        assert_ok!(Assets::freeze(
            RuntimeOrigin::signed(owner.clone()),
            1,
            project.clone()
        ));
        let ratings_before = call(&owner, &ratings, "get_all_ratings", ()).data;
        let failure = call(
            &client,
            &project,
            "mark_completed",
            (vec![(coordinator.clone(), 80_u8)], 80_u8),
        );
        assert!(failure.did_revert());
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &client), 51);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &project), 50);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &coordinator), 0);
        assert_eq!(
            call(&owner, &ratings, "get_all_ratings", ()).data,
            ratings_before
        );
        assert_ok!(Assets::thaw(
            RuntimeOrigin::signed(owner.clone()),
            1,
            project.clone()
        ));
        unit(
            &client,
            &project,
            "mark_completed",
            (vec![(coordinator.clone(), 80_u8)], 80_u8),
        );
        // Legacy code deposits the remaining 50, reads contract balance and transfers 99.
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &client), 1);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &project), 1);
        assert_eq!(<Assets as Inspect<Account>>::balance(1, &coordinator), 99);
        assert_eq!(<Assets as Inspect<Account>>::total_issuance(1), 101);
    });
}

#[test]
fn gas_exhaustion_invalid_input_and_unauthorized_call_do_not_mutate_contract() {
    let owner = account(4);
    let stranger = account(5);
    let mut ext = new_externalities(vec![owner.clone(), stranger.clone()]).unwrap();
    ext.execute_with(|| {
        let calendar = instantiate("calendar", &owner, Option::<Account>::None);
        let failure = call(&stranger, &calendar, "register_worker", stranger.clone());
        assert!(failure.did_revert());
        let result = Contracts::bare_call(
            owner.clone(),
            calendar.clone(),
            0,
            Weight::zero(),
            None,
            input("register_worker", stranger.clone()),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        );
        assert!(result.result.is_err());
        let invalid = call(&owner, &calendar, "missing_selector", ());
        assert!(invalid.did_revert());
        assert!(query::<Vec<Account>>(&owner, &calendar, "get_registered_workers", ()).is_empty());
        unit(&owner, &calendar, "register_worker", stranger);
    });
}

#[test]
fn wasm_traps_nested_rejections_and_endless_execution_roll_back_child_storage() {
    let owner = account(6);
    let other = account(7);
    let mut ext = new_externalities(vec![owner.clone(), other.clone()]).unwrap();
    ext.execute_with(|| {
        let parent = instantiate("sandbox-probe", &owner, ());
        let child = instantiate("sandbox-probe", &other, ());
        let output = call(&owner, &parent, "write_then_revert", ());
        assert!(output.did_revert());
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
        let output = call(&owner, &parent, "nested_failure", child.clone());
        assert!(output.did_revert());
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
        assert_eq!(query::<u32>(&owner, &child, "value", ()), 0);
        let trap = Contracts::bare_call(
            owner.clone(),
            parent.clone(),
            0,
            GAS,
            None,
            input("write_then_trap", ()),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        );
        assert_eq!(
            trap.result,
            Err(pallet_contracts::Error::<ink_v5_compatibility::Runtime>::ContractTrapped.into())
        );
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
        let limit = Weight::from_parts(10_000_000_000, 16 * 1024 * 1024);
        let exhausted = Contracts::bare_call(
            owner.clone(),
            parent.clone(),
            0,
            limit,
            None,
            input("exhaust_gas", ()),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        );
        assert_eq!(
            exhausted.result,
            Err(pallet_contracts::Error::<ink_v5_compatibility::Runtime>::OutOfGas.into())
        );
        assert!(exhausted.gas_consumed.all_lte(limit));
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
    });
}
