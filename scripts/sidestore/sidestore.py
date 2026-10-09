#!/usr/bin/env python3
"""Publishes the iPad builds for SideStore.

CI runs this script. Each build is a pre-release on GitHub that holds one
`.ipa` file. The tag is `pr-<number>` for a PR and `main-build` for `main`.
The source file lists these releases in the AltStore format.

Usage:
  sidestore.py publish --ipa FILE --sha SHA --run-number N [--pr N]
  sidestore.py comment --sha SHA --run-number N --pr N
  sidestore.py remove --pr N
  sidestore.py source --out DIR

The script reads GITHUB_REPOSITORY, and `gh` reads GH_TOKEN.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
MARKER = "<!-- vale-sidestore -->"
BUNDLE_ID = "dev.vale.app"
MAIN_TAG = "main-build"
TAG = re.compile(r"^(?:main-build|pr-(\d+))$")
ASSET = re.compile(r"^Vale-(\d+\.\d+\.\d+)\.ipa$")


def version(pr, run_number):
    """The middle number is the PR, or 0 for `main`. The last number is the build."""
    return f"0.{int(pr) if pr else 0}.{int(run_number)}"


def tag_of(pr):
    return f"pr-{int(pr)}" if pr else MAIN_TAG


def default_pages_url(repo):
    owner, name = repo.split("/", 1)
    return f"https://{owner.lower()}.github.io/{name}/"


def install_url(pages, tag, asset):
    return f"{pages}?tag={tag}&file={asset}"


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


def build_source(repo, releases, pages):
    """Makes the source from the list of releases that the GitHub API returns."""
    versions = []
    for release in releases:
        if not TAG.match(release.get("tag_name", "")):
            continue
        for asset in release.get("assets", []):
            name = ASSET.match(asset["name"])
            if not name:
                continue
            versions.append(
                {
                    "version": name.group(1),
                    "date": asset["created_at"],
                    "localizedDescription": release.get("name") or release["tag_name"],
                    "downloadURL": asset["browser_download_url"],
                    "size": asset["size"],
                    "minOSVersion": "16.0",
                }
            )
    versions.sort(key=lambda v: v["date"], reverse=True)

    apps = []
    if versions:
        newest = versions[0]
        apps.append(
            {
                "name": "Vale",
                "bundleIdentifier": BUNDLE_ID,
                "developerName": repo.split("/", 1)[0],
                "subtitle": "Builds of main and of each open PR",
                "localizedDescription": "Vale is a GIS for fictional worlds. "
                "The version number is 0.<PR>.<build>. PR 0 is the main branch.",
                "iconURL": pages + "icon.png",
                "tintColor": "#2f6f73",
                "versions": versions,
                # Older versions of the format read these fields.
                "version": newest["version"],
                "versionDate": newest["date"],
                "versionDescription": newest["localizedDescription"],
                "downloadURL": newest["downloadURL"],
                "size": newest["size"],
            }
        )
    return {
        "name": "Vale builds",
        "identifier": "dev.vale.builds",
        "sourceURL": pages + "apps.json",
        "apps": apps,
        "news": [],
    }


def comment_body(repo, pr, sha, run_number, pages):
    number = pr["number"]
    tag = tag_of(number)
    ver = version(number, run_number)
    asset = f"Vale-{ver}.ipa"
    download = f"https://github.com/{repo}/releases/download/{tag}/{asset}"
    lines = [
        MARKER,
        f"The iPad build of commit {sha} is ready. The version is {ver}.",
        "",
        f"[Install in SideStore]({install_url(pages, tag, asset)}) or [download the .ipa file]({download}).",
    ]
    head_repo = (pr.get("head") or {}).get("repo") or {}
    if head_repo.get("full_name") != repo:
        lines += ["", "This PR comes from a fork. Read its code before you install the build."]
    steps = ipad_test_section(pr.get("body"))
    lines += ["", "iPad test:", ""]
    if steps:
        lines += [f"> {line}".rstrip() for line in steps.splitlines()]
    else:
        lines.append("The PR description has no iPad test section.")
    return "\n".join(lines) + "\n"


def gh(*args, stdin=None, check=True):
    result = subprocess.run(["gh", *args], input=stdin, text=True, capture_output=True)
    if check and result.returncode != 0:
        sys.exit(f"gh {' '.join(args)} failed:\n{result.stderr}")
    return result


def api(path, method="GET", body=None):
    args = ["api", "--method", method, path]
    if body is not None:
        args += ["--input", "-"]
    out = gh(*args, stdin=None if body is None else json.dumps(body)).stdout
    return json.loads(out) if out.strip() else None


def repository():
    return os.environ["GITHUB_REPOSITORY"]


def pages_url(repo):
    """Returns the address of the GitHub Pages site. An account can have its own domain."""
    site = gh("api", f"repos/{repo}/pages", "--jq", ".html_url", check=False)
    url = site.stdout.strip()
    if site.returncode != 0 or not url:
        return default_pages_url(repo)
    return re.sub(r"^http://", "https://", url).rstrip("/") + "/"


def find_comment(repo, number):
    out = gh(
        "api",
        "--paginate",
        f"repos/{repo}/issues/{number}/comments?per_page=100",
        "--jq",
        f'.[] | select(.body | startswith("{MARKER}")) | .id',
    ).stdout.split()
    return out[0] if out else None


def set_comment(repo, number, body, create):
    comment = find_comment(repo, number)
    if comment:
        api(f"repos/{repo}/issues/comments/{comment}", "PATCH", {"body": body})
    elif create:
        api(f"repos/{repo}/issues/{number}/comments", "POST", {"body": body})


def open_pr(repo, number):
    """Returns the PR, or exits when the PR is not open."""
    pr = api(f"repos/{repo}/pulls/{int(number)}")
    if pr["state"] != "open":
        print(f"PR {number} is not open. Nothing to do.")
        sys.exit(0)
    return pr


def publish(args):
    repo = repository()
    tag = tag_of(args.pr)
    ver = version(args.pr, args.run_number)
    short = args.sha[:7]
    if args.pr:
        pr = open_pr(repo, args.pr)
        title = f"PR #{pr['number']}: {pr['title']} ({short})"
        notes = f"The iPad build of PR #{pr['number']} at commit {args.sha}. Each push to the PR replaces the file."
    else:
        title = f"main ({short})"
        notes = f"The iPad build of `main` at commit {args.sha}. Each push to `main` replaces the file."

    with tempfile.TemporaryDirectory() as tmp:
        asset = Path(tmp) / f"Vale-{ver}.ipa"
        shutil.copyfile(args.ipa, asset)
        old = gh("release", "view", tag, "-R", repo, "--json", "assets", check=False)
        if old.returncode == 0:
            api(f"repos/{repo}/git/refs/tags/{tag}", "PATCH", {"sha": args.sha, "force": True})
            gh("release", "upload", tag, str(asset), "-R", repo, "--clobber")
            for other in json.loads(old.stdout)["assets"]:
                if other["name"] != asset.name:
                    gh("release", "delete-asset", tag, other["name"], "-R", repo, "--yes")
            gh("release", "edit", tag, "-R", repo, "--title", title, "--notes", notes, "--prerelease")
        else:
            gh(
                "release", "create", tag, str(asset), "-R", repo, "--target", args.sha,
                "--title", title, "--notes", notes, "--prerelease",
            )  # fmt: skip
    print(f"Published {asset.name} in the release {tag}.")


def comment(args):
    repo = repository()
    pr = open_pr(repo, args.pr)
    set_comment(repo, pr["number"], comment_body(repo, pr, args.sha, args.run_number, pages_url(repo)), create=True)


def remove(args):
    repo = repository()
    number = int(args.pr)
    gh("release", "delete", tag_of(number), "-R", repo, "--cleanup-tag", "--yes", check=False)
    body = f"{MARKER}\nThe iPad build of this PR is removed, because the PR is closed.\n"
    set_comment(repo, number, body, create=False)


def source(args):
    repo = repository()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    releases = api(f"repos/{repo}/releases?per_page=100")
    (out / "apps.json").write_text(json.dumps(build_source(repo, releases, pages_url(repo)), indent=2) + "\n")
    page = (HERE / "index.html").read_text().replace("OWNER/REPOSITORY", repo)
    (out / "index.html").write_text(page)
    shutil.copyfile(HERE / "icon.png", out / "icon.png")
    (out / ".nojekyll").touch()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)

    p = commands.add_parser("publish")
    p.add_argument("--ipa", required=True)
    p.add_argument("--sha", required=True)
    p.add_argument("--run-number", required=True)
    p.add_argument("--pr", default="")
    p.set_defaults(run=publish)

    p = commands.add_parser("comment")
    p.add_argument("--sha", required=True)
    p.add_argument("--run-number", required=True)
    p.add_argument("--pr", required=True)
    p.set_defaults(run=comment)

    p = commands.add_parser("remove")
    p.add_argument("--pr", required=True)
    p.set_defaults(run=remove)

    p = commands.add_parser("source")
    p.add_argument("--out", required=True)
    p.set_defaults(run=source)

    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
