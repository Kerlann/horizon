#!/usr/bin/env python3
"""Choose a hosted runner using repository-local Actions history, never boat secrets."""
from datetime import datetime, timezone
import json
import os
import subprocess


def user_step_started(job):
    return any(step.get("started_at") for step in job.get("steps", [])
               if step.get("name") not in ("Set up job", "Complete job"))


def genuine_failure(jobs):
    return any(job.get("conclusion") in ("failure", "timed_out") and user_step_started(job) for job in jobs)


def recovery_reason(jobs, now):
    """An outage must not hide any genuine failure in the original run."""
    if genuine_failure(jobs):
        return ""
    return health_reason([job for job in jobs if job["name"] in ("lint", "verify")], now)


def previous_attempt_reason(jobs, now):
    """Retain an outage across cancellation; rerun APIs cannot override labels."""
    reason = recovery_reason(jobs, now)
    if reason:
        return reason
    if genuine_failure(jobs):
        return ""
    for job in jobs:
        if (job["name"] in ("lint", "verify") and "boat-large" in job.get("labels", [])
                and job.get("conclusion") == "cancelled" and not user_step_started(job)):
            created = datetime.fromisoformat(job["created_at"].replace("Z", "+00:00"))
            completed = datetime.fromisoformat(job["completed_at"].replace("Z", "+00:00"))
            if (completed - created).total_seconds() > 600:
                return f"previous attempt boat job {job['id']} cancelled after waiting more than 10 minutes"
    return ""


def health_reason(jobs, now):
    """Return a runner outage reason, not an ordinary build/test failure."""
    for job in jobs:
        if "boat-large" not in job.get("labels", []):
            continue
        if job["status"] == "queued":
            created = datetime.fromisoformat(job["created_at"].replace("Z", "+00:00"))
            if (now - created).total_seconds() > 600:
                return f"boat job {job['id']} queued for more than 10 minutes"
        if job.get("conclusion") == "failure" and not user_step_started(job):
            return f"boat job {job['id']} failed before its first step"
    return ""


def select_label(enabled, users, author, actor, jobs, now):
    if enabled != "on" or author not in users or actor not in users:
        return "ubuntu-latest", "boat is disabled or the author/actor is not listed"
    reason = health_reason(jobs, now)
    return ("ubuntu-latest", reason) if reason else ("boat-large", "recent boat jobs show no runner outage")


def api(path):
    result = subprocess.run(["gh", "api", path], text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def main():
    repo = os.environ["GITHUB_REPOSITORY"]
    jobs = []
    reason = ""
    enabled = os.environ.get("BOAT_CI", "")
    users = os.environ.get("BOAT_USERS", "").split(",")
    author, actor = os.environ["AUTHOR"], os.environ["ACTOR"]
    if enabled == "on" and author in users and actor in users:
        try:
            # Bound the read cost: five recent runs, at most 100 jobs per run.
            for run in api(f"repos/{repo}/actions/runs?per_page=5")["workflow_runs"]:
                jobs.extend(api(f"repos/{repo}/actions/runs/{run['id']}/jobs?per_page=100")["jobs"])
        except (subprocess.CalledProcessError, json.JSONDecodeError, KeyError) as failure:
            reason = f"boat health history is unavailable ({type(failure).__name__})"
    label, decision = select_label(enabled, users, author, actor, jobs, datetime.now(timezone.utc))
    attempt = int(os.environ.get("GITHUB_RUN_ATTEMPT", "1"))
    prior = ""
    if attempt > 1:
        try:
            previous = api(f"repos/{repo}/actions/runs/{os.environ['GITHUB_RUN_ID']}/attempts/{attempt - 1}/jobs?per_page=100")["jobs"]
            prior = previous_attempt_reason(previous, datetime.now(timezone.utc))
        except (subprocess.CalledProcessError, json.JSONDecodeError, KeyError) as failure:
            prior = f"previous attempt history is unavailable ({type(failure).__name__})"
    forced = bool(reason or prior or (label == "ubuntu-latest" and health_reason(jobs, datetime.now(timezone.utc))))
    if reason or prior:
        label, decision = "ubuntu-latest", prior or reason
    print(f"hosted runner: {label}: {decision}")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"label={label}\nforced_github={str(forced).lower()}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
        summary.write(f"### Hosted runner selection\n\n{label}: {decision}.\n")


if __name__ == "__main__":
    main()
