#!/usr/bin/env bash
# Minimal library for isolated ephemeral acceptance sessions; no scenario runner.
# Requires Linux, GNU timeout and mktemp. Prefer upstream skill base if updated.
# shellcheck shell=bash
TH_PIDS=()
TH_PID_STARTS=()
TH_CONTAINER_IDS=()
TH_CONTAINER_ENGINES=()
TH_CLEANUP_ERROR=0
TH_SESSION=""
TH_REPORT=""

_th_pid_start() {
    [[ -r "/proc/$1/stat" ]] || return 1
    local stat
    stat=$(<"/proc/$1/stat")
    # After final ')' fields begin at state (field 3), starttime is field 22 => index 19.
    stat=${stat##*) }
    local -a fields
    read -ra fields <<< "$stat"
    printf '%s' "${fields[19]:-}"
}

th_init() {
    if [[ -z "${TH_TARGET_ID:-}" || "${TH_TARGET_ID}" == *'<'* ]]; then
        echo 'BLOCKED: TH_TARGET_ID must be verified target SHA/digest' >&2; return 2
    fi
    command -v timeout >/dev/null || { echo 'BLOCKED: GNU timeout missing' >&2; return 2; }
    command -v setsid >/dev/null || { echo 'BLOCKED: setsid missing' >&2; return 2; }
    local home="${TH_HARNESS_HOME:-test-harness}"
    mkdir -p "$home/.sessions" "$home/.reports"
    umask 077
    TH_SESSION=$(mktemp -d "$home/.sessions/run.XXXXXXXX") || return 2
    TH_REPORT="$home/.reports/$(basename "$TH_SESSION").txt"
    printf 'target=%s\nstart=%s\n' "$TH_TARGET_ID" "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" > "$TH_REPORT"
    trap 'th_cleanup "$?"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
}

th_capture() {
    local label="$1"; shift
    [[ "$label" =~ ^[a-zA-Z0-9_-]+$ ]] || { echo 'invalid label' >&2; return 2; }
    [[ -n "$TH_SESSION" && $# -gt 0 ]] || return 2
    timeout --signal=TERM --kill-after=3s "${TH_TIMEOUT_SEC:-60}s" "$@" > "$TH_SESSION/$label.log" 2>&1
}

th_assert_contains() {
    local f="$1" needle="$2"
    [[ -f "$f" ]] && grep -Fq -- "$needle" "$f"
}

th_assert_nonempty() { [[ -s "$1" ]]; }

th_start_service() {
    local name="$1"; shift
    [[ "$name" =~ ^[a-zA-Z0-9_-]+$ && $# -gt 0 ]] || return 2
    setsid "$@" > "$TH_SESSION/$name.log" 2>&1 &
    local pid=$!
    TH_PIDS+=("$pid")
    TH_PID_STARTS+=("$(_th_pid_start "$pid" || true)")
    [[ -n "${TH_PID_STARTS[-1]}" ]] || { echo 'Cannot identify spawned process' >&2; return 2; }
    printf '%s' "$pid"
}

th_register_container() {
    local engine="$1" cid="$2"
    [[ "$engine" == docker || "$engine" == podman ]] || return 2
    [[ "$cid" =~ ^[[:alnum:]_.-]+$ ]] || return 2
    command -v "$engine" >/dev/null || return 2
    TH_CONTAINER_ENGINES+=("$engine"); TH_CONTAINER_IDS+=("$cid")
}

th_cleanup() {
    local prior="${1:-0}" i pid actual engine cid
    trap - EXIT INT TERM
    for i in "${!TH_PIDS[@]}"; do
        pid="${TH_PIDS[$i]}"
        actual=$(_th_pid_start "$pid" || true)
        if [[ -n "$actual" && "$actual" == "${TH_PID_STARTS[$i]}" ]]; then
            kill -- "-$pid" 2>/dev/null || true
            # Verify group terminates; do not signal a reused PID.
            for _ in 1 2 3 4 5; do
                actual=$(_th_pid_start "$pid" || true)
                [[ -z "$actual" || "$actual" != "${TH_PID_STARTS[$i]}" ]] && break
                sleep 0.1
            done
            actual=$(_th_pid_start "$pid" || true)
            [[ -z "$actual" || "$actual" != "${TH_PID_STARTS[$i]}" ]] || TH_CLEANUP_ERROR=1
            # Descendant process group might outlive leader; never mistake leader death for full cleanup.
            if kill -0 -- "-$pid" 2>/dev/null; then TH_CLEANUP_ERROR=1; fi
        fi
    done
    for i in "${!TH_CONTAINER_IDS[@]}"; do
        engine="${TH_CONTAINER_ENGINES[$i]}"; cid="${TH_CONTAINER_IDS[$i]}"
        "$engine" rm -f -- "$cid" >/dev/null 2>&1 || TH_CLEANUP_ERROR=1
    done
    if [[ -n "$TH_SESSION" && -d "$TH_SESSION" ]]; then
        rm -rf -- "$TH_SESSION" || TH_CLEANUP_ERROR=1
    fi
    if [[ -n "$TH_REPORT" ]]; then
        printf 'command_exit=%s\ncleanup=%s\n' "$prior" "$([[ $TH_CLEANUP_ERROR -eq 0 ]] && echo complete || echo incomplete)" >> "$TH_REPORT" || true
    fi
    if [[ "$prior" -ne 0 ]]; then exit "$prior"; fi
    if [[ "$TH_CLEANUP_ERROR" -ne 0 ]]; then echo 'BLOCKED: cleanup incomplete' >&2; exit 2; fi
    exit 0
}
