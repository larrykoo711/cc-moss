#!/usr/bin/env python3
"""用隔离 Claude Code 与本机假 API 验证 resume 的模型优先级。

运行：uv run --no-project --offline python -I scripts/provider-model-probe.py
所有会话与证据留在新建临时目录；不读取真实配置，不访问真实 API。
"""

import argparse
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


OLD_MODEL = "probe-model-a"
NEW_MODEL = "probe-model-b"
CLI_MODEL = "probe-model-c"
FAKE_KEY = "probe-fake-key-not-a-credential"
MARKER = re.compile(r"PROBE_[A-Z0-9_]+")


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def markers(value):
    return sorted(set(MARKER.findall(json.dumps(value))))


class FakeAPI(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_CONNECT(self):
        self.send_error(403, "External connections disabled by probe")

    def do_GET(self):
        self.send_error(404, "Probe has no external or discovery endpoint")

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        if self.path.startswith("/v1/messages/count_tokens"):
            data = json.dumps({"input_tokens": 16}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if not self.path.startswith("/v1/messages"):
            self.send_error(404, "Only local fake Messages API is supported")
            return
        messages = body.get("messages", [])
        found = markers(messages)
        current = next((markers(message.get("content", ""))
                        for message in reversed(messages)
                        if message.get("role") == "user"
                        and markers(message.get("content", ""))), [])
        current_marker = current[-1] if current else "PROBE_UNKNOWN"
        session = None
        user_id = body.get("metadata", {}).get("user_id", "")
        match = re.search(r"session_([0-9a-f-]{36})", user_id)
        if match:
            session = match.group(1)
        record = {
            "path": self.path,
            "model": body.get("model"),
            "sessionId": session,
            "current_marker": current_marker,
            "history_markers": found,
            "stream": body.get("stream", False),
        }
        with self.server.record_lock:
            self.server.records.append(record)
            with (self.server.evidence / "requests.jsonl").open("a") as handle:
                handle.write(json.dumps(record) + "\n")
        reply = "FAKE_REPLY_" + current_marker
        message = {
            "id": "msg_probe_" + uuid.uuid4().hex,
            "type": "message",
            "role": "assistant",
            "model": body.get("model"),
            "content": [{"type": "text", "text": reply}],
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 16, "output_tokens": 8},
        }
        if body.get("stream"):
            start = dict(message, content=[], stop_reason=None)
            events = [
                ("message_start", {"type": "message_start", "message": start}),
                ("content_block_start", {"type": "content_block_start", "index": 0,
                 "content_block": {"type": "text", "text": ""}}),
                ("content_block_delta", {"type": "content_block_delta", "index": 0,
                 "delta": {"type": "text_delta", "text": reply}}),
                ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                ("message_delta", {"type": "message_delta",
                 "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                 "usage": {"output_tokens": 8}}),
                ("message_stop", {"type": "message_stop"}),
            ]
            data = "".join(f"event: {event}\ndata: {json.dumps(payload)}\n\n"
                           for event, payload in events).encode()
            content_type = "text/event-stream"
        else:
            data = json.dumps(message).encode()
            content_type = "application/json"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def clean_env(case_dir, endpoint):
    home = case_dir / "home"
    config = case_dir / "config"
    state = case_dir / "state"
    for directory in (home, config, state):
        directory.mkdir(parents=True, exist_ok=True)
    return {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(home),
        "TMPDIR": str(case_dir),
        "CLAUDE_CONFIG_DIR": str(config),
        "CC_MOSS_STATE_DIR": str(state),
        "ANTHROPIC_API_KEY": FAKE_KEY,
        "ANTHROPIC_BASE_URL": endpoint,
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
        "DISABLE_AUTOUPDATER": "1",
        "DISABLE_ERROR_REPORTING": "1",
        "DISABLE_TELEMETRY": "1",
        "HTTP_PROXY": endpoint,
        "HTTPS_PROXY": endpoint,
        "ALL_PROXY": endpoint,
        "NO_PROXY": "127.0.0.1,localhost",
        "LANG": "en_US.UTF-8",
    }


def invoke(binary, case_dir, endpoint, label, args, children, mode):
    env = clean_env(case_dir, endpoint)
    command = [binary, "--bare" if mode == "bare" else "--safe-mode",
               "--setting-sources", "user",
               "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}',
               "--disable-slash-commands", "--tools", "", "--max-turns", "1",
               "--output-format", "json", "-p", *args]
    process = subprocess.Popen(command, cwd=case_dir, env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, start_new_session=True)
    children.append(process)
    try:
        stdout, stderr = process.communicate(timeout=45)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate()
        raise AssertionError(f"{label}: CLI timed out (pid {process.pid})")
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    (case_dir / f"{label}.stdout").write_text(stdout.replace(FAKE_KEY, "[fake-key]"))
    (case_dir / f"{label}.stderr").write_text(stderr.replace(FAKE_KEY, "[fake-key]"))
    assert process.returncode == 0, f"{label}: CLI rc={process.returncode}; see evidence"
    result = json.loads(stdout)
    assert not result.get("is_error"), f"{label}: CLI returned is_error; see evidence"
    return result, process.pid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", required=True, help="已安装 Claude Code 的绝对路径")
    parser.add_argument("--mode", choices=("bare", "safe"), default="bare",
                        help="safe 为非 bare、禁自定义的 user-source 控制组")
    args = parser.parse_args()
    binary = str(Path(args.claude).resolve())
    evidence = Path(tempfile.mkdtemp(prefix="cc-moss-model-probe-"))
    server = ThreadingHTTPServer(("127.0.0.1", 0), FakeAPI)
    server.evidence = evidence
    server.records = []
    server.record_lock = threading.Lock()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    endpoint = f"http://127.0.0.1:{server.server_port}"
    children = []
    results = []
    print(f"evidence={evidence}", flush=True)
    print(f"endpoint={endpoint}", flush=True)
    cases = [
        ("TOP_LEVEL", {"model": NEW_MODEL}, [], NEW_MODEL),
        ("SETTINGS_ENV", {"env": {"ANTHROPIC_MODEL": NEW_MODEL}}, [], NEW_MODEL),
        ("EXPLICIT", {"model": NEW_MODEL, "env": {"ANTHROPIC_MODEL": NEW_MODEL}},
         ["--model", CLI_MODEL], CLI_MODEL),
        ("DEFAULT", {}, ["--model", "default"], None),
        ("FAMILY", {"model": "opus", "env": {"ANTHROPIC_DEFAULT_OPUS_MODEL": NEW_MODEL}},
         ["--model", "opus"], NEW_MODEL),
    ]
    try:
        version_dir = evidence / "version"
        version_dir.mkdir()
        version = subprocess.run([binary, "--version"], env=clean_env(version_dir, endpoint),
                                 cwd=version_dir, capture_output=True, text=True, timeout=10)
        assert version.returncode == 0 and "2.1.306" in version.stdout, version.stdout
        write_json(evidence / "environment.json", {
            "version": version.stdout.strip(), "endpoint": endpoint,
            "server_pid": os.getpid(), "settings_sources": "user",
            "bare": args.mode == "bare", "safe_mode": args.mode == "safe",
            "settings_flag": False,
            "isolation": "fresh HOME/config/state/cwd; fake key; local-only API; external proxy rejected",
        })
        default_model = None
        for name, settings, extra, expected in cases:
            case_dir = evidence / name.lower()
            case_dir.mkdir()
            clean_env(case_dir, endpoint)
            settings_file = case_dir / "config" / "settings.json"
            session_id = str(uuid.uuid4())
            seed_marker = f"PROBE_{name}_SEED"
            resume_marker = f"PROBE_{name}_RESUME"
            row = {"case": name, "sessionId": session_id, "expected": expected}
            try:
                write_json(settings_file, {"model": OLD_MODEL})
                seed, seed_pid = invoke(binary, case_dir, endpoint, "seed",
                                        ["--session-id", session_id, "--model", OLD_MODEL, seed_marker], children, args.mode)
                row["seed_pid"] = seed_pid
                assert seed.get("session_id") == session_id, "seed session ID changed"
                seed_requests = [r for r in server.records if r["current_marker"] == seed_marker]
                assert seed_requests and seed_requests[0]["model"] == OLD_MODEL, "seed did not request old model A"
                transcripts = list((case_dir / "config").rglob(f"{session_id}.jsonl"))
                assert len(transcripts) == 1, f"expected one transcript, got {len(transcripts)}"
                transcript = transcripts[0]
                before = transcript.read_bytes()
                assert seed_marker.encode() in before, "seed marker missing from transcript"
                write_json(settings_file, settings)
                resumed, resume_pid = invoke(binary, case_dir, endpoint, "resume",
                                             ["--resume", session_id, *extra, resume_marker], children, args.mode)
                row["resume_pid"] = resume_pid
                assert resumed.get("session_id") == session_id, "resume changed session ID"
                matches = [r for r in server.records if r["current_marker"] == resume_marker]
                assert matches, "no main resume request captured"
                request = matches[0]
                row["actual"] = request["model"]
                row["request_count"] = len(matches)
                row["history_visible"] = seed_marker in request["history_markers"]
                row["session_id_preserved"] = True
                after = transcript.read_bytes()
                row["same_transcript_appended"] = after.startswith(before) and len(after) > len(before)
                row["transcript"] = str(transcript)
                assert row["history_visible"], "seed history not present in first resume request"
                assert row["same_transcript_appended"], "same transcript was not appended"
                assert resume_marker.encode() in after, "resume marker absent from transcript"
                if name == "DEFAULT":
                    fresh, fresh_pid = invoke(binary, case_dir, endpoint, "fresh-default",
                                              ["--session-id", str(uuid.uuid4()), "PROBE_DEFAULT_FRESH"], children, args.mode)
                    row["fresh_default_pid"] = fresh_pid
                    fresh_requests = [r for r in server.records if r["current_marker"] == "PROBE_DEFAULT_FRESH"]
                    assert fresh_requests, "no fresh default request captured"
                    default_model = fresh_requests[0]["model"]
                    row["fresh_default_model"] = default_model
                    assert row["actual"] == default_model, "default resume differs from fresh default"
                    assert row["actual"] != OLD_MODEL, "default retained old model"
                else:
                    assert row["actual"] == expected, f"model expected {expected}, got {row['actual']}"
                row["passed"] = True
            except (AssertionError, ValueError, OSError, subprocess.TimeoutExpired) as error:
                row["passed"] = False
                row["error"] = str(error)
            results.append(row)
            write_json(evidence / "results.json", results)
            print(json.dumps(row, ensure_ascii=False), flush=True)
    finally:
        for process in children:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        write_json(evidence / "cleanup.json", {
            "child_pids": [process.pid for process in children],
            "all_children_exited": all(process.poll() is not None for process in children),
            "server_thread_exited": not thread.is_alive(),
            "server_socket_closed": server.socket.fileno() == -1,
        })
    assert len(results) == len(cases), "probe did not complete all cases"
    failures = [row["case"] for row in results if not row["passed"]]
    assert not failures, "probe assertions failed: " + ", ".join(failures)
    print("PASS: all five isolated resume cases", flush=True)


if __name__ == "__main__":
    main()
