"""Read public release evidence; never publish, install, or execute downloads."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
RAW = ROOT / "build/compliance/audit-2026-09-14/public"
RAW.mkdir(parents=True, exist_ok=True)
HEADERS = {"User-Agent": "Mozilla/5.0", "Accept-Encoding": "identity"}


def fetch(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers=HEADERS), timeout=45)


def small(url, filename):
    with fetch(url) as response:
        contents = response.read()
    (RAW / filename).write_bytes(contents)
    return contents


def download_hash(url):
    result = {"url": url, "checkedAt": datetime.now(timezone.utc).isoformat()}
    try:
        digest, size = hashlib.sha256(), 0
        with fetch(url) as response:
            result.update(status=response.status, contentLength=response.headers.get("Content-Length"),
                          etag=response.headers.get("ETag"), lastModified=response.headers.get("Last-Modified"))
            while data := response.read(1024 * 1024):
                digest.update(data)
                size += len(data)
        result.update(bytes=size, sha256=digest.hexdigest())
        result["matchesHistoricalRecord"] = next((x["sha256"] == result["sha256"]
            for x in history if x["file"] == url.rsplit("/", 1)[-1]), None)
    except Exception as error:
        result["error"] = str(error)
    print(json.dumps(result), flush=True)
    return result


history = json.loads((ROOT / "compliance/evidence/website-installers.json").read_text(encoding="utf-8"))
homepage = small("https://yougori.com/", "homepage.html").decode("utf-8")
script_paths = re.findall(r'<script\b[^>]*\bsrc="([^\"]+)"', homepage)
site_scripts = []
for number, relative in enumerate(script_paths):
    url = urllib.parse.urljoin("https://yougori.com/", relative)
    if urllib.parse.urlparse(url).netloc != "yougori.com":
        continue
    content = small(url, f"site-{number}.js").decode("utf-8")
    site_scripts.append((url, content))
download_paths = sorted(set(re.findall(r'/downloads/[A-Za-z0-9_.-]+\.(?:exe|msi|deb|dmg)',
                                     "\n".join(x[1] for x in site_scripts))))
releases = json.loads(small("https://api.github.com/repos/Yougori/yougori/releases?per_page=100", "releases.json"))
release_checks = []
local_index = (ROOT / "build/compliance/bundle/SOURCE_INDEX.json").read_bytes()
local_archives = {x["file"]: x["sha256"] for x in json.loads(local_index)["archives"]}
for number, release in enumerate(releases):
    record = {key: release[key] for key in ["tag_name", "html_url", "draft", "prerelease", "published_at", "body"]}
    record["assets"] = [{key: asset.get(key) for key in ["name", "size", "digest", "browser_download_url"]}
                        for asset in release["assets"]]
    for asset in release["assets"]:
        if asset["name"] != "SOURCE_INDEX.json":
            continue
        data = small(asset["browser_download_url"], f"source-index-{number}.json")
        source = json.loads(data)
        public_archives = {x["file"]: x["sha256"] for x in source["archives"]}
        record.update(indexSha256=hashlib.sha256(data).hexdigest(),
                      indexMatchesCurrent=data == local_index,
                      changedArchives=[name for name in local_archives if public_archives.get(name) != local_archives[name]],
                      obsoleteArchives=[name for name in public_archives if name not in local_archives])
    release_checks.append(record)
with ThreadPoolExecutor(max_workers=3) as pool:
    downloads = list(pool.map(download_hash, ["https://yougori.com" + path for path in download_paths]))
readme = (ROOT / "README.md").read_text(encoding="utf-8")
readme_checks = []
for url in re.findall(r'https://yougori\.com/downloads/[^)\s]+', readme):
    try:
        with fetch(url) as response:
            readme_checks.append({"url": url, "status": response.status})
    except urllib.error.HTTPError as error:
        readme_checks.append({"url": url, "status": error.code})
result = {"checkedAt": datetime.now(timezone.utc).isoformat(),
          "method": "Anonymous HTTPS GET; full installer bodies streamed and hashed without execution",
          "homepageScripts": [x[0] for x in site_scripts], "downloads": downloads,
          "localSourceIndexSha256": hashlib.sha256(local_index).hexdigest(),
          "publicReleases": release_checks, "readmeLinks": readme_checks}
(OUT / "public-distribution.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"releases": len(release_checks), "downloadCount": len(downloads),
                  "currentSourceIndexPublished": any(x.get("indexMatchesCurrent") for x in release_checks)}, indent=2))
