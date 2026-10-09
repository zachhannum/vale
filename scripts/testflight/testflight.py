#!/usr/bin/env python3
"""Reports a TestFlight upload on the PR and sets the "What to Test" text.

CI runs this script after `xcodebuild` uploads a build.

Usage:
  testflight.py comment --state STATE --version V --build B --sha SHA --pr N
  testflight.py notes --bundle-id ID --version V --build B --sha SHA [--pr N]

`comment` reads GITHUB_REPOSITORY, and `gh` reads GH_TOKEN. `notes` reads
ASC_KEY_PATH, ASC_KEY_ID, and ASC_ISSUER_ID, which describe an App Store
Connect API key. For a PR, it reads PR_TITLE and PR_BODY.
"""

import argparse
import base64
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

MARKER = "<!-- vale-testflight -->"
API = "https://api.appstoreconnect.apple.com"
LOCALE = "en-US"
# The limit of the "What to Test" text.
NOTES_LIMIT = 4000
STATES = {
    "uploading": "CI uploads it to TestFlight now.",
    "processing": "The upload is complete. Apple processes the build now.",
    "ready": "It is ready in the TestFlight app on the iPad.",
    "slow": "The upload is complete, but Apple did not finish in time. "
    "The build will appear in the TestFlight app, with no \"What to Test\" text.",
    "failed": "The upload failed.",
}


def ipad_test_section(body):
    """Returns the iPad test steps of a PR description, or an empty string.

    The steps are below a heading that starts with "iPad test", or in a
    paragraph that starts with "iPad test:".
    """
    lines = re.sub(r"<!--.*?-->", "", body or "", flags=re.S).splitlines()
    for i, line in enumerate(lines):
        heading = re.match(r"^(#{1,6})\s*iPad test", line, re.I)
        if heading:
            out = []
            for rest in lines[i + 1 :]:
                other = re.match(r"^(#{1,6})\s", rest)
                if other and len(other.group(1)) <= len(heading.group(1)):
                    break
                out.append(rest)
            return "\n".join(out).strip()
        paragraph = re.match(r"^iPad test:\s*(.*)", line, re.I)
        if paragraph:
            out = [paragraph.group(1)]
            for rest in lines[i + 1 :]:
                if not rest.strip():
                    break
                out.append(rest)
            return "\n".join(out).strip()
    return ""


def what_to_test(pr, title, body, sha):
    """Makes the "What to Test" text of a build."""
    if not pr:
        return f"main at commit {sha[:7]}"
    steps = ipad_test_section(body) or "The PR description has no iPad test section."
    return f"PR #{int(pr)}: {title}\n\n{steps}"[:NOTES_LIMIT]


def comment_body(state, version, build, sha, run_url=""):
    lines = [MARKER, f"TestFlight build {build} of commit {sha}, version {version}: {STATES[state]}"]
    if state == "failed" and run_url:
        lines += ["", f"The log is in the [workflow run]({run_url})."]
    return "\n".join(lines) + "\n"


def der_to_raw(der):
    """Changes an ECDSA P-256 signature from DER to the 64 bytes that a JWT holds."""
    if der[0] != 0x30 or der[2] != 0x02:
        raise ValueError("not a DER signature")
    r_len = der[3]
    r = der[4 : 4 + r_len]
    if der[4 + r_len] != 0x02:
        raise ValueError("not a DER signature")
    s_len = der[5 + r_len]
    s = der[6 + r_len : 6 + r_len + s_len]
    return b"".join(int.from_bytes(n, "big").to_bytes(32, "big") for n in (r, s))


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def token(key_path, key_id, issuer_id, now=None):
    """Makes a token for the App Store Connect API. `openssl` signs it."""
    now = int(now if now is not None else time.time())
    header = {"alg": "ES256", "kid": key_id, "typ": "JWT"}
    claims = {"iss": issuer_id, "iat": now, "exp": now + 600, "aud": "appstoreconnect-v1"}
    signed = ".".join(b64url(json.dumps(part, separators=(",", ":")).encode()) for part in (header, claims))
    der = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", key_path], input=signed.encode(), capture_output=True, check=True
    ).stdout
    return f"{signed}.{b64url(der_to_raw(der))}"


class Connect:
    """Sends requests to the App Store Connect API."""

    def __init__(self):
        self.key = (os.environ["ASC_KEY_PATH"], os.environ["ASC_KEY_ID"], os.environ["ASC_ISSUER_ID"])

    def request(self, method, path, query=None, body=None):
        url = API + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("Authorization", "Bearer " + token(*self.key))
        if data:
            request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                text = response.read()
        except urllib.error.HTTPError as error:
            sys.exit(f"{method} {path} failed with status {error.code}:\n{error.read().decode(errors='replace')}")
        return json.loads(text) if text else None


def find_build(connect, app, version, build):
    found = connect.request(
        "GET",
        "/v1/builds",
        {"filter[app]": app, "filter[version]": build, "filter[preReleaseVersion.version]": version, "limit": 1},
    )["data"]
    return found[0] if found else None


def set_notes(connect, build_id, text):
    for item in connect.request("GET", f"/v1/builds/{build_id}/betaBuildLocalizations")["data"]:
        if item["attributes"]["locale"] == LOCALE:
            body = {"data": {"type": "betaBuildLocalizations", "id": item["id"], "attributes": {"whatsNew": text}}}
            connect.request("PATCH", f"/v1/betaBuildLocalizations/{item['id']}", body=body)
            return
    body = {
        "data": {
            "type": "betaBuildLocalizations",
            "attributes": {"locale": LOCALE, "whatsNew": text},
            "relationships": {"build": {"data": {"type": "builds", "id": build_id}}},
        }
    }
    connect.request("POST", "/v1/betaBuildLocalizations", body=body)


def gh(*args, stdin=None):
    result = subprocess.run(["gh", *args], input=stdin, text=True, capture_output=True)
    if result.returncode != 0:
        sys.exit(f"gh {' '.join(args)} failed:\n{result.stderr}")
    return result.stdout


def comment(args):
    repo = os.environ["GITHUB_REPOSITORY"]
    number = int(args.pr)
    body = json.dumps({"body": comment_body(args.state, args.version, args.build, args.sha, args.run_url)})
    old = gh(
        "api",
        "--paginate",
        f"repos/{repo}/issues/{number}/comments?per_page=100",
        "--jq",
        f'.[] | select(.body | startswith("{MARKER}")) | .id',
    ).split()
    if old:
        gh("api", "--method", "PATCH", f"repos/{repo}/issues/comments/{old[0]}", "--input", "-", stdin=body)
    else:
        gh("api", "--method", "POST", f"repos/{repo}/issues/{number}/comments", "--input", "-", stdin=body)


def notes(args):
    """Waits for Apple to process the build, then sets the text.

    Writes `state=ready` or `state=slow` to the file that GITHUB_OUTPUT names.
    """
    connect = Connect()
    apps = connect.request("GET", "/v1/apps", {"filter[bundleId]": args.bundle_id, "limit": 1})["data"]
    if not apps:
        sys.exit(f"App Store Connect has no app with the bundle ID {args.bundle_id}.")
    text = what_to_test(args.pr, os.environ.get("PR_TITLE", ""), os.environ.get("PR_BODY", ""), args.sha)

    state = "slow"
    deadline = time.monotonic() + args.timeout
    while True:
        build = find_build(connect, apps[0]["id"], args.version, args.build)
        processing = build["attributes"]["processingState"] if build else "not listed"
        print(f"Build {args.build} of version {args.version}: {processing}", flush=True)
        if processing == "VALID":
            set_notes(connect, build["id"], text)
            state = "ready"
            break
        if processing in ("FAILED", "INVALID"):
            sys.exit(f"Apple rejected the build. The state is {processing}.")
        if time.monotonic() >= deadline:
            print("::warning::Apple did not finish in time. The build has no \"What to Test\" text.")
            break
        time.sleep(args.interval)

    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a") as file:
            file.write(f"state={state}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)

    p = commands.add_parser("comment")
    p.add_argument("--state", required=True, choices=sorted(STATES))
    p.add_argument("--pr", required=True)
    p.add_argument("--run-url", default="")
    p.set_defaults(run=comment)

    q = commands.add_parser("notes")
    q.add_argument("--bundle-id", required=True)
    q.add_argument("--pr", default="")
    q.add_argument("--timeout", type=int, default=1500)
    q.add_argument("--interval", type=int, default=30)
    q.set_defaults(run=notes)

    for command in (p, q):
        command.add_argument("--version", required=True)
        command.add_argument("--build", required=True)
        command.add_argument("--sha", required=True)

    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
