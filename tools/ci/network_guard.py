"""Linux verify network guard: isolated libtest invocations and an inherited violation channel."""
import os
import tempfile
from pathlib import Path


def invocations(binaries, env, test_args, list_tests):
    """List selections without preloading; remove ignored tests before isolating each invocation."""
    for binary in binaries:
        if not binary.get("libtest", True):
            yield binary, test_args
            continue
        local_env = {**os.environ, **env, **binary["env"]}
        names = list_tests(binary, local_env, test_args)
        ignored = list_tests(binary, local_env, [*test_args, "--ignored"])
        if names is None or ignored is None:
            raise ValueError(f"{binary['label']}: cannot list tests for network guard")
        for name in names:
            if name not in ignored:
                yield {**binary, "label": f"{binary['label']}: {name}", "isolated_test": name}, [
                    *test_args, "--exact", name]


def run(invocation, env, timed, shim, run_one):
    binary, args = invocation
    with tempfile.TemporaryDirectory(prefix="verify-network-") as scratch:
        log = Path(scratch, "violations")
        log.touch()
        preload = os.pathsep.join(filter(None, [str(shim), os.environ.get("LD_PRELOAD")]))
        result = run_one(binary, {**env, "LD_PRELOAD": preload, "HORIZON_NETWORK_GUARD_LOG": str(log)}, args, timed)
        records = log.read_text().splitlines()
        operations = sorted(set(records) - {"loaded"})
        if "loaded" not in records or operations:
            detail = ", ".join(operations) if operations else "guard library did not load"
            result["status"] = f"network guard: {detail}"
            result["output"].write(f"\n::error::network guard: {binary['label']}: {detail}; "
                                   'mark it #[ignore = "daily"] and add its daily row, reason network '
                                   '(tools/ci/move_to_daily.py)\n')
        return result
