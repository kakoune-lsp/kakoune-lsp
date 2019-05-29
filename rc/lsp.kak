### Options and faces ###

# Feel free to update path and arguments according to your setup when sourcing lsp.kak directly.
# Sourcing via `kak-lsp --kakoune` does it automatically.
declare-option -docstring "Command with which lsp is run" str lsp_cmd "kak-lsp -s %val{session}"

# Faces

# Faces used by inline diagnostics.
set-face global DiagnosticError red
set-face global DiagnosticWarning yellow
# Line flags for errors and warnings both use this face.
set-face global LineFlagErrors red
# Face for highlighting references.
set-face global Reference MatchingChar

# Options for tuning kak-lsp behaviour.

# Display hover info anchored to the hovered position.
declare-option -docstring "Display hover info anchored to the hovered position" bool lsp_hover_anchor false
# Completions request is sent only when this expression doesn't fail.
# By default, it ensures that preceding character is not a whitespace.
declare-option -docstring "Completions request is sent only when this expression does not fail" str lsp_completion_trigger %{execute-keys '<a-h><a-k>\S.\z<ret>'}
# If hover in insert mode is enabled then request is made only when this expression doesn't fail and
# for position at which it moves cursor; by default, it ensures that cursor is after opening parens
# and then moves cursor to opening parens to request hover info for current function; note that it
# doesn't handle well nested function calls.
declare-option -docstring "If hover in insert mode is enabled then request is made only when this expression does not fail and for position at which it moves cursor" \
str lsp_hover_insert_mode_trigger %{execute-keys '<a-f>(s\A[^)]+[)]?\z<ret>'}
# Formatting: prefer spaces over tabs.
declare-option -docstring "Prefer spaces over tabs" bool lsp_insert_spaces true
# Set to true to automatically highlight references with Reference face.
declare-option -docstring "Automatically highlight references with Reference face" bool lsp_auto_highlight_references false
# Set it to a positive number to limit the size of the lsp-hover output.
# (e.g. `set global lsp_hover_max_lines 40` would cut hover down to 40 lines)
declare-option -docstring "Set it to a positive number to limit the size of the lsp hover output" int lsp_hover_max_lines 0
# Configuration to send in DidChangeNotification messages.
declare-option -docstring "Configuration to send in DidChangeNotification messages" str-to-str-map lsp_server_configuration
# Configuration to send in initializationOptions of Initialize messages.
declare-option -docstring "Configuration to send in initializationOptions of Initialize messages." str-to-str-map lsp_server_initialization_options
# Line flags for inline diagnostics.
declare-option -docstring "Character to signal an error in the gutter" str lsp_diagnostic_line_error_sign '*'
declare-option -docstring "Character to signal a warning in the gutter" str lsp_diagnostic_line_warning_sign '!'
# Another good default:
# set-option global lsp_diagnostic_line_error_sign '▓'
# set-option global lsp_diagnostic_line_warning_sign '▒'

# Options for information exposed by kak-lsp.

# Count of diagnostics published for the current buffer.
declare-option -docstring "Number of errors" int lsp_diagnostic_error_count 0
declare-option -docstring "Number of warnings" int lsp_diagnostic_warning_count 0

# Internal variables.

declare-option -hidden completions lsp_completions
declare-option -hidden range-specs lsp_errors
declare-option -hidden line-specs lsp_error_lines
declare-option -hidden range-specs cquery_semhl
declare-option -hidden str lsp_draft
declare-option -hidden int lsp_timestamp -1
declare-option -hidden range-specs lsp_references

### Requests ###

define-command lsp-start -docstring "Start kak-lsp session" %{ nop %sh{ (${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & } }

define-command -hidden lsp-did-change -docstring "Notify language server about buffer change" %{ try %{
    evaluate-commands %sh{
        if [ $kak_opt_lsp_timestamp -eq $kak_timestamp ]; then
            echo "fail"
        else
            echo "evaluate-commands -draft -no-hooks %{set-option buffer lsp_timestamp %val{timestamp}; execute-keys '%'; set-option buffer lsp_draft %val{selection}}"
        fi
    }
    nop %sh{ (
lsp_draft=$(printf '%s.' "${kak_opt_lsp_draft}" | sed 's/\\/\\\\/g' | sed 's/"/\\"/g' | sed "s/$(printf '\t')/\\\\t/g")
lsp_draft=${lsp_draft%.}
printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/didChange"
[params]
draft    = """
%s"""
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${lsp_draft}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null }
}}

define-command -hidden lsp-completion -docstring "Request completions for the main cursor position" %{
lsp-did-change
try %{
    # Fail if preceding character is a whitespace (by default; the trigger could be customized).
    evaluate-commands -draft %opt{lsp_completion_trigger}

    # Kakoune requires completions to point fragment start rather than cursor position.
    # We try to detect it and put into lsp_completion_offset and then pass via completion.offset
    # parameter to the kak-lsp server so it can use it when sending completions back.
    declare-option -hidden str lsp_completion_offset

    set-option window lsp_completion_offset %val{cursor_column}
    evaluate-commands -draft %{
        try %{
            execute-keys <esc><a-h>s\$?\w+.\z<ret>
            set-option window lsp_completion_offset %val{cursor_column}
        }
    }

    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/completion"
[params.position]
line      = %d
column    = %d
[params.completion]
offset    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} ${kak_opt_lsp_completion_offset} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}}

define-command lsp-hover -docstring "Request hover info for the main cursor position" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/hover"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-definition -docstring "Go to definition" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/definition"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-code-actions -docstring "Request code actions for the main cursor position" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/codeAction"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}


define-command -hidden lsp-execute-command -params 2 -docstring "Execute a command" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "workspace/executeCommand"
[params]
command = "%s"
arguments = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" "$2" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}


define-command lsp-references -docstring "Open buffer with symbol references" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/references"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-references-next-match -docstring 'Jump to the next references match' %{
    lsp-next-match '*references*'
}

define-command lsp-references-previous-match -docstring 'Jump to the previous references match' %{
    lsp-previous-match '*references*'
}

define-command lsp-highlight-references -docstring "Highlight symbol references" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/referencesHighlight"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-rename -params 1 -docstring "Rename symbol under the main cursor" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/rename"
[params]
newName   = "%s"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-rename-prompt -docstring "Rename symbol under the main cursor (prompt for a new name)" %{
    evaluate-commands -save-regs a %{
        # It'd be more obvious to use "evaluate-commands -draft" and %val{selection},
        # but :prompt doesn't work inside a draft context for some reason.
        execute-keys <space><a-i>w"ay
        prompt -init "%reg{a}" 'New name: ' %{ lsp-rename %val{text} }
    }
}

define-command lsp-signature-help -docstring "Request signature help for the main cursor position" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/signatureHelp"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-diagnostics -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    lsp-did-change
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/diagnostics"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-document-symbol -docstring "Open buffer with document symbols" %{
    lsp-did-change
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/documentSymbol"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-symbols-next-match -docstring 'Jump to the next symbols match' %{
    lsp-next-match '*symbols*'
}

define-command lsp-symbols-previous-match -docstring 'Jump to the previous symbols match' %{
    lsp-previous-match '*symbols*'
}

define-command -hidden lsp-workspace-symbol-buffer -params 4 -docstring %{
    buffile filetype timestamp query
    Open buffer with a list of project-wide symbols matching the query
    on behalf of the buffile at timestamp
 } %{ try %{
    evaluate-commands %sh{
        if [ -z "${4}" ];
        then echo "fail";
        else echo "nop";
        fi
    }
    lsp-did-change
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "workspace/symbol"
[params]
query    = "%s"
' "${kak_session}" "${kak_client}" "${1}" "${2}" "${3}" "${4}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}}

define-command lsp-capabilities -docstring "List available commands for current filetype" %{
    lsp-did-change
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "capabilities"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-open %{
    evaluate-commands %sh{
        if [ $kak_opt_lsp_timestamp -eq $kak_timestamp ]; then
            echo "fail"
        else
            echo "evaluate-commands -draft -no-hooks %{set-option buffer lsp_timestamp %val{timestamp}; execute-keys '%'; set-option buffer lsp_draft %val{selection}}"
        fi
    }
    nop %sh{ (
lsp_draft=$(printf '%s.' "${kak_opt_lsp_draft}" | sed 's/\\/\\\\/g' | sed 's/"/\\"/g' | sed "s/$(printf '\t')/\\\\t/g")
lsp_draft=${lsp_draft%.}
printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/didOpen"
[params]
draft    = """
%s"""
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${lsp_draft}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-close %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/didClose"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-save %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/didSave"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-change-config %{
    echo -debug "Config-change detected:" %opt{lsp_server_configuration}
    nop %sh{
((printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "workspace/didChangeConfiguration"
[params.settings]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}";
eval set -- $kak_opt_lsp_server_configuration
while [ $# -gt 0 ]; do
    key=${1%%=*}
    value=${1#*=}
    quotedkey='"'$(printf %s "$key"|sed -e 's/\\/\\\\/' -e 's/"/\\"/')'"'

    printf '%s = %s\n' "$quotedkey" "$value"

    shift
done
) | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-exit-editor-session -docstring "Shutdown language servers associated with current editor session but keep kak-lsp session running" %{
    remove-hooks global lsp
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "exit"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-stop -docstring "Stop kak-lsp session" %{
    remove-hooks global lsp
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "stop"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-formatting -docstring "Format document" %{
    lsp-did-change
    nop %sh{ (printf '
session      = "%s"
client       = "%s"
buffile      = "%s"
filetype     = "%s"
version      = %d
method       = "textDocument/formatting"
[params]
tabSize      = %d
insertSpaces = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null }
}

define-command lsp-formatting-sync -docstring "Format document, blocking Kakoune session until done" %{
    lsp-did-change
    evaluate-commands -no-hooks %sh{
tmp=$(mktemp -q -d -t 'lsp-formatting.XXXXXX' 2>/dev/null || mktemp -q -d)
pipe=${tmp}/fifo
mkfifo ${pipe}

(printf '
session      = "%s"
client       = "%s"
buffile      = "%s"
filetype     = "%s"
version      = %d
fifo         = "%s"
method       = "textDocument/formatting"
[params]
tabSize      = %d
insertSpaces = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${pipe} "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null

cat ${pipe}
rm -rf ${tmp}
}}

# CCLS Extension

define-command ccls-navigate -docstring "Navigate C/C++/ObjectiveC file" -params 1 %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "$ccls/navigate"
[params]
direction = "%s"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-vars -docstring "ccls-vars: Find instances of symbol at point." %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "$ccls/vars"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-inheritance -params 1..2 -docstring "ccls-inheritance <derived|base> [levels]: Find base- or derived classes of symbol at point." %{
    lsp-did-change
    nop %sh{
        derived="false"
        if [ "$1" = "derived" ]; then
            derived="true"
        fi
        levels="${2:-null}"
        (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "$ccls/inheritance"
[params]
derived   = %s
levels    = %d
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$derived" "$levels" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-call -params 1 -docstring "ccls-call <caller|callee>: Find callers or callees of symbol at point." %{
    lsp-did-change
    nop %sh{
        callee="false"
        if [ "$1" = "callee" ]; then
            callee="true"
        fi
        (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "$ccls/call"
[params]
callee    = %s
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$callee" ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-member -params 1 -docstring "ccls-member <vars|types|functions>: Find member variables/types/functions of symbol at point." %{
    lsp-did-change
    nop %sh{
        kind=0
        case "$1" in
            *var*) kind=1;;
            *type*) kind=2;;
            *func*) kind=3;;
        esac
        (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "$ccls/member"
[params]
kind     = %d
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" $kind ${kak_cursor_line} ${kak_cursor_column} | ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

### Response handling ###

# Feel free to override these commands in your config if you need to customise response handling.

define-command -hidden lsp-show-hover -params 2 -docstring "Render hover info" %{ evaluate-commands %sh{
    content=$2

    if [ $kak_opt_lsp_hover_max_lines -gt 0 ]; then
        content=$(printf %s "$2" | head -n $kak_opt_lsp_hover_max_lines)
    fi

    content=$(printf %s "$content" | sed s/\'/\'\'/g)

    case $kak_opt_lsp_hover_anchor in
        true) printf "info -anchor %%arg{1} '%s'" "$content";;
        *)    printf "info '%s'" "$content";;
    esac
}}

define-command -hidden lsp-show-error -params 1 -docstring "Render error" %{
    echo -debug "kak-lsp:" %arg{1}
    info %arg{1}
}

define-command -hidden lsp-show-diagnostics -params 2 -docstring "Render diagnostics" %{
    evaluate-commands -try-client %opt[toolsclient] %{
        edit! -scratch *diagnostics*
        cd %arg{1}
        try %{ set-option buffer working_folder %sh{pwd} }
        set-option buffer filetype make
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command -hidden lsp-show-references -params 2 -docstring "Render references" %{
    evaluate-commands -try-client %opt[toolsclient] %{
        edit! -scratch *references*
        cd %arg{1}
        try %{ set-option buffer working_folder %sh{pwd} }
        set-option buffer filetype grep
        set-option buffer grep_current_line 0
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command -hidden lsp-show-document-symbol -params 2 -docstring "Render document symbols" %{
    evaluate-commands -try-client %opt[toolsclient] %{
        edit! -scratch *symbols*
        cd %arg{1}
        try %{ set-option buffer working_folder %sh{pwd} }
        set-option buffer filetype grep
        set-option buffer grep_current_line 0
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command -hidden lsp-next-match -params 1 -docstring %{
    buffile
    Jump to next match in grep filetype buffile
} %{
    evaluate-commands -try-client %opt{jumpclient} %{
        buffer %arg{1}
        execute-keys "ge %opt{grep_current_line}g<a-l> /^[^:]+:\d+:<ret>"
        grep-jump
    }
    try %{ evaluate-commands -client %opt{toolsclient} %{ execute-keys gg %opt{grep_current_line}g } }
}

define-command -hidden lsp-previous-match -params 1 -docstring %{
    buffile
    Jump to previous match in grep filetype buffile
} %{
    evaluate-commands -try-client %opt{jumpclient} %{
        buffer %arg{1}
        execute-keys "ge %opt{grep_current_line}g<a-h> <a-/>^[^:]+:\d+:<ret>"
        grep-jump
    }
    try %{ evaluate-commands -client %opt{toolsclient} %{ execute-keys gg %opt{grep_current_line}g } }
}

define-command -hidden lsp-update-workspace-symbol -params 2 -docstring "Update workspace symbols buffer" %{
    cd %arg{1}
    try %{ set-option buffer working_folder %sh{pwd} }
    execute-keys '<a-;>%<a-;>d'
    set-register '"' %arg{2}
    execute-keys '<a-;>P<a-;>gg'
}

define-command -hidden lsp-show-workspace-symbol -params 2 -docstring "Render workspace symbols" %{
    evaluate-commands %sh{
        if [ "${kak_buffile}" = "*symbols*" ];
        then echo 'lsp-update-workspace-symbol %arg{1} %arg{2}';
        else echo 'lsp-show-document-symbol %arg{1} %arg{2}';
        fi
    }
}

define-command -hidden lsp-show-signature-help -params 2 -docstring "Render signature help" %{
    echo %arg{2}
}

define-command -hidden lsp-insert-after-selection -params 1 -docstring %{
    Insert content after current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    declare-option -hidden str lsp_text_edit_tmp %sh{ mktemp }
    declare-option -hidden str lsp_text_edit_content %arg{1}
    execute-keys %sh{
        printf "%s" "$kak_opt_lsp_text_edit_content" > $kak_opt_lsp_text_edit_tmp
        printf "<a-!>cat %s<ret>" $kak_opt_lsp_text_edit_tmp
    }
    nop %sh{ rm $kak_opt_lsp_text_edit_tmp }
}

define-command -hidden lsp-insert-before-selection -params 1 -docstring %{
    Insert content before current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    declare-option -hidden str lsp_text_edit_tmp %sh{ mktemp }
    declare-option -hidden str lsp_text_edit_content %arg{1}
    execute-keys %sh{
        printf "%s" "$kak_opt_lsp_text_edit_content" > $kak_opt_lsp_text_edit_tmp
        printf "!cat %s<ret>" $kak_opt_lsp_text_edit_tmp
    }
    nop %sh{ rm $kak_opt_lsp_text_edit_tmp }
}

define-command -hidden lsp-replace-selection -params 1 -docstring %{
    Replace content of current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    declare-option -hidden str lsp_text_edit_tmp %sh{ mktemp }
    declare-option -hidden str lsp_text_edit_content %arg{1}
    execute-keys %sh{
        printf "%s" "$kak_opt_lsp_text_edit_content" > $kak_opt_lsp_text_edit_tmp
        printf "|cat %s<ret>" $kak_opt_lsp_text_edit_tmp
    }
    nop %sh{ rm $kak_opt_lsp_text_edit_tmp }
}

define-command -hidden lsp-handle-progress -params 4 -docstring %{
  lsp-handle-progress <title> <message> <percentage> <done>
  Handle progress messages sent from the language server. Override to handle this.
} %{ nop }

### Handling requests from server ###

define-command -hidden lsp-get-server-initialization-options -params 1 -docstring %{
    lsp-get-server-initialization-options <fifo>
    Format lsp_server_initialization_options as TOML and write to the given <fifo> path.
} %{
    nop %sh{
(eval set -- $kak_opt_lsp_server_initialization_options
while [ $# -gt 0 ]; do
    key=${1%%=*}
    value=${1#*=}
    quotedkey='"'$(printf %s "$key"|sed -e 's/\\/\\\\/' -e 's/"/\\"/')'"'

    printf '%s = %s\n' "$quotedkey" "$value"

    shift
done
) > $1 }
}

### Other commands ###

define-command lsp-find-error -params 0..2 -docstring "lsp-find-error [--previous] [--include-warnings]
Jump to the next or previous diagnostic error" %{
    evaluate-commands %sh{
        previous=false
        errorCompare="DiagnosticError"
        if [ "$1" = "--previous" ]; then
            previous=true
            shift
        fi
        if [ "$1" = "--include-warnings" ]; then
            errorCompare="Diagnostic"
        fi
        #expand quoting, stores option in $@
        eval "set -- ${kak_opt_lsp_errors}"

        first=""
        current=""
        prev=""
        selection=""
        for e in "$@"; do
            if [ -z "${e##*${errorCompare}*}" ]; then # e contains errorCompare
                e=${e%,*}
                line=${e%.*}
                column=${e#*.}
                if [ $line -eq $kak_cursor_line ] && [ $column -eq $kak_cursor_column ]; then
                    continue #do not return the current location
                fi
                current="$line $column"
                if [ -z "$first" ]; then
                    first="$current"
                fi
                if [ $line -gt $kak_cursor_line ] || { [ $line -eq $kak_cursor_line ] && [ $column -gt $kak_cursor_column ]; }; then
                    #after the cursor
                    if $previous; then
                        selection="$prev"
                    else
                        selection="$current"
                    fi
                    if [ ! -z "$selection" ]; then
                        # if a selection is found
                        break
                    fi
                else
                    prev="$current"
                fi
            fi
        done
        if [ -z "$first" ]; then
            # if nothing found
            echo "echo -markup '{Error}No errors found'"
        fi
        if [ -z "$selection" ]; then #if nothing found past the cursor
            if $previous; then
                selection="$current"
            else
                selection="$first"
            fi
        fi
        printf "edit %b %b" "$kak_buffile" "$selection"
    }
}

define-command lsp-workspace-symbol -params 1 -docstring "Open buffer with a list of project-wide symbols matching the query" %{ lsp-workspace-symbol-buffer %val{buffile} %opt{filetype} %val{timestamp} %arg{1} }

define-command lsp-workspace-symbol-incr -docstring "Open buffer with an incrementally updated list of project-wide symbols matching the query" %{
    declare-option -hidden str lsp_ws_buffile %val{buffile}
    declare-option -hidden str lsp_ws_filetype %opt{filetype}
    declare-option -hidden int lsp_ws_timestamp %val{timestamp}
    declare-option -hidden str lsp_ws_query
    edit! -scratch *symbols*
    set-option buffer filetype grep
    set-option buffer grep_current_line 0
    prompt -on-change %{ try %{
        # lsp-show-workspace-symbol triggers on-change somehow which causes inifinite loop
        # the following check prevents it
        evaluate-commands %sh{
            if [ "${kak_opt_lsp_ws_query}" = "${kak_text}" ];
            then echo 'fail';
            else echo 'set current lsp_ws_query %val{text}';
            fi
        }
        lsp-workspace-symbol-buffer %opt{lsp_ws_buffile} %opt{lsp_ws_filetype} %opt{lsp_ws_timestamp} %val{text}
    }} -on-abort %{execute-keys ga} 'Query: ' nop
}

### Hooks and highlighters ###

define-command lsp-inline-diagnostics-enable -params 1 -docstring "lsp-inline-diagnostics-enable <scope>: Enable inline diagnostics highlighting for <scope>" %{
    add-highlighter "%arg{1}/lsp_errors" ranges lsp_errors
}

define-command lsp-inline-diagnostics-disable -params 1 -docstring "lsp-inline-diagnostics-disable <scope>: Disable inline diagnostics highlighting for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_errors"
}

define-command lsp-diagnostic-lines-enable -params 1 -docstring "lsp-diagnostic-lines-enable <scope>: Show flags on lines with diagnostics in <scope>" %{
    add-highlighter "%arg{1}/lsp_error_lines" flag-lines LineFlagErrors lsp_error_lines
}

define-command lsp-diagnostic-lines-disable -params 1 -docstring "lsp-diagnostic-lines-disable <scope>: Hide flags on lines with diagnostics in <scope>"  %{
    remove-highlighter "%arg{1}/lsp_error_lines"
}

define-command lsp-auto-hover-enable -docstring "Enable auto-requesting hover info for current position" %{
    hook -group lsp-auto-hover global NormalIdle .* %{
        lsp-hover
    }
}

define-command lsp-auto-hover-disable -docstring "Disable auto-requesting hover info for current position" %{
    remove-hooks global lsp-auto-hover
}

define-command lsp-auto-hover-insert-mode-enable -docstring "Enable auto-requesting hover info for current function in insert mode" %{
    hook -group lsp-auto-hover-insert-mode global InsertIdle .* %{ try %{ evaluate-commands -draft %{
        evaluate-commands %opt{lsp_hover_insert_mode_trigger}
        lsp-hover
    }}}
}

define-command lsp-auto-hover-insert-mode-disable -docstring "Disable auto-requesting hover info for current function in insert mode" %{
    remove-hooks global lsp-auto-hover-insert-mode
}

define-command lsp-auto-signature-help-enable -docstring "Enable auto-requesting signature help in insert mode" %{
    hook -group lsp-auto-signature-help global InsertIdle .* lsp-signature-help
}

define-command lsp-auto-signature-help-disable -docstring "Disable auto-requesting signature help in insert mode" %{
    remove-hooks global lsp-auto-signature-help
}

define-command lsp-stop-on-exit-enable -docstring "End kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-stop
}

define-command lsp-stop-on-exit-disable -docstring "Don't end kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-exit-editor-session
}

### lsp-* commands as subcommands of lsp command ###

define-command lsp -params 1.. -shell-script-candidates %{
    for cmd in start hover definition references signature-help diagnostics document-symbol\
    workspace-symbol workspace-symbol-incr rename rename-prompt\
    capabilities stop formatting formatting-sync highlight-references\
    inline-diagnostics-enable inline-diagnostics-disable\
    diagnostic-lines-enable diagnostics-lines-disable auto-hover-enable auto-hover-disable\
    auto-hover-insert-mode-enable auto-hover-insert-mode-disable auto-signature-help-enable\
    auto-signature-help-disable stop-on-exit-enable stop-on-exit-disable find-error;
        do echo $cmd;
    done
} %{ evaluate-commands "lsp-%arg{1}" }


### User mode ###

declare-user-mode lsp
map global lsp a '<esc>: lsp-code-actions<ret>'           -docstring 'show code actions for current position'
map global lsp c '<esc>: lsp-capabilities<ret>'           -docstring 'show language server capabilities'
map global lsp d '<esc>: lsp-definition<ret>'             -docstring 'go to definition'
map global lsp e '<esc>: lsp-diagnostics<ret>'            -docstring 'list project errors and warnings'
map global lsp f '<esc>: lsp-formatting<ret>'             -docstring 'format buffer'
map global lsp h '<esc>: lsp-hover<ret>'                  -docstring 'show info for current position'
map global lsp r '<esc>: lsp-references<ret>'             -docstring 'list symbol references'
map global lsp s '<esc>: lsp-signature-help<ret>'         -docstring 'show function signature help'
map global lsp S '<esc>: lsp-document-symbol<ret>'        -docstring 'list document symbols'
map global lsp o '<esc>: lsp-workspace-symbol-incr<ret>'  -docstring 'search project symbols'
map global lsp n '<esc>: lsp-find-error<ret>'             -docstring 'find next error'
map global lsp p '<esc>: lsp-find-error --previous<ret>'  -docstring 'find previous error'
map global lsp <&> '<esc>: lsp-highlight-references<ret>' -docstring 'lsp-highlight-references'

### Default integration ###

define-command -hidden lsp-enable -docstring "Default integration with kak-lsp" %{
    set-option global completers option=lsp_completions %opt{completers}
    add-highlighter global/cquery_semhl ranges cquery_semhl
    add-highlighter global/lsp_references ranges lsp_references
    lsp-inline-diagnostics-enable global
    lsp-diagnostic-lines-enable global

    map global goto d '<esc>: lsp-definition<ret>' -docstring 'definition'
    map global goto r '<esc>: lsp-references<ret>' -docstring 'references'

    hook -group lsp global BufCreate .* %{
        lsp-did-open
        lsp-did-change-config
    }
    hook -group lsp global BufClose .* lsp-did-close
    hook -group lsp global BufWritePost .* lsp-did-save
    hook -group lsp global BufSetOption lsp_server_configuration=.* lsp-did-change-config
    hook -group lsp global InsertIdle .* lsp-completion
    hook -group lsp global NormalIdle .* %{
        lsp-did-change
        %sh{if $kak_opt_lsp_auto_highlight_references; then echo "lsp-highlight-references"; else echo "nop"; fi}
    }
    hook -group lsp global KakEnd .* lsp-exit
}

define-command lsp-enable-window -docstring "Default integration with kak-lsp in the window scope" %{
    set-option window completers option=lsp_completions %opt{completers}

    add-highlighter window/cquery_semhl ranges cquery_semhl
    add-highlighter window/lsp_references ranges lsp_references

    lsp-inline-diagnostics-enable window
    lsp-diagnostic-lines-enable window

    map window goto d '<esc>: lsp-definition<ret>' -docstring 'definition'
    map window goto r '<esc>: lsp-references<ret>' -docstring 'references'

    hook -group lsp window WinClose .* lsp-did-close
    hook -group lsp window BufWritePost .* lsp-did-save
    hook -group lsp window WinSetOption lsp_server_configuration=.* lsp-did-change-config
    hook -group lsp window InsertIdle .* lsp-completion
    hook -group lsp window NormalIdle .* %{
        lsp-did-change
        %sh{if $kak_opt_lsp_auto_highlight_references; then echo "lsp-highlight-references"; else echo "nop"; fi}
    }

    lsp-did-open
    lsp-did-change-config
}

lsp-stop-on-exit-enable
