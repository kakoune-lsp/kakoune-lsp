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
         exec p
     }
}

def -hidden lsp-show-references -params 2 -docstring "Render references" %{
     eval -try-client %opt[toolsclient] %{
         edit! -scratch *references*
         cd %arg{1}
         try %{ set buffer working_folder %sh{pwd} }
         set buffer filetype grep
         set-register '"' %arg{2}
         exec p
     }
}

def -hidden lsp-show-document-symbol -params 2 -docstring "Render document symbols" %{
     eval -try-client %opt[toolsclient] %{
         edit! -scratch *symbols*
         cd %arg{1}
         try %{ set buffer working_folder %sh{pwd} }
         set buffer filetype grep
         set-register '"' %arg{2}
         exec p
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


def lsp -params 1.. %sh{
    if [ $kak_version \< "v2018.09.04-128-g5bdcfab0" ];
    then echo "-shell-candidates";
    else echo "-shell-script-candidates";
    fi
} %{
    for cmd in start hover definition references signature-help diagnostics document-symbol\
    capabilities stop formatting highlight-references inline-diagnostics-enable inline-diagnostics-disable\
    diagnostic-lines-enable diagnostics-lines-disable auto-hover-enable auto-hover-disable\
    auto-hover-insert-mode-enable auto-hover-insert-mode-disable auto-signature-help-enable\
    auto-signature-help-disable stop-on-exit-enable stop-on-exit-disable;
        do echo $cmd;
    done
} %{ eval "lsp-%arg{1}" }


declare-user-mode lsp
map global lsp c '<esc>:lsp-capabilities<ret>'    -docstring 'capabilities'
map global lsp d '<esc>:lsp-definition<ret>'      -docstring 'definition'
map global lsp D '<esc>:lsp-diagnostics<ret>'     -docstring 'diagnostics'
map global lsp f '<esc>:lsp-formatting<ret>'      -docstring 'formatting'
map global lsp h '<esc>:lsp-hover<ret>'           -docstring 'hover'
map global lsp r '<esc>:lsp-references<ret>'      -docstring 'references'
map global lsp s '<esc>:lsp-signature-help<ret>'  -docstring 'signature help'
map global lsp S '<esc>:lsp-document-symbol<ret>' -docstring 'document symbols'


lsp-stop-on-exit-enable
lsp-enable
