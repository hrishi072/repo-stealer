"""Run the real disk-service test as root, only against its exclusive temp image.

No policy rules, disks, user containers or production authentication are changed.
"""
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
result = subprocess.run(["cargo", "test", "-p", "repo-stealer-app", "--release", "--locked", "--no-run", "--message-format=json"], cwd=root, check=True, capture_output=True, text=True)
executables = [row["executable"] for line in result.stdout.splitlines() if (row := json.loads(line)).get("reason") == "compiler-artifact" and row.get("executable") and row.get("profile", {}).get("test")]
assert len(executables) == 1, "Expected the app service test executable"
subprocess.run(["sudo", "-n", executables[0], "--ignored", "--exact", "protected_storage::tests::disk_service_roundtrip", "--nocapture", "--test-threads=1"], check=True, timeout=240)
