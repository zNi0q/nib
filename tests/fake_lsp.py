#!/usr/bin/env python3
"""Minimal fake LSP server for nib's end-to-end tests (stdlib only).

Speaks JSON-RPC over stdio with Content-Length framing. Behavior:
- initialize: advertises full sync, hover, definition, completion ('.' trigger).
- after `initialized`: sends a workspace/configuration request and publishes
  no diagnostics until the client answers it.
- didOpen/didChange: lines with BAD -> error "bad thing here",
  lines with WARN -> warning "careful" (UTF-16 columns).
- hover: "**fake hover** for <word>".
- definition: `fn <word>` in the same file; word `other` -> other.fk line 1 col 3.
- completion: alpha (detail "first"), alphabet, beta.
Env: FAKE_PIDFILE (pid written on start), FAKE_LOG (received methods appended).
"""

import json
import os
import re
import sys

stdin = sys.stdin.buffer
stdout = sys.stdout.buffer

docs = {}  # uri -> text
config_answered = False
config_req_id = "cfg-1"
LOG = os.environ.get("FAKE_LOG")


def log(method):
    if LOG:
        with open(LOG, "a") as f:
            f.write(method + "\n")


def send(msg):
    msg["jsonrpc"] = "2.0"
    body = json.dumps(msg).encode()
    stdout.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    stdout.flush()


def read():
    length = None
    while True:
        line = stdin.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        k, _, v = line.decode().partition(":")
        if k.lower() == "content-length":
            length = int(v.strip())
    if length is None:
        return None
    return json.loads(stdin.read(length))


def utf16_len(s):
    return len(s.encode("utf-16-le")) // 2


def char_at_utf16(line, col16):
    """Index into `line` (python str) of UTF-16 column col16."""
    n = 0
    for i, ch in enumerate(line):
        if n >= col16:
            return i
        n += utf16_len(ch)
    return len(line)


def word_at(uri, pos):
    lines = docs.get(uri, "").split("\n")
    if pos["line"] >= len(lines):
        return ""
    line = lines[pos["line"]]
    i = char_at_utf16(line, pos["character"])
    for m in re.finditer(r"\w+", line):
        if m.start() <= i <= m.end():
            return m.group(0)
    return ""


def publish(uri):
    if not config_answered:
        return
    diags = []
    for y, line in enumerate(docs.get(uri, "").split("\n")):
        for needle, sev, msg in (("BAD", 1, "bad thing here"), ("WARN", 2, "careful")):
            x = line.find(needle)
            if x >= 0:
                c = utf16_len(line[:x])
                diags.append({
                    "range": {"start": {"line": y, "character": c},
                              "end": {"line": y, "character": c + len(needle)}},
                    "severity": sev,
                    "source": "fake",
                    "message": msg,
                })
    send({"method": "textDocument/publishDiagnostics",
          "params": {"uri": uri, "diagnostics": diags}})


def main():
    global config_answered
    pidfile = os.environ.get("FAKE_PIDFILE")
    if pidfile:
        with open(pidfile, "w") as f:
            f.write(str(os.getpid()))
        # A helper process, like real servers start (e.g. rust-analyzer's proc-macro server).
        import subprocess
        helper = subprocess.Popen(["sleep", "300"], stdin=subprocess.DEVNULL)
        with open(pidfile + ".helper", "w") as f:
            f.write(str(helper.pid))
        # Stubborn mode: ignore exit requests and SIGTERM (needs SIGKILL).
        if os.path.exists(pidfile + ".stubborn"):
            import signal
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            os.environ["FAKE_STUBBORN"] = "1"

    stubborn = os.environ.get("FAKE_STUBBORN") == "1"
    while True:
        msg = read()
        if msg is None:
            while stubborn:
                import time
                time.sleep(1)
            return
        method = msg.get("method")
        if method:
            log(method)
        mid = msg.get("id")

        # Response from the client (to our configuration request).
        if method is None:
            if mid == config_req_id and "result" in msg:
                config_answered = True
                for uri in docs:
                    publish(uri)
            continue

        p = msg.get("params") or {}
        if method == "initialize":
            send({"id": mid, "result": {"capabilities": {
                "textDocumentSync": 1,
                "hoverProvider": True,
                "definitionProvider": True,
                "completionProvider": {"triggerCharacters": ["."]},
            }, "serverInfo": {"name": "fake"}}})
        elif method == "initialized":
            send({"id": config_req_id, "method": "workspace/configuration",
                  "params": {"items": [{"section": "fake"}, {"section": "fake.more"}]}})
        elif method == "textDocument/didOpen":
            td = p["textDocument"]
            docs[td["uri"]] = td["text"]
            publish(td["uri"])
        elif method == "textDocument/didChange":
            uri = p["textDocument"]["uri"]
            for ch in p["contentChanges"]:
                if "range" not in ch:
                    docs[uri] = ch["text"]
            publish(uri)
        elif method == "textDocument/didClose":
            docs.pop(p["textDocument"]["uri"], None)
        elif method == "textDocument/hover":
            w = word_at(p["textDocument"]["uri"], p["position"])
            send({"id": mid, "result": {"contents": {
                "kind": "markdown", "value": "**fake hover** for " + w}}})
        elif method == "textDocument/definition":
            uri = p["textDocument"]["uri"]
            w = word_at(uri, p["position"])
            result = None
            if w == "other":
                base = uri.rsplit("/", 1)[0]
                result = {"uri": base + "/other.fk",
                          "range": {"start": {"line": 1, "character": 3},
                                    "end": {"line": 1, "character": 3}}}
            elif w:
                for y, line in enumerate(docs.get(uri, "").split("\n")):
                    m = re.search(r"\bfn\s+" + re.escape(w) + r"\b", line)
                    if m:
                        c = utf16_len(line[:m.start()])
                        result = {"uri": uri,
                                  "range": {"start": {"line": y, "character": c},
                                            "end": {"line": y, "character": c}}}
                        break
            send({"id": mid, "result": result})
        elif method == "textDocument/completion":
            send({"id": mid, "result": [
                {"label": "alpha", "detail": "first"},
                {"label": "alphabet"},
                {"label": "beta"},
            ]})
        elif method == "shutdown":
            send({"id": mid, "result": None})
        elif method == "exit":
            if not stubborn:
                return
        elif mid is not None:
            send({"id": mid, "result": None})


if __name__ == "__main__":
    try:
        main()
    except (BrokenPipeError, KeyboardInterrupt):
        pass
