//! Executes production ink WASM, never ink's cfg(test) offchain environment.
use frame_support::weights::Weight;
use ink_sandbox::{Account, Contracts, GAS, new_externalities};
use pallet_contracts::{Code, CollectEvents, DebugInfo, Determinism, ExecReturnValue};
use parity_scale_codec::{Decode, DecodeAll, Encode};
use sp_core::hashing::{blake2_256, sha2_256};
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
fn gas_exhaustion_invalid_input_and_unauthorized_call_do_not_mutate_contract() {
    let owner = account(4);
    let stranger = account(5);
    let mut ext = new_externalities(vec![owner.clone(), stranger.clone()]).unwrap();
    ext.execute_with(|| {
        let probe = instantiate("sandbox-probe", &owner, ());
        let failure = call(&stranger, &probe, "set_value", 2_u32);
        assert!(failure.did_revert());
        let result = Contracts::bare_call(
            owner.clone(),
            probe.clone(),
            0,
            Weight::zero(),
            None,
            input("set_value", 2_u32),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        );
        assert!(result.result.is_err());
        let invalid = call(&owner, &probe, "missing_selector", ());
        assert!(invalid.did_revert());
        // ink! 5.1.1 MessageResult::Err(LangError::CouldNotReadInput).
        assert_eq!(invalid.data, [1, 1]);
        assert_eq!(query::<u32>(&owner, &probe, "value", ()), 0);
        unit(&owner, &probe, "set_value", 2_u32);
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
        assert_eq!(output.data, [0, 1, 0]); // MessageResult::Ok(Err(Rejected))
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
        let output = call(&owner, &parent, "nested_failure", child.clone());
        assert!(output.did_revert());
        // Prove the child's domain error propagated through both dispatch layers.
        assert_eq!(output.data, [0, 1, 0]);
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
            Err(pallet_contracts::Error::<ink_sandbox::Runtime>::ContractTrapped.into())
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
            Err(pallet_contracts::Error::<ink_sandbox::Runtime>::OutOfGas.into())
        );
        assert!(exhausted.gas_consumed.all_lte(limit));
        assert_eq!(query::<u32>(&owner, &parent, "value", ()), 0);
    });
}
