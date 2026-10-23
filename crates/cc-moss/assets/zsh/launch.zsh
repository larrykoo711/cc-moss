# 原生 Claude 保留 stdin、TTY、参数和退出码；目录检查不接触 stdin。
# 已有自定义同名函数或 alias 留给用户，不静默替换。
function _cc_moss_launch_cli {
    if (( ${+CC_MOSS_LAUNCH_CLI} )); then
        if [[ "$CC_MOSS_LAUNCH_CLI" != /* || ! -x "$CC_MOSS_LAUNCH_CLI" || -d "$CC_MOSS_LAUNCH_CLI" ]]; then
            print -u2 -- 'cc-moss: CC_MOSS_LAUNCH_CLI must name an executable absolute path; no PATH fallback.'
            return 2
        fi
        command "$CC_MOSS_LAUNCH_CLI" "$@"
    else
        command 'cc-moss' "$@"
    fi
}

function _cc_moss_launch_refused {
    print -u2 -- 'cc-moss: these arguments do not establish a verifiable working directory; no session was started.'
    print -u2 -- 'Select the session explicitly in cc-moss: attach/view an existing session, or use guarded restore/restart.'
    return 2
}

function _cc_moss_native_launch {
    local mode
    mode=$(_cc_moss_launch_cli launch-mode -- "$@") || return $?
    case "$mode" in
        fresh|native|non-session) ;;
        *) _cc_moss_launch_refused; return $? ;;
    esac
    if [[ "$mode" != non-session ]]; then
        _cc_moss_launch_cli check-directory -- "$PWD" || return $?
    fi
    command 'claude' "$@"
}

if (( ! ${+functions[claude]} && ! ${+aliases[claude]} )); then
    function claude {
        _cc_moss_native_launch "$@"
    }
fi

if (( ! ${+functions[jarvis]} && ! ${+aliases[jarvis]} )); then
    function jarvis {
        local mode
        mode=$(_cc_moss_launch_cli launch-mode -- "$@") || return $?
        if [[ "$mode" == fresh ]]; then
            _cc_moss_launch_cli new "$@"
        else
            _cc_moss_native_launch "$@"
        fi
    }
fi
