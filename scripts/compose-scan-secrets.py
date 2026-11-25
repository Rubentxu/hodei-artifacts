#!/usr/bin/env python3
"""Credential-literal scanner for the compose files (HSP-258 / HSP-273).

Usage: compose-scan-secrets.py FILE...

Exit codes: 0 = clean, 1 = a credential-like literal was found, 2 = usage or
unreadable input.

A line is a finding when the key of an assignment (`KEY: value`, `KEY=value`,
`- KEY=value`) names a credential (SECRET, TOKEN, ACCESS_KEY, PASSWORD, ...)
and the value is anything other than:

  - empty;
  - a pure reference: `${NAME}`, `${NAME:?message}`, `${NAME?message}`,
    `${NAME:-}`, `${NAME-}` or `$NAME`.

Unlike the first grep-based version, this scanner does NOT skip a line just
because it contains `${`: `${NAME:-hunter2}` carries a literal default and is a
finding. It does not impose a minimum length either: a short literal such as
`abc` is a finding. Keys ending in `_FILE` hold a path, not a credential.
"""
import re
import sys

KEY = re.compile(
    r"(?P<key>[A-Za-z0-9_]*(?:SECRET|TOKEN|ACCESS_KEY|PASSWORD|PASSWD|API_KEY|PRIVATE_KEY)"
    r"[A-Za-z0-9_]*)\s*[:=]\s*(?P<value>.*)$"
)
REFERENCE = re.compile(
    r"^(?:\$\{[A-Za-z_][A-Za-z0-9_]*(?::?\?[^}]*|:?-)?\}|\$[A-Za-z_][A-Za-z0-9_]*)$"
)


def clean_value(raw):
    value = raw.strip()
    quoted = value[:1] in ("'", '"')
    if quoted:
        end = value.find(value[0], 1)
        value = value[1:end] if end > 0 else value[1:]
    else:
        value = re.split(r"\s+#", value, maxsplit=1)[0]
    return value.strip()


def scan(path):
    findings = []
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            if line.lstrip().startswith("#"):
                continue
            match = KEY.search(line)
            if not match or match.group("key").endswith("_FILE"):
                continue
            value = clean_value(match.group("value"))
            if value == "" or REFERENCE.match(value):
                continue
            findings.append((number, match.group("key")))
    return findings


def main(paths):
    if not paths:
        print("usage: compose-scan-secrets.py FILE...", file=sys.stderr)
        return 2
    found = False
    for path in paths:
        try:
            findings = scan(path)
        except OSError as error:
            print(f"cannot read {path}: {error}", file=sys.stderr)
            return 2
        for number, key in findings:
            # The value is deliberately never printed.
            print(f"{path}:{number}: credential-like literal for {key}")
            found = True
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
