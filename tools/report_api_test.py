#!/usr/bin/env python3
import argparse
import json
import random
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from datetime import datetime, timezone
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


LOG_TYPES = ["error_999", "error_opt_999", "error_pay"]
PACKAGES = [
    "com.demo.alpha",
    "com.demo.beta",
    "com.demo.gamma",
    "com.slots789.ow",
    "com.game.center",
]
CLI_TYPES = ["launcher_v2", "launcher_v3", "desktop_new"]
REGIONS = ["CN", "HK", "SG", "US", "TH", "VN"]


@dataclass
class ApiResult:
    endpoint: str
    ok: bool
    status: int
    elapsed_ms: float
    body: str
    error: str = ""


def utc_text():
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")


def post_json(base_url, endpoint, payload, timeout):
    url = f"{base_url.rstrip('/')}{endpoint}"
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = Request(
        url,
        data=body,
        method="POST",
        headers={
            "Content-Type": "application/json",
            "User-Agent": "tiny-http-api-test/1.0",
        },
    )
    start = time.perf_counter()
    try:
        with urlopen(request, timeout=timeout) as response:
            text = response.read().decode("utf-8", errors="replace")
            elapsed_ms = (time.perf_counter() - start) * 1000
            return ApiResult(endpoint, 200 <= response.status < 300, response.status, elapsed_ms, text)
    except HTTPError as err:
        text = err.read().decode("utf-8", errors="replace")
        elapsed_ms = (time.perf_counter() - start) * 1000
        return ApiResult(endpoint, False, err.code, elapsed_ms, text, str(err))
    except URLError as err:
        elapsed_ms = (time.perf_counter() - start) * 1000
        return ApiResult(endpoint, False, 0, elapsed_ms, "", str(err.reason))
    except Exception as err:
        elapsed_ms = (time.perf_counter() - start) * 1000
        return ApiResult(endpoint, False, 0, elapsed_ms, "", str(err))


def error_payload(rng, index, duplicate_ratio):
    log_type = rng.choice(LOG_TYPES)
    package = rng.choice(PACKAGES)
    user = str(100000 + rng.randint(0, 899999))
    issue_count = max(1, int(100 * (1.0 - duplicate_ratio)))
    issue_no = rng.randint(1, issue_count)
    if rng.random() < duplicate_ratio:
        issue_no = rng.randint(1, 8)
    message = (
        f"RuntimeError: api test issue {issue_no} at worker_{issue_no % 5}.rs:42 "
        f"address 0x{rng.randint(1000, 999999):x}"
    )
    version = json.dumps(
        {
            "branch": rng.choice(["main", "release", "hotfix"]),
            "patch_time": utc_text()[:10],
            "game_id": rng.randint(-1, 5),
            "build": rng.randint(1000, 9999),
        },
        ensure_ascii=False,
    )
    logs = "\n".join(
        [
            f"[{utc_text()}] api test source #{index}",
            f"user={user}",
            f"package={package}",
            f"message={message}",
        ]
    )
    return {
        "log_type": log_type,
        "message": message,
        "user": user,
        "package": package,
        "nav_url": f"https://example.test/{package}/page/{rng.randint(1, 30)}",
        "version": version,
        "logs": logs,
    }


def device_payload(rng, index):
    cli_type = rng.choice(CLI_TYPES)
    package = rng.choice(PACKAGES)
    user = str(100000 + rng.randint(0, 899999))
    configuration_info = {
        "os": rng.choice(["windows", "macos", "linux"]),
        "first_report": True,
        "cpu": rng.choice(["x64", "arm64"]),
        "memory_gb": rng.choice([8, 16, 32, 64]),
        "sample": index,
    }
    return {
        "cli_type": cli_type,
        "user": user,
        "package": package,
        "configuration_info": json.dumps(configuration_info, ensure_ascii=False),
        "region": rng.choice(REGIONS),
    }


def build_jobs(args):
    jobs = []
    for index in range(args.error_logs):
        rng = random.Random(args.seed + index)
        jobs.append(("/api/upload_log", error_payload(rng, index, args.duplicate_ratio)))
    for index in range(args.device_reports):
        rng = random.Random(args.seed + 100000 + index)
        jobs.append(("/api/upload_statistics_cli_cfg", device_payload(rng, index)))
    random.Random(args.seed).shuffle(jobs)
    return jobs


def run_jobs(args):
    jobs = build_jobs(args)
    results = []
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = [
            pool.submit(post_json, args.base_url, endpoint, payload, args.timeout)
            for endpoint, payload in jobs
        ]
        for completed, future in enumerate(as_completed(futures), start=1):
            result = future.result()
            results.append(result)
            if args.verbose or not result.ok:
                flag = "OK" if result.ok else "FAIL"
                print(
                    f"[{completed}/{len(jobs)}] {flag} {result.endpoint} "
                    f"status={result.status} elapsed={result.elapsed_ms:.1f}ms"
                )
                if not result.ok:
                    print(f"  error={result.error}")
                    print(f"  body={result.body[:500]}")
    return results


def print_summary(results):
    total = len(results)
    failed = [item for item in results if not item.ok]
    by_endpoint = {}
    for item in results:
        bucket = by_endpoint.setdefault(item.endpoint, [])
        bucket.append(item)

    print("")
    print("summary")
    print(f"  total: {total}")
    print(f"  success: {total - len(failed)}")
    print(f"  failed: {len(failed)}")
    for endpoint, items in sorted(by_endpoint.items()):
        ok_count = sum(1 for item in items if item.ok)
        avg_ms = sum(item.elapsed_ms for item in items) / len(items)
        max_ms = max(item.elapsed_ms for item in items)
        print(
            f"  {endpoint}: {ok_count}/{len(items)} ok, "
            f"avg={avg_ms:.1f}ms, max={max_ms:.1f}ms"
        )

    if failed:
        print("")
        print("first failures")
        for item in failed[:5]:
            print(f"  {item.endpoint} status={item.status} error={item.error} body={item.body[:200]}")

    return len(failed) == 0


def main():
    parser = argparse.ArgumentParser(
        description="Report test error logs and device information through tiny-http HTTP APIs."
    )
    parser.add_argument("--base-url", default="http://127.0.0.1:8000", help="Server base URL")
    parser.add_argument("--error-logs", type=int, default=100, help="Number of /api/upload_log calls")
    parser.add_argument(
        "--device-reports",
        type=int,
        default=100,
        help="Number of /api/upload_statistics_cli_cfg calls",
    )
    parser.add_argument("--concurrency", type=int, default=8, help="Concurrent HTTP workers")
    parser.add_argument("--timeout", type=float, default=8.0, help="Per-request timeout seconds")
    parser.add_argument("--seed", type=int, default=20260601, help="Random seed")
    parser.add_argument(
        "--duplicate-ratio",
        type=float,
        default=0.75,
        help="Higher value creates more repeated error messages to test aggregation",
    )
    parser.add_argument("--verbose", action="store_true", help="Print every request result")
    args = parser.parse_args()

    args.error_logs = max(0, args.error_logs)
    args.device_reports = max(0, args.device_reports)
    args.concurrency = max(1, args.concurrency)
    args.duplicate_ratio = min(1.0, max(0.0, args.duplicate_ratio))

    print(f"target: {args.base_url.rstrip('/')}")
    print(f"upload_log: {args.error_logs}")
    print(f"upload_statistics_cli_cfg: {args.device_reports}")
    print(f"concurrency: {args.concurrency}")

    results = run_jobs(args)
    success = print_summary(results)
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
