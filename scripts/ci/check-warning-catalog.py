#!/usr/bin/env python3
"""Reconstruct exact builtin warning catalogs from pinned, caller-supplied sources.

Offline by default. --oracle-root additionally compares isolated Perl runtimes.
No downloaded Perl generator is executed. --contaminate-oracle is an intentional
negative instrument control: helper registration must fail the denominator check.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request

SOURCES = {
    "5.36.3": ("46ff3a554aff65378c2d3f7f5868e35ecd6964f7aaa972e7294e588e50230aed", "8710df821360065bf364da87908ae7ba2eb0e5126e0850010708e101c06a9165"),
    "5.38.5": ("59de2009e061feb90df252e364e9ff19973c9cdd2c2b655f40a665d871945892", "01d1e55728ac5bed27e861658776221da6407012de1449e0a5f389a073da188b"),
    "5.42.3": ("ae12326adcd5e48e948dad139abf5906a3c5c19719ec47c0d86ef65222c47d15", "b13c8e18aa95dc467db6bfbd825df867dc24b253944b17d38cde79b0555de033"),
    "5.44.0": ("7697bbbc339476da60178daffa5770417ec6a548dda8d2aa00a8d30bffd4d49d", "732b1dfd9140f79d5a98f48f67beb70db5457bd7ab2612b114181e98f7af3d25"),
}
COUNTS = {"5.36.3": 80, "5.38.5": 80, "5.42.3": 80, "5.44.0": 81}
DOC_SOURCES = {
    "5.36.3": ("f884e9179cd02e84e1f91fa3a61b8ad224907e136c242f7d99de9c95f4920d6a", "9609087098adaa5a31b524922f210c2b8d7687fc2818e98cfed306656bac03c1"),
    "5.38.5": ("3551120fd57cccad20a4f437dd0bdd3ed1a51eca390e397dbbdf10af14f30ea4", "30fcccdf8f9dbe7787289bfe31faaa362758c76d84099235ae61aaea60bcdde9"),
    "5.42.3": ("49ba784cca80e921b2205a6afbec8763a18346da76a1821c240c7679b18eed40", "1cbc8e30133fc03a44bad3b9c292bcdd7b7cba7554da1208d94130aa3b3f5b38"),
    "5.44.0": ("3bedafdb3ff719ce1e5914d31bf2d282ef7cb2c3d5db1c0f1ca88462bc1f8586", "98982daba451ad7fbfab360f600e3e06c99719c3a5e6a138fc966ca8119b53d5"),
}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def bounded_source(path):
    with path.open("rb") as stream:
        data = stream.read(1_048_577)
    require(len(data) <= 1_048_576, "source exceeds bound")
    return data


class Tree:
    """Small rejecting grammar for the upstream literal, not a Perl evaluator."""
    def __init__(self, text):
        text = re.sub(r"#[^\n]*", "", text)
        token = re.compile(r"\s*('(?:[^'\\]|\\.)*'|\d+\.\d+|DEFAULT_ON|DEFAULT_OFF|=>|[{},\[\]])")
        self.tokens = []
        offset = 0
        while text[offset:].strip():
            match = token.match(text, offset)
            require(match is not None, f"unrecognized tree literal at {offset}")
            self.tokens.append(match.group(1))
            offset = match.end()
        self.index = 0

    def pop(self, expected=None):
        require(self.index < len(self.tokens), "truncated tree")
        token = self.tokens[self.index]
        self.index += 1
        require(expected is None or token == expected, f"expected {expected}, got {token}")
        return token

    def value(self):
        token = self.pop()
        if token in ("{", "["):
            close = "}" if token == "{" else "]"
            result = {} if token == "{" else []
            while self.tokens[self.index] != close:
                if token == "{":
                    name = self.pop()[1:-1]
                    self.pop("=>")
                    require(name not in result, f"duplicate tree name {name}")
                    result[name] = self.value()
                else:
                    result.append(self.value())
                if self.tokens[self.index] == ",":
                    self.pop(",")
                else:
                    require(self.tokens[self.index] == close, "missing literal separator")
            self.pop(close)
            return result
        return token


def section(text, name):
    match = re.search(r"our %" + name + r"\s*=\s*\((.*?)\n\);", text, re.S)
    require(match is not None, f"missing generated {name}")
    return match.group(1)


def masks(text, name):
    return {key: bytes.fromhex("".join(re.findall(r"\\x([0-9a-fA-F]{2})", value))).hex()
            for key, value in re.findall(r"'([^']+)'\s*=>\s*\"([^\"]*)\"", section(text, name))}


def extract(root, version):
    authority = []
    source = []
    for label, upstream, expected in zip(("regen", "module", "feature", "perlsub"),
            ("regen/warnings.pl", "lib/warnings.pm", "lib/feature.pm", "pod/perlsub.pod"),
            SOURCES[version] + DOC_SOURCES[version]):
        data = bounded_source(root / f"{version}-{label}.txt")
        digest = hashlib.sha256(data).hexdigest()
        require(digest == expected, f"pinned source hash mismatch: {version} {label}")
        authority.append({"url": f"https://raw.githubusercontent.com/Perl/perl5/v{version}/{upstream}", "sha256": digest})
        source.append(data.decode())
    regen, module, feature, perlsub = source
    match = re.search(r"(?:our|my)\s+\$(?:WARNING_TREE|TREE)\s*=\s*(\{.*?\});", regen, re.S)
    require(match is not None, "missing warning tree literal")
    literal = match.group(1)
    parser = Tree(literal)
    tree = parser.value()
    require(parser.index == len(parser.tokens), "trailing tree tokens")
    retired_match = re.search(r"my %NO_BIT_FOR\s*=.*?qw\((.*?)\);", regen, re.S)
    retired = sorted(retired_match.group(1).split()) if retired_match else []
    offsets = {name: int(value) for name, value in re.findall(r"'([^']+)'\s*=>\s*(\d+)", section(module, "Offsets"))}
    bits, fatal = masks(module, "Bits"), masks(module, "DeadBits")
    default_literal = re.search(r"our \$DEFAULT\s*=\s*\"([^\"]*)\"", module).group(1)
    default = bytes.fromhex("".join(re.findall(r"\\x([0-9a-fA-F]{2})", default_literal)))
    rows = {}

    def walk(nodes, parent):
        for name, data in nodes.items():
            require(name not in rows, f"duplicate category {name}")
            children = next((part for part in data if isinstance(part, dict)), {})
            default_flag = next((part for part in data if part in ("DEFAULT_ON", "DEFAULT_OFF")), None)
            rows[name] = {"id": f"perl.warning/{name}", "name": name, "parent": parent,
                          "introduced_upstream": data[0], "declared_default": default_flag,
                          "aliases": [], "builtin": True}
            walk(children, name)
    walk(tree, None)
    relations = {name: [] for name in set(rows) | set(retired)}
    sections = re.split(r"(?m)^=head2 ", feature)
    for section_text in sections[1:]:
        heading, _, body = section_text.partition("\n")
        if not heading.startswith("The ") or "feature" not in heading:
            continue
        features = re.findall(r"'([^']+)'", heading)
        mentioned = set(re.findall(r"experimental::[a-z_]+", body)) & set(relations)
        for name in sorted(mentioned):
            for related in features:
                relations[name].append({"kind": "documented_feature_association", "related_name": related,
                    "qualifier": "historical_warning" if name in ("experimental::signatures", "experimental::postderef") else "documentation_only",
                    "source_section": heading, "source_url": authority[2]["url"], "source_sha256": authority[2]["sha256"]})
    signature_match = re.search(r"(?ms)^=head2 Signatures\s*\n(.*?)(?=^=head[12] |\Z)", perlsub)
    require(signature_match is not None, "missing Signatures documentation section")
    signatures = signature_match.group(1)
    for name, context in [("experimental::signature_named_parameters", "named_parameters"),
                          ("experimental::args_array_with_signatures", "args_array_access")]:
        if name in relations and name in signatures:
            relations[name].append({"kind": "documented_signature_syntax", "related_name": "signatures",
                "qualifier": context, "source_section": "Signatures", "source_url": authority[3]["url"],
                "source_sha256": authority[3]["sha256"]})
    for name, row in rows.items():
        row["documented_relationships"] = relations[name]
    live = {name: row for name, row in rows.items() if name not in retired}
    require(set(live) == set(offsets) == set(bits) == set(fatal), f"source denominator mismatch {version}")
    require(len(live) == COUNTS[version], f"wrong builtin count {version}")
    for name, row in live.items():
        offset = offsets[name]
        row.update(native_offset=offset, enabled_mask=bits[name], fatal_mask=fatal[name],
                   default_enabled=bool(default[offset // 8] & (1 << (offset % 8))))
        descendants = {other for other in live if other == name or ancestor(other, name, live)}
        mask = bytes.fromhex(bits[name])
        observed = {other for other, bit in offsets.items() if mask[bit // 8] & (1 << (bit % 8))}
        require(descendants == observed, f"tree/mask mismatch {version} {name}")
    return {"schema_version": 1, "profile": version, "sources": authority,
            "categories": [live[name] for name in sorted(live)],
            "accepted_noops": [{"name": name, "tree_metadata": rows.get(name), "documented_relationships": relations[name]} for name in retired],
            "default_mask": default.hex()}


def ancestor(child, parent, rows):
    visited = set()
    while rows[child]["parent"] is not None:
        require(child not in visited, "category cycle")
        visited.add(child)
        child = rows[child]["parent"]
        require(child in rows, "missing parent")
        if child == parent:
            return True
    return False


def oracle(catalog, root, contaminate):
    version = catalog["profile"]
    executable = root / f"perl-{version}/runtime/bin/perl.exe"
    code = ("require JSON::PP; " if contaminate else "") + r'''
my %offsets=%warnings::Offsets;
my %bits=map { $_=>unpack("H*",$warnings::Bits{$_}) } keys %warnings::Bits;
my %fatal=map { $_=>unpack("H*",$warnings::DeadBits{$_}) } keys %warnings::DeadBits;
my %row=(version=>"$^V", offsets=>\%offsets,bits=>\%bits,fatal=>\%fatal,
default=>unpack("H*",$warnings::DEFAULT),module=>$INC{"warnings.pm"});
my %retired; for my $name (@ARGV) { my $mask=eval { warnings::bits($name) }; $retired{$name}={error=>"$@",nonzero=>defined($mask)&&$mask=~/[^\x00]/?1:0}; }
$row{retired}=\%retired;
require JSON::PP; print JSON::PP->new->canonical->encode(\%row);
'''
    env = dict(os.environ, PERL5OPT="", PERL5LIB="", LC_ALL="C", LANG="C", LC_CTYPE="C")
    result = subprocess.run([str(executable), "-Mwarnings", "-e", code, *[row["name"] for row in catalog["accepted_noops"]]], env=env, capture_output=True, timeout=30)
    require(result.returncode == 0 and not result.stderr, f"oracle failed: {result.stderr!r}")
    data = json.loads(result.stdout)
    rows = {row["name"]: row for row in catalog["categories"]}
    require(set(data["offsets"]) == set(rows), f"builtin oracle denominator {version}: expected {len(rows)}, got {len(data['offsets'])}; extra={sorted(set(data['offsets'])-set(rows))}")
    require(data["version"] == "v" + version, "wrong runtime version")
    require(data["default"] == catalog["default_mask"], "default mask differs")
    for name, row in rows.items():
        require(data["offsets"][name] == row["native_offset"] and data["bits"][name] == row["enabled_mask"] and data["fatal"][name] == row["fatal_mask"], f"oracle row differs: {name}")
    require(all(row == {"error": "", "nonzero": 0} for row in data["retired"].values()), "retired name not accepted no-op")
    module_hash = hashlib.sha256(bounded_source(Path(data["module"]))).hexdigest()
    require(module_hash == catalog["sources"][1]["sha256"], "runtime module differs from pinned source")
    return {"profile": version, "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
            "module_sha256": module_hash, "builtin_count": len(rows), "retired_count": len(data["retired"])}


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--sources", type=Path, required=True)
    args.add_argument("--output", type=Path, required=True)
    args.add_argument("--check", action="store_true")
    args.add_argument("--fetch", action="store_true", help="explicitly download hash-pinned sources before offline generation/check")
    args.add_argument("--oracle-root", type=Path)
    args.add_argument("--receipt", type=Path)
    args.add_argument("--contaminate-oracle", action="store_true")
    args = args.parse_args()
    require(not args.contaminate_oracle or args.oracle_root is not None,
            "--contaminate-oracle requires --oracle-root")
    if args.fetch:
        args.sources.mkdir(parents=True, exist_ok=True)
        for version in SOURCES:
            for label, upstream, expected in zip(("regen", "module", "feature", "perlsub"),
                    ("regen/warnings.pl", "lib/warnings.pm", "lib/feature.pm", "pod/perlsub.pod"),
                    SOURCES[version] + DOC_SOURCES[version]):
                with urllib.request.urlopen(f"https://raw.githubusercontent.com/Perl/perl5/v{version}/{upstream}", timeout=30) as response:
                    data = response.read(1_048_577)
                require(len(data) <= 1_048_576, "download source exceeds bound")
                require(hashlib.sha256(data).hexdigest() == expected, "downloaded source identity mismatch")
                (args.sources / f"{version}-{label}.txt").write_bytes(data)
    catalogs = [extract(args.sources, version) for version in SOURCES]
    if args.oracle_root:
        receipts = [oracle(catalog, args.oracle_root, args.contaminate_oracle) for catalog in catalogs]
        if args.receipt:
            args.receipt.write_text(json.dumps(receipts, indent=2) + "\n", encoding="utf-8")
    encoded = (json.dumps(catalogs, indent=2, ensure_ascii=False) + "\n").encode()
    if args.check:
        require(args.output.read_bytes() == encoded, "generated catalog differs")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(encoded)
    print("PASS complete catalogs:", ", ".join(f"{v}={COUNTS[v]}" for v in SOURCES))


if __name__ == "__main__":
    main()
