//! Disposable-media acceptance coverage for the native installer boundary.
//!
//! These tests deliberately use serialized snapshots and typed plans. They
//! exercise the same Rust modules used by the live daemon while ensuring that
//! acceptance runs cannot discover or mutate a real block device.

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{json, Value};

    use crate::installer_bootc::{self, BootcInstallInput};
    use crate::installer_disk::{self, DiskOperationInput};
    use crate::installer_orchestration::{self, OrchestrationInput};
    use crate::installer_plan::{self, InstallerPlanInput};
    use crate::installer_recovery;
    use crate::installer_secure_boot::{self, SecureBootInput};
    use crate::installer_storage;
    use crate::installer_transaction::{self, TransactionSource, TransactionState};

    const LSBLK: &str = include_str!("../testdata/lsblk_snapshot.json");

    fn plan_input(value: &Value) -> InstallerPlanInput {
        serde_json::from_value(value.clone()).expect("installer plan fixture input")
    }

    #[test]
    fn disposable_media_plan_matrix_covers_all_install_modes() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../testdata/installer_plan_cases.json"))
                .expect("installer plan fixture must be valid JSON");

        for case in cases {
            let name = case["name"].as_str().expect("fixture case name");
            let result = installer_plan::build_plan(plan_input(&case["input"]));
            if let Some(expected_error) = case.get("error_contains") {
                let error = result.expect_err(name);
                assert!(
                    error.contains(expected_error.as_str().unwrap()),
                    "{name}: {error}"
                );
            } else {
                let plan = result.unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(
                    serde_json::to_value(plan).expect("plan serializes"),
                    case["expected"],
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn disposable_media_disk_operations_are_fixed_and_confirmed() {
        let cases = vec![
            (
                DiskOperationInput::CreateLabel {
                    disk: "/dev/sda".into(),
                    table_type: "gpt".into(),
                },
                "/usr/sbin/parted",
                false,
            ),
            (
                DiskOperationInput::CreatePartition {
                    disk: "/dev/sda".into(),
                    start: 1_048_576,
                    size: 4 * 1024 * 1024,
                    fs: "btrfs".into(),
                    label: "KythRoot".into(),
                    sector_size: 512,
                },
                "/usr/sbin/parted",
                false,
            ),
            (
                DiskOperationInput::DeletePartition {
                    disk: "/dev/sda".into(),
                    part_num: 2,
                },
                "/usr/sbin/parted",
                false,
            ),
            (
                DiskOperationInput::ResizePartition {
                    disk: "/dev/sda".into(),
                    part_num: 2,
                    start: 1_048_576,
                    new_size: 4 * 1024 * 1024,
                    sector_size: 512,
                },
                "/usr/sbin/parted",
                true,
            ),
            (
                DiskOperationInput::FilesystemResize {
                    device: "/dev/sda2".into(),
                    fs: "ntfs".into(),
                    new_size_bytes: 40 * 1024 * 1024 * 1024,
                    stage: "dry_run".into(),
                },
                "/usr/sbin/ntfsresize",
                false,
            ),
            (
                DiskOperationInput::FormatFilesystem {
                    device: "/dev/sda2".into(),
                    fs: "ext4".into(),
                    label: "KythRoot".into(),
                },
                "/usr/sbin/mkfs.ext4",
                false,
            ),
        ];

        for (operation, executable, needs_confirmation) in cases {
            let plan = installer_disk::build_plan(operation).expect("safe disk plan");
            assert_eq!(plan.argv.first().map(String::as_str), Some(executable));
            assert_eq!(plan.needs_confirmation, needs_confirmation);
            assert!(plan.argv.iter().all(|argument| !argument.contains("..")));
        }
    }

    #[test]
    fn snapshot_identity_rejects_stale_names_and_accepts_matching_geometry() {
        let disks = installer_storage::parse_disks(LSBLK, &[], Some("/dev/sda"))
            .expect("disposable snapshot should parse");
        assert_eq!(disks.len(), 2);
        assert!(disks.iter().any(|disk| disk.current));

        let new_start_sectors = 4_000_000_u64;
        let new_size_bytes = 40 * 1024 * 1024 * 1024_u64;
        let mut after: Value = serde_json::from_str(LSBLK).expect("snapshot JSON");
        after["blockdevices"][0]["children"]
            .as_array_mut()
            .expect("disk children")
            .push(json!({
                "name": "/dev/sda3",
                "size": new_size_bytes,
                "type": "part",
                "fstype": "btrfs",
                "start": new_start_sectors
            }));
        let after = serde_json::to_string(&after).expect("snapshot serializes");
        assert_eq!(
            installer_storage::new_partition_from_snapshots(
                LSBLK,
                &after,
                new_start_sectors * 512,
                new_size_bytes,
            )
            .expect("geometry identifies one new partition"),
            "/dev/sda3"
        );
        assert!(installer_storage::new_partition_from_snapshots(
            LSBLK,
            LSBLK,
            new_start_sectors * 512,
            new_size_bytes
        )
        .is_err());
    }

    #[test]
    fn cancellation_and_failure_boundaries_are_recovery_safe() {
        let cancel = installer_orchestration::apply(OrchestrationInput {
            action: "cancel-check".into(),
            lifecycle: "installing".into(),
            phase: "storage".into(),
            target: String::new(),
            cancel_requested: true,
            slot_held: true,
            status: String::new(),
            next_status: String::new(),
        })
        .expect("destructive cancellation is a normal response");
        assert!(cancel.cancelled);
        assert!(cancel.cancel_message.contains("may have already"));

        let directory = tempfile::tempdir().expect("transaction directory");
        let failure_path = directory.path().join("failure.json");
        let state = TransactionState {
            schema_version: 1,
            transaction_id: "acceptance".into(),
            job_id: Some(7),
            updated_at: String::new(),
            status: "storage_complete".into(),
            phase: "image".into(),
            lifecycle: "installing".into(),
            install_mode: "wipe".into(),
            disk: "/dev/sda".into(),
            target_partition: String::new(),
            source: TransactionSource {
                kind: "embedded".into(),
                digest: "sha256:acceptance".into(),
                verified: true,
                target_ref: "ghcr.io/kyth-os/kyth:testing".into(),
            },
            checks: vec![json!({"name": "power", "status": "passed"})],
            partition_steps: vec![json!({"kind": "format_filesystem", "status": "completed"})],
            message: String::new(),
            recovery_required: false,
        };
        installer_transaction::write_failure_summary(
            failure_path.to_str().unwrap(),
            &state,
            "boot configuration was not reached",
        )
        .expect("failure summary is durable");
        let decoded = installer_transaction::decode(
            &fs::read_to_string(&failure_path).expect("failure summary file"),
        )
        .expect("failure summary remains readable");
        assert_eq!(decoded.state.status, "failed");
        assert!(decoded.state.recovery_required);
        assert!(!decoded.guidance.bootable);
        assert_eq!(decoded.state.partition_steps.len(), 1);
    }

    #[test]
    fn bootc_and_secure_boot_staging_are_explicit_in_acceptance() {
        let bootc = installer_bootc::build_plan(BootcInstallInput {
            subcommand: "to-disk".into(),
            source_imgref: "docker://ghcr.io/kyth-os/kyth:testing".into(),
            target_imgref: "ghcr.io/kyth-os/kyth:testing".into(),
            target: "/dev/sda".into(),
            skip_fetch_check: true,
            skip_finalize: false,
            root_subvolume: false,
            wipe: true,
        })
        .expect("bootc wipe plan");
        assert!(bootc.destructive);
        assert!(bootc.argv.iter().any(|argument| argument == "--wipe"));
        assert_eq!(bootc.executor, "kyth-installerd");

        let secure_boot = installer_secure_boot::build_plan(SecureBootInput {
            kernel: "cachy".into(),
            force_stage: false,
            certificate_present: true,
            mokutil_present: true,
            secure_boot: "enabled".into(),
            enrolled: "no".into(),
            pending: "no".into(),
        })
        .expect("secure boot plan");
        assert_eq!(secure_boot.action, "import-certificate");
        assert!(secure_boot.requires_password);
        assert!(secure_boot.requires_reboot_confirmation);
    }

    #[test]
    fn recovery_guidance_fixture_is_fail_closed_for_every_state() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../testdata/recovery_cases.json"))
                .expect("recovery fixture must be valid JSON");
        for case in cases {
            let status = case["status"].as_str().expect("status");
            let guidance = installer_recovery::rescue_guidance(Some(status));
            assert_eq!(guidance.bootable, case["bootable"], "{status}");
            if status != "complete" && status != "secure_boot_staged" {
                assert!(!guidance.bootable, "{status}");
            }
        }
        assert!(!installer_recovery::rescue_guidance(Some("future")).bootable);
    }
}
