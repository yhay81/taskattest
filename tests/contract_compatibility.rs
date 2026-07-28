use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use taskattest::execute::build_receipt;
use taskattest::model::Receipt;
use taskattest::source::sha256_bytes;
use taskattest::store::StateStore;
use taskattest::verify::verify_receipt;

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contracts/v1")
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn state_store() -> (tempfile::TempDir, StateStore) {
    let directory = tempfile::tempdir().expect("state directory");
    let root = directory.path().join("state");
    fs::create_dir_all(root.join("receipts")).expect("receipt directory");
    fs::create_dir_all(root.join("blobs").join("sha256")).expect("blob directory");
    let store = StateStore::open_existing(&root).expect("open fixture state store");
    (directory, store)
}

fn apply_mutation(document: &mut Value, operation: &str, pointer: &str, value: Value) {
    match operation {
        "replace" => {
            let target = document
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("replace target {pointer} exists"));
            *target = value;
        }
        "add" => {
            let (parent, key) = object_parent(document, pointer);
            assert!(
                parent.insert(key.clone(), value).is_none(),
                "add target {pointer} must not already exist"
            );
        }
        "remove" => {
            let (parent, key) = object_parent(document, pointer);
            assert!(
                parent.remove(&key).is_some(),
                "remove target {pointer} must exist"
            );
        }
        other => panic!("unsupported corpus mutation operation {other}"),
    }
}

fn object_parent<'a>(
    document: &'a mut Value,
    pointer: &str,
) -> (&'a mut serde_json::Map<String, Value>, String) {
    let (parent_pointer, encoded_key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("object pointer {pointer} has a parent"));
    let parent = if parent_pointer.is_empty() {
        document
    } else {
        document
            .pointer_mut(parent_pointer)
            .unwrap_or_else(|| panic!("object parent {parent_pointer} exists"))
    };
    let key = encoded_key.replace("~1", "/").replace("~0", "~");
    (
        parent
            .as_object_mut()
            .unwrap_or_else(|| panic!("object parent {parent_pointer} is an object")),
        key,
    )
}

#[test]
fn current_reader_preserves_and_verifies_v1_receipts() {
    let root = corpus_root();
    let manifest = read_json(&root.join("manifest.json"));
    assert_eq!(manifest["schema_version"], "taskattest.contract-corpus/v1");
    let mut declared_paths = BTreeSet::new();

    for entry in manifest["accepted"]
        .as_array()
        .expect("accepted corpus entries")
    {
        let relative_path = entry["path"].as_str().expect("accepted path");
        assert!(
            declared_paths.insert(relative_path.to_owned()),
            "duplicate accepted fixture {relative_path}"
        );
        let path = root.join(relative_path);
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("read accepted fixture {relative_path}: {error}"));
        assert_eq!(
            sha256_bytes(&bytes),
            entry["sha256"].as_str().expect("accepted SHA-256"),
            "{relative_path} digest changed"
        );
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("parse accepted fixture {relative_path}: {error}"));
        assert_eq!(
            value["schema_version"], entry["schema_version"],
            "{relative_path} schema version"
        );

        let receipt = StateStore::read_receipt_file(&path)
            .unwrap_or_else(|error| panic!("read receipt {relative_path}: {error}"));
        let serialized = format!(
            "{}\n",
            serde_json::to_string_pretty(&receipt).expect("serialize accepted receipt")
        );
        assert_eq!(
            serialized,
            String::from_utf8(bytes).expect("UTF-8 receipt"),
            "{relative_path} is not the exact stable serialization"
        );
        let rebuilt = build_receipt(receipt.payload.clone()).expect("rebuild accepted receipt");
        assert_eq!(rebuilt.receipt_id, receipt.receipt_id);
        assert_eq!(rebuilt.canonical_digest, receipt.canonical_digest);
        let (_directory, store) = state_store();
        let report = verify_receipt(&receipt, &store).expect("verify accepted receipt");
        assert!(report.valid, "{relative_path}: {:?}", report.problems);
    }

    let discovered_paths = fs::read_dir(&root)
        .expect("read corpus directory")
        .map(|entry| entry.expect("read corpus entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && path.file_name().is_some_and(|name| name != "manifest.json")
        })
        .map(|path| {
            path.file_name()
                .expect("fixture file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        discovered_paths, declared_paths,
        "every accepted JSON fixture must be digest-pinned in the manifest"
    );
}

#[test]
fn declared_v1_mutations_fail_closed() {
    let root = corpus_root();
    let manifest = read_json(&root.join("manifest.json"));
    let mut rejection_ids = BTreeSet::new();

    for case in manifest["rejections"]
        .as_array()
        .expect("rejection corpus entries")
    {
        let id = case["id"].as_str().expect("rejection id");
        assert!(
            rejection_ids.insert(id.to_owned()),
            "duplicate rejection id {id}"
        );
        let mut document = read_json(&root.join(case["base"].as_str().expect("base fixture")));
        apply_mutation(
            &mut document,
            case["operation"].as_str().expect("mutation operation"),
            case["pointer"].as_str().expect("mutation pointer"),
            case["value"].clone(),
        );
        if case["rebind_identity"].as_bool().unwrap_or(false) {
            let mutated: Receipt =
                serde_json::from_value(document).expect("deserialize rebindable mutation");
            document = serde_json::to_value(
                build_receipt(mutated.payload).expect("rebind mutated payload"),
            )
            .expect("serialize rebound receipt");
        }

        let directory = tempfile::tempdir().expect("mutation directory");
        let path = directory.path().join(format!("{id}.json"));
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&document).expect("serialize mutation")
            ),
        )
        .expect("write mutation");

        match case["stage"].as_str().expect("rejection stage") {
            "reader" => {
                let error = StateStore::read_receipt_file(&path)
                    .expect_err("reader mutation must be rejected");
                assert_eq!(
                    error.code,
                    case["expected_error_code"].as_str().expect("error code"),
                    "rejection {id}: {}",
                    case["reason"].as_str().expect("rejection reason")
                );
            }
            "verifier" => {
                let receipt = StateStore::read_receipt_file(&path).expect("read verifier mutation");
                let (_state_directory, store) = state_store();
                let report = verify_receipt(&receipt, &store).expect("verify mutation");
                let expected_problem = case["expected_problem"].as_str().expect("expected problem");
                assert!(!report.valid, "rejection {id} must be invalid");
                assert!(
                    report
                        .problems
                        .iter()
                        .any(|problem| problem.contains(expected_problem)),
                    "rejection {id} did not report {expected_problem:?}: {:?}",
                    report.problems
                );
            }
            other => panic!("unsupported rejection stage {other}"),
        }
    }
}
