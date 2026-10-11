#!/usr/bin/env python3
"""summon-wiki.py — mirror a MediaWiki (pages + images) for offline reference.

Caput Mortuum ports game behaviour from upstream *code*; the wikis explain
what the systems are *for* in play (guides, mechanics, numbers players rely
on). This keeps a local copy next to the upstream checkouts.

    tools/summon-wiki.py frontier            # https://frontierstation.wiki.gg
    tools/summon-wiki.py ss14                # https://wiki.spacestation14.com
    tools/summon-wiki.py frontier --no-images

Output (gitignored, under upstream/wiki/<site>/):

    siteinfo.json          namespaces, statistics, licence
    index.json             one entry per page: title, ns, file, revid, timestamp
    wikitext/<ns>/<t>.wiki raw source of every page in every namespace
    html/<t>.html          rendered article HTML (main namespace), templates expanded
    images/<file>          every uploaded file, at full resolution
    images.json            file metadata: url, size, sha1, mime, uploader

Re-running is incremental: pages whose revid is unchanged and images whose
size+sha1 match are skipped. Wiki content is CC BY-SA (see siteinfo.json);
keep attribution if any of it is redistributed.
"""

import argparse
import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

SITES = {
    "frontier": "https://frontierstation.wiki.gg/api.php",
    "ss14": "https://wiki.spacestation14.com/api.php",
}
UA = "CaputMortuumWikiMirror/0.1 (offline reference for a Rust port of SS14; github.com/7uk0)"
DELAY = 0.25  # seconds between requests: be a polite client


def safe(title):
    return urllib.parse.quote(title.replace(" ", "_"), safe="()_-,.'!&+=@")


class Api:
    def __init__(self, base):
        self.base = base
        self.last = 0.0

    def _open(self, url, tries=5):
        for i in range(tries):
            wait = self.last + DELAY - time.time()
            if wait > 0:
                time.sleep(wait)
            self.last = time.time()
            try:
                req = urllib.request.Request(url, headers={"User-Agent": UA})
                with urllib.request.urlopen(req, timeout=60) as r:
                    return r.read()
            except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
                code = getattr(e, "code", None)
                if code in (403, 404):
                    raise
                print(f"  retry {i + 1}/{tries} after {e}", file=sys.stderr)
                time.sleep(2 ** (i + 1))
        raise RuntimeError(f"giving up on {url}")

    def get(self, **params):
        params.setdefault("format", "json")
        params.setdefault("formatversion", "2")
        params.setdefault("maxlag", "5")
        data = json.loads(self._open(self.base + "?" + urllib.parse.urlencode(params)))
        if "error" in data:
            if data["error"].get("code") == "maxlag":
                time.sleep(5)
                return self.get(**params)
            raise RuntimeError(data["error"])
        return data

    def query_all(self, **params):
        """Follow `continue` until exhausted, yielding each response."""
        cont = {}
        while True:
            data = self.get(action="query", **params, **cont)
            yield data
            if "continue" not in data:
                return
            cont = data["continue"]

    def download(self, url, path):
        tmp = path + ".part"
        with open(tmp, "wb") as f:
            f.write(self._open(url))
        os.replace(tmp, path)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("site", choices=sorted(SITES))
    ap.add_argument("--api", help="override the api.php URL")
    ap.add_argument("--dest", help="output directory (default upstream/wiki/<site>)")
    ap.add_argument("--no-images", action="store_true")
    ap.add_argument("--no-html", action="store_true")
    args = ap.parse_args()

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    dest = args.dest or os.path.join(root, "upstream", "wiki", args.site)
    os.makedirs(dest, exist_ok=True)
    api = Api(args.api or SITES[args.site])

    si = api.get(action="query", meta="siteinfo", siprop="general|namespaces|statistics|rightsinfo")["query"]
    with open(os.path.join(dest, "siteinfo.json"), "w") as f:
        json.dump(si, f, indent=1)
    namespaces = {int(k): v.get("name") or "Main" for k, v in si["namespaces"].items() if int(k) >= 0}
    print(f"{si['general']['sitename']}: {si['statistics']['pages']} pages, {si['statistics']['images']} files")

    # Every page title in every namespace.
    titles = []
    for ns in sorted(namespaces):
        for data in api.query_all(list="allpages", apnamespace=ns, aplimit="max"):
            titles += [(p["title"], ns) for p in data["query"]["allpages"]]
    print(f"{len(titles)} pages across {len(namespaces)} namespaces")

    index_path = os.path.join(dest, "index.json")
    old = {}
    if os.path.exists(index_path):
        old = {e["title"]: e for e in json.load(open(index_path))}
    index = []
    fetched = 0
    for i in range(0, len(titles), 50):
        batch = titles[i : i + 50]
        ns_of = dict(batch)
        data = api.get(action="query", prop="revisions", rvprop="ids|timestamp|content", rvslots="main",
                       titles="|".join(t for t, _ in batch))
        for p in data["query"]["pages"]:
            if "revisions" not in p:
                continue
            rev = p["revisions"][0]
            ns = p.get("ns", ns_of.get(p["title"], 0))
            rel = os.path.join("wikitext", f"{ns:03d}-{namespaces.get(ns, ns)}", safe(p["title"]) + ".wiki")
            entry = {"title": p["title"], "ns": ns, "file": rel, "revid": rev["revid"], "timestamp": rev["timestamp"]}
            path = os.path.join(dest, rel)
            if old.get(p["title"], {}).get("revid") != rev["revid"] or not os.path.exists(path):
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w") as f:
                    f.write(rev["slots"]["main"].get("content", ""))
                fetched += 1
            if ns == 0 and not args.no_html:
                hrel = os.path.join("html", safe(p["title"]) + ".html")
                hpath = os.path.join(dest, hrel)
                if old.get(p["title"], {}).get("revid") != rev["revid"] or not os.path.exists(hpath):
                    parsed = api.get(action="parse", pageid=p["pageid"], prop="text|categories|links", disablelimitreport="1")
                    os.makedirs(os.path.dirname(hpath), exist_ok=True)
                    with open(hpath, "w") as f:
                        f.write(f"<!-- {p['title']} rev {rev['revid']} {rev['timestamp']} -->\n")
                        f.write(parsed["parse"]["text"])
                entry["html"] = hrel
            index.append(entry)
        print(f"  pages {min(i + 50, len(titles))}/{len(titles)} ({fetched} new/changed)", flush=True)
    with open(index_path, "w") as f:
        json.dump(index, f, indent=1)

    if args.no_images:
        return
    images = []
    for data in api.query_all(list="allimages", aiprop="url|size|sha1|mime|user|timestamp", ailimit="max"):
        images += data["query"]["allimages"]
    with open(os.path.join(dest, "images.json"), "w") as f:
        json.dump(images, f, indent=1)
    idir = os.path.join(dest, "images")
    os.makedirs(idir, exist_ok=True)
    total = sum(im.get("size", 0) for im in images)
    print(f"{len(images)} files, {total / 1e6:.1f} MB")
    got = 0
    for n, im in enumerate(images, 1):
        path = os.path.join(idir, safe(im["name"]))
        if os.path.exists(path) and os.path.getsize(path) == im.get("size"):
            if hashlib.sha1(open(path, "rb").read()).hexdigest() == im.get("sha1"):
                continue
        try:
            api.download(im["url"], path)
            got += 1
        except Exception as e:  # keep going; report at the end
            print(f"  failed {im['name']}: {e}", file=sys.stderr)
        if n % 100 == 0:
            print(f"  files {n}/{len(images)}", flush=True)
    print(f"done: {len(index)} pages, {len(images)} files ({got} downloaded)")


if __name__ == "__main__":
    main()
