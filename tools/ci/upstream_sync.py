#!/usr/bin/env python3
"""Stage an upstream merge, verify its exact SHA, then fast-forward protected main."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

TITLE = "Upstream sync failed"
STAGING = "refs/heads/sync/upstream"
# Upstream still tracks these; the fork builds them into OUT_DIR instead.
GENERATED_API = (
    "crates/horizon-client-render/src/gfx/native_render/generated.rs",
    "crates/horizon-client/src/mod_api/generated.rs",
    "crates/horizon-client/src/mod_api/generated_host.rs",
    "crates/horizon-client/src/game/mod_api_host_generated.rs",
)


def command(*args):
    return subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE).stdout.strip()


def api(path):
    return json.loads(command("gh", "api", path))


def verify(repo, head, request):
    label = f"{head[:7]} ({request})"
    title = f"Upstream sync {label}"
    command("gh", "workflow", "run", "ci.yml", "--repo", repo, "--ref", "sync/upstream",
            "--field", f"upstream_sync={label}")
    # The dispatch endpoint returns no run ID. A unique run-name distinguishes this
    # request from manual dispatches and reruns, even when they share the same SHA.
    deadline = time.monotonic() + 300
    while True:
        runs = api(f"repos/{repo}/actions/workflows/ci.yml/runs?event=workflow_dispatch&branch=sync%2Fupstream&per_page=100")
        run = next((run for run in runs["workflow_runs"] if run["display_title"] == title), None)
        if run is not None:
            break
        if time.monotonic() >= deadline:
            raise RuntimeError("Dispatched checks did not appear within five minutes.")
        time.sleep(15)
    url = run["html_url"]
    if run["head_sha"] != head:
        raise RuntimeError(f"Checks targeted {run['head_sha']}, not candidate {head}: {url}")
    print(f"Checking {head}: {url}", flush=True)
    deadline = time.monotonic() + 105 * 60
    while run["status"] != "completed":
        if time.monotonic() >= deadline:
            command("gh", "run", "cancel", str(run["id"]), "--repo", repo)
            raise RuntimeError(f"Checks exceeded 105 minutes (cancellation requested): {url}")
        time.sleep(30)
        run = api(f"repos/{repo}/actions/runs/{run['id']}")
    if run["conclusion"] != "success":
        raise RuntimeError(f"Checks concluded {run['conclusion']}: {url}")
    jobs = api(f"repos/{repo}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
    # Protection requires the check named verify on this SHA, not a skipped job
    # or merely a green workflow. The jobs API exposes its check-run URL.
    check = next((job for job in jobs if job["name"] == "verify"), None)
    if check is None or check["conclusion"] != "success" or check["head_sha"] != head:
        raise RuntimeError(f"No successful verify check on candidate {head}: {url}")
    print(f"verify passed: {check['check_run_url']}", flush=True)
    return url


def sync():
    repo = os.environ["GITHUB_REPOSITORY"]
    request = f"{os.environ['GITHUB_RUN_ID']}-{os.environ['GITHUB_RUN_ATTEMPT']}"
    run_url = f"{os.environ['GITHUB_SERVER_URL']}/{repo}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
    # Do not create another issue if listing existing issues itself fails.
    issues = json.loads(command("gh", "issue", "list", "--repo", repo, "--state", "open",
                                "--label", "sync", "--limit", "100", "--json", "number,title"))
    issue = next((issue["number"] for issue in issues if issue["title"] == TITLE), None)
    upstream = "unavailable (fetch did not complete)"
    promoted = False
    try:
        base = command("git", "rev-parse", "HEAD")
        command("git", "fetch", "--no-tags", "https://github.com/horizon-rust/horizon.git",
                "+refs/heads/main:refs/remotes/upstream/main")
        upstream = command("git", "rev-parse", "upstream/main")
        result = subprocess.run(["bash", str(Path(__file__).with_name("merge_upstream.sh"))], check=False)
        if result.returncode == 2:
            conflicts = set(command("git", "diff", "--name-only", "--diff-filter=U").splitlines())
            for path in GENERATED_API:
                if path in conflicts:
                    stages = {entry.split()[2] for entry in
                              command("git", "ls-files", "--unmerged", "--", path).splitlines()}
                    # Stage 2 absent: deleted by the fork, modified by upstream (stage 3).
                    if stages == {"1", "3"}:
                        command("git", "rm", "--", path)
            files = command("git", "diff", "--name-only", "--diff-filter=U")
            if files:
                command("git", "merge", "--abort")
                raise RuntimeError(f"Merge conflicts (nothing pushed):\n\n```text\n{files}\n```")
            # Match merge_upstream.sh's clean-merge identity and message.
            command("git", "-c", "commit.gpgsign=false", "-c", "user.name=arismoko", "-c",
                    "user.email=19542195+arismoko@users.noreply.github.com", "commit", "--no-edit", "--cleanup=strip")
        elif result.returncode not in (0, 3):
            raise RuntimeError(f"Merge failed with exit code {result.returncode}; nothing pushed.")
        if result.returncode == 3 and issue is None:
            return 0
        head = command("git", "rev-parse", "HEAD")
        workflows = command("git", "diff", "--name-only", base, head, "--", ".github/workflows")
        if workflows:
            if issue is not None:
                thread = json.loads(command("gh", "issue", "view", str(issue), "--repo", repo,
                                            "--json", "body,comments"))
                reports = [thread["body"], *(comment["body"] for comment in thread["comments"])]
                if any(upstream in report for report in reports):
                    print(f"Workflow-file handoff for {upstream} already reported in issue {issue}; nothing pushed.",
                          file=sys.stderr)
                    return 1
            raise RuntimeError("Upstream changed workflow files; an agent must push this one. "
                               "upstream-sync prepares the merge branch, and root pushes main from hermeslab. "
                               "GITHUB_TOKEN cannot update .github/workflows/ without workflows permission."
                               f"\n\n```text\n{workflows}\n```")
        previous = command("git", "ls-remote", "--heads", "origin", STAGING).split()
        lease = previous[0] if previous else ""
        # Only the automation-owned side branch can be replaced. Main is never forced;
        # if someone advances it while checks run, the final push fails safely.
        command("git", "push", f"--force-with-lease={STAGING}:{lease}", "origin", f"{head}:{STAGING}")
        proof = verify(repo, head, request)
        command("git", "push", "origin", f"{head}:refs/heads/main")
        promoted = True
        if issue is not None:
            command("gh", "issue", "close", str(issue), "--repo", repo, "--comment",
                    f"🤖 Agent: Upstream `{upstream}` synced after [checks passed]({proof}). [Sync run]({run_url}).")
        return 0
    except (subprocess.CalledProcessError, RuntimeError) as failure:
        state = "Main advanced, but closing the sync issue failed." if promoted else "Main was not advanced."
        body = (f"🤖 Agent: Could not complete sync of upstream `{upstream}`. {state}"
                f"\n\n{failure}\n\n[Sync run]({run_url})")
        if issue is None:
            command("gh", "label", "create", "sync", "--repo", repo, "--color", "B60205",
                    "--description", "Upstream sync needs attention", "--force")
            command("gh", "issue", "create", "--repo", repo, "--title", TITLE, "--label", "sync",
                    "--body", "> 🤖 Filed by Aria's agent swarm, Dyna, not by Aria.\n\n" + body)
        else:
            command("gh", "issue", "comment", str(issue), "--repo", repo, "--body", body)
        print(body, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(sync())
