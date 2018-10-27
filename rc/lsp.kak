# faces used by inline diagnostics
set-face global DiagnosticError red
set-face global DiagnosticWarning yellow
# Line flags for errors and warnings both use this face
set-face global LineFlagErrors red
set-face global Reference MatchingChar


decl str lsp_cmd '{{cmd}} --request {{args}}'


# set to true to display hover info anchored to hovered position
decl bool lsp_hover_anchor false
# completions request is sent only when this expression doesn't fail
# by default it ensures that preceding character is not a whitespace
decl str lsp_completion_trigger %{execute-keys '<a-h><a-k>\S.\z<ret>'}
# if hover in insert mode is enabled then request is made only when this expression doesn't fail and
# for position at which it moves cursor; by default it ensures that cursor is after opening parens
# and then moves cursor to opening parens to request hover info for current function; note that it
# doesn't handle well nested function calls
decl str lsp_hover_insert_mode_trigger %{execute-keys '<a-f>(s\A[^)]+\z<ret>'}
# formatting: size of a tab in spaces
decl int lsp_tab_size 4
# formatting: prefer spaces over tabs
decl bool lsp_insert_spaces true
# set to true to automatically highlight references with Reference face
decl bool lsp_auto_highlight_references false
# number of diagnostics published for current buffer
decl int lsp_diagnostic_count 0

# configuration to send in DidChangeNotification messages
decl str-to-str-map lsp_server_configuration

decl str lsp_diagnostic_line_error_sign '*'
decl str lsp_diagnostic_line_warning_sign '!'


decl -hidden completions lsp_completions
decl -hidden range-specs lsp_errors
decl -hidden line-specs lsp_error_lines
decl -hidden range-specs cquery_semhl
decl -hidden str lsp_draft
decl -hidden int lsp_timestamp -1
decl -hidden range-specs lsp_references

# commands to make kak-lsp requests

def lsp-start -docstring "Start kak-lsp session" %{ nop %sh{ ({{cmd}} {{args}}) > /dev/null 2>&1 < /dev/null & } }

def -hidden lsp-did-change -docstring "Notify language server about buffer change" %{ try %{
    evaluate-commands %sh{
        if [ $kak_opt_lsp_timestamp -eq $kak_timestamp ]; then
            echo "fail"
        else
            echo "eval -draft -no-hooks %{set buffer lsp_timestamp %val{timestamp}; exec '%'; set buffer lsp_draft %val{selection}}"
        fi
    }
    nop %sh{ (
lsp_draft=$(printf '%s.' "${kak_opt_lsp_draft}" | sed 's/\\/\\\\/g' | sed 's/"/\\"/g' | sed "s/$(printf '\t')/\\\\t/g")
lsp_draft=${lsp_draft%.}
printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/didChange"
[params]
draft   = """
%s"""
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" "${lsp_draft}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null }
}}

def -hidden lsp-completion -docstring "Request completions for the main cursor position" %{
lsp-did-change
try %{
    # fail if preceding character is a whitespace
    eval -draft %opt{lsp_completion_trigger}

    decl -hidden str lsp_completion_offset

    eval -draft %{ try %{
        execute-keys <esc><a-h>s\$?\w+.\z<ret>
        set window lsp_completion_offset %sh{echo $((${#kak_selection} - 1))}
    } catch %{
        set window lsp_completion_offset "0"
    }}

    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/completion"
[params.position]
line      = %d
character = %d
[params.completion]
offset    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) ${kak_opt_lsp_completion_offset} | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}}

def lsp-hover -docstring "Request hover info for the main cursor position" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/hover"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-definition -docstring "Go to definition" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/definition"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-references -docstring "Open buffer with symbol references" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/references"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-highlight-references -docstring "Highlight symbol references" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/referencesHighlight"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-signature-help -docstring "Request signature help for the main cursor position" %{
    lsp-did-change
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/signatureHelp"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $((${kak_cursor_line} - 1)) $((${kak_cursor_column} - 1)) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-diagnostics -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    lsp-did-change
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/diagnostics"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-document-symbol -docstring "Open buffer with document symbols" %{
    lsp-did-change
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/documentSymbol"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-workspace-symbol-buffer -params 3 -docstring %{
    buffile timestamp query
    Open buffer with a list of project-wide symbols matching the query
    on behalf of the buffile at timestamp
 } %{ try %{
    eval %sh{
        if [ -z "${3}" ];
        then echo "fail";
        else echo "nop";
        fi
    }
    lsp-did-change
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "workspace/symbol"
[params]
query   = "%s"
' "${kak_session}" "${kak_client}" "${1}" "${2}" "${3}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}}

def lsp-capabilities -docstring "List available commands for current filetype" %{
    lsp-did-change
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "capabilities"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-did-open %{
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/didOpen"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-did-close %{
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/didClose"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-did-save %{
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/didSave"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-did-change-config %{
    echo -debug "Config-change detected:" %opt{lsp_server_configuration}
    nop %sh{
((printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "workspace/didChangeConfiguration"
[params.settings]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}";
eval set -- $kak_opt_lsp_server_configuration
while [ $# -gt 0 ]; do
    key=${1%%=*}
    value=${1#*=}
    quotedkey='"'$(printf %s "$key"|sed -e 's/\\/\\\\/' -e 's/"/\\"/')'"'

    printf '%s = %s\n' "$quotedkey" "$value"

    shift
done
) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-exit-editor-session -docstring "Shutdown language servers associated with current editor session but keep kak-lsp session running" %{
    remove-hooks global lsp
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "exit"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-stop -docstring "Stop kak-lsp session" %{
    remove-hooks global lsp
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "stop"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-formatting -docstring "Format document" %{
    lsp-did-change
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/formatting"
[params]
tabSize = %d
insertSpaces = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" "${kak_opt_lsp_tab_size}" "${kak_opt_lsp_insert_spaces}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null }
}

def lsp-formatting-sync -docstring "Format document, blocking Kakoune session until done" %{
    lsp-did-change
    eval -no-hooks %sh{
tmp=$(mktemp -q -d -t 'lsp-formatting.XXXXXX' 2>/dev/null || mktemp -q -d)
pipe=${tmp}/fifo
mkfifo ${pipe}

(printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
fifo    = "%s"
method  = "textDocument/formatting"
[params]
tabSize = %d
insertSpaces = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" ${pipe} "${kak_opt_lsp_tab_size}" "${kak_opt_lsp_insert_spaces}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null

cat ${pipe}
rm -rf ${tmp}
}}

# commands called as kak-lsp responses

def -hidden lsp-show-hover -params 2 -docstring "Render hover info" %{ evaluate-commands %sh{
    case $kak_opt_lsp_hover_anchor in
        true) echo 'info -anchor %arg{1} %arg{2}';;
        *)    echo 'info %arg{2}';;
    esac
}}

def -hidden lsp-show-error -params 1 -docstring "Render error" %{
    echo -debug "kak-lsp:" %arg{1}
    info %arg{1}
}

def -hidden lsp-show-diagnostics -params 2 -docstring "Render diagnostics" %{
    eval -try-client %opt[toolsclient] %{
        edit! -scratch *diagnostics*
        cd %arg{1}
        try %{ set buffer working_folder %sh{pwd} }
        set buffer filetype make
        set-register '"' %arg{2}
        exec Pgg
    }
}

def -hidden lsp-show-references -params 2 -docstring "Render references" %{
    eval -try-client %opt[toolsclient] %{
        edit! -scratch *references*
        cd %arg{1}
        try %{ set buffer working_folder %sh{pwd} }
        set buffer filetype grep
        set-register '"' %arg{2}
        exec Pgg
    }
}

def -hidden lsp-show-document-symbol -params 2 -docstring "Render document symbols" %{
    eval -try-client %opt[toolsclient] %{
        edit! -scratch *symbols*
        cd %arg{1}
        try %{ set buffer working_folder %sh{pwd} }
        set buffer filetype grep
        set-register '"' %arg{2}
        exec Pgg
    }
}

def -hidden lsp-update-workspace-symbol -params 2 -docstring "Update workspace symbols buffer" %{
    cd %arg{1}
    try %{ set buffer working_folder %sh{pwd} }
    exec '<a-;>%<a-;>d'
    set-register '"' %arg{2}
    exec '<a-;>P<a-;>gg'
}

def -hidden lsp-show-workspace-symbol -params 2 -docstring "Render workspace symbols" %{
    eval %sh{
        if [ "${kak_buffile}" = "*symbols*" ];
        then echo 'lsp-update-workspace-symbol %arg{1} %arg{2}';
        else echo 'lsp-show-document-symbol %arg{1} %arg{2}';
        fi
    }
}

def -hidden lsp-show-signature-help -params 2 -docstring "Render signature help" %{
    echo %arg{2}
}

def -hidden lsp-insert-after-selection -params 1 -docstring %{
    Insert content after current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    decl -hidden str lsp_text_edit_tmp %sh{ mktemp }
    decl -hidden str lsp_text_edit_content %arg{1}
    exec %sh{
        printf "%s" "$kak_opt_lsp_text_edit_content" > $kak_opt_lsp_text_edit_tmp 
        printf "<a-!>cat %s<ret>" $kak_opt_lsp_text_edit_tmp
    }
    nop %sh{ rm $kak_opt_lsp_text_edit_tmp }
}

def -hidden lsp-replace-selection -params 1 -docstring %{
    Replace content of current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    decl -hidden str lsp_text_edit_tmp %sh{ mktemp }
    decl -hidden str lsp_text_edit_content %arg{1}
    exec %sh{
        printf "%s" "$kak_opt_lsp_text_edit_content" > $kak_opt_lsp_text_edit_tmp 
        printf "|cat %s<ret>" $kak_opt_lsp_text_edit_tmp
    }
    nop %sh{ rm $kak_opt_lsp_text_edit_tmp }
}

# convenient commands to set and remove hooks for common cases

def lsp-inline-diagnostics-enable -docstring "Enable inline diagnostics highlighting" %{
    add-highlighter global/lsp_errors ranges lsp_errors
}

def lsp-inline-diagnostics-disable -docstring "Disable inline diagnostics highlighting"  %{
    remove-highlighter global/lsp_errors
}

def lsp-diagnostic-lines-enable -docstring "Enable diagnostics line flags" %{
    add-highlighter global/lsp_error_lines flag-lines LineFlagErrors lsp_error_lines
}

def lsp-diagnostic-lines-disable -docstring "Disable diagnostics line flags"  %{
    remove-highlighter global/lsp_error_lines
}

def lsp-find-error -params 0..2 -docstring "lsp-find-error [--previous] [--include-warnings]
Jump to the next or previous diagnostic error" %{
    evaluate-commands %sh{
        previous=false
        errorCompare="DiagnosticError"
        if [ $1 = "--previous" ]; then
            previous=true
            shift
        fi
        if [ $1 = "--include-warnings" ]; then
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

def lsp-auto-hover-enable -docstring "Enable auto-requesting hover info for current position" %{
    hook -group lsp-auto-hover global NormalIdle .* %{
        lsp-hover
    }
}

def lsp-auto-hover-disable -docstring "Disable auto-requesting hover info for current position" %{
    remove-hooks global lsp-auto-hover
}

def lsp-auto-hover-insert-mode-enable -docstring "Enable auto-requesting hover info for current function in insert mode" %{
    hook -group lsp-auto-hover-insert-mode global InsertIdle .* %{ try %{ eval -draft %{
        eval %opt{lsp_hover_insert_mode_trigger}
        lsp-hover
    }}}
}

def lsp-auto-hover-insert-mode-disable -docstring "Disable auto-requesting hover info for current function in insert mode" %{
    remove-hooks global lsp-auto-hover-insert-mode
}

def lsp-auto-signature-help-enable -docstring "Enable auto-requesting signature help in insert mode" %{
    hook -group lsp-auto-signature-help global InsertIdle .* lsp-signature-help
}

def lsp-auto-signature-help-disable -docstring "Disable auto-requesting signature help in insert mode" %{
    remove-hooks global lsp-auto-signature-help
}

def lsp-stop-on-exit-enable -docstring "End kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-stop
}

def lsp-stop-on-exit-disable -docstring "Don't end kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-exit-editor-session
}


def lsp-workspace-symbol -params 1 -docstring "Open buffer with a list of project-wide symbols matching the query" %{ lsp-workspace-symbol-buffer %val{buffile} %val{timestamp} %arg{1} }

def lsp-workspace-symbol-incr -docstring "Open buffer with an incrementally updated list of project-wide symbols matching the query" %{
    decl -hidden str lsp_ws_buffile %val{buffile}
    decl -hidden int lsp_ws_timestamp %val{timestamp}
    decl -hidden str lsp_ws_query
    edit! -scratch *symbols*
    set buffer filetype grep
    prompt -on-change %{ try %{
        # lsp-show-workspace-symbol triggers on-change somehow which causes inifinite loop
        # the following check prevents it
        eval %sh{
            if [ "${kak_opt_lsp_ws_query}" = "${kak_text}" ];
            then echo 'fail';
            else echo 'set current lsp_ws_query %val{text}';
            fi
        }
        lsp-workspace-symbol-buffer %opt{lsp_ws_buffile} %opt{lsp_ws_timestamp} %val{text}
    }} -on-abort %{exec ga} 'Query: ' nop
}


# lsp-* commands as subcommands of lsp command

def lsp -params 1.. %sh{
    if [ $kak_version \< "v2018.09.04-128-g5bdcfab0" ];
    then echo "-shell-candidates";
    else echo "-shell-script-candidates";
    fi
} %{
    for cmd in start hover definition references signature-help diagnostics document-symbol\
    workspace-symbol workspace-symbol-incr\
    capabilities stop formatting formatting-sync highlight-references\
    inline-diagnostics-enable inline-diagnostics-disable\
    diagnostic-lines-enable diagnostics-lines-disable auto-hover-enable auto-hover-disable\
    auto-hover-insert-mode-enable auto-hover-insert-mode-disable auto-signature-help-enable\
    auto-signature-help-disable stop-on-exit-enable stop-on-exit-disable find-error;
        do echo $cmd;
    done
} %{ eval "lsp-%arg{1}" }


# user mode

declare-user-mode lsp
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


# init

def -hidden lsp-enable -docstring "Default integration with kak-lsp" %{
    set global completers option=lsp_completions %opt{completers}
    add-highlighter global/cquery_semhl ranges cquery_semhl
    add-highlighter global/lsp_references ranges lsp_references
    lsp-inline-diagnostics-enable
    lsp-diagnostic-lines-enable

    map global goto d '<esc>:lsp-definition<ret>' -docstring 'definition'
    map global goto r '<esc>:lsp-references<ret>' -docstring 'references'

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

lsp-stop-on-exit-enable
lsp-enable
