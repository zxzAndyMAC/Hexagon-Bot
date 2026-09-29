#!/usr/bin/env python3
"""Install/check the pinned local retrieval weights; never uploads repository data."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import urllib.request


def main():
    manifest = json.loads((Path(__file__).resolve().parents[1] /
        "crates/hexagon-core/src/semsearch/model.json").read_text())
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=Path.home() /
                        ".hexagon/models" / manifest["directory"])
    parser.add_argument("--check", action="store_true", help="verify local files without downloading")
    args = parser.parse_args()
    print("Model license:", manifest["license"], flush=True)
    if not args.check:
        args.directory.mkdir(parents=True, exist_ok=True)
    for asset in manifest["assets"]:
        target = args.directory / asset["name"]
        if target.is_file() and target.stat().st_size == asset["size"]:
            with target.open("rb") as stream:
                digest = hashlib.sha256()
                while chunk := stream.read(1024 * 1024):
                    digest.update(chunk)
                if digest.hexdigest() == asset["sha256"]:
                    print("Verified", asset["name"], flush=True)
                    continue
        if args.check:
            raise SystemExit(f"Missing or invalid model asset: {asset['name']}")
        url = f"https://huggingface.co/{manifest['repo']}/resolve/{manifest['revision']}/{asset['remote']}"
        digest = hashlib.sha256()
        count = 0
        temporary = None
        try:
            print("Downloading", asset["name"], flush=True)
            with tempfile.NamedTemporaryFile(dir=args.directory, delete=False) as output:
                temporary = Path(output.name)
                with urllib.request.urlopen(url, timeout=60) as response:
                    while chunk := response.read(1024 * 1024):
                        count += len(chunk)
                        if count > asset["size"]:
                            raise RuntimeError("model asset exceeds pinned size")
                        digest.update(chunk)
                        output.write(chunk)
            if count != asset["size"] or digest.hexdigest() != asset["sha256"]:
                raise RuntimeError("model asset failed integrity check")
            os.replace(temporary, target)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    print("Local retrieval model ready:", args.directory, flush=True)


if __name__ == "__main__":
    main()
