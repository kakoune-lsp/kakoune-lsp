set-face global DiagnosticError red
set-face global DiagnosticWarning yellow

decl str lsp_cmd '{{cmd}} --request {{args}}'
decl bool lsp_hover_anchor false
decl str lsp_completion_trigger '<a-h><a-k>\S.\z<ret>'
decl -hidden completions lsp_completions
decl -hidden range-specs lsp_errors

decl -hidden range-specs cquery_semhl
decl -hidden str lsp_draft
decl -hidden int lsp_timestamp -1

def lsp-start -docstring "Start kak-lsp session" %{ nop %sh{ ({{cmd}} {{args}}) > /dev/null 2>&1 < /dev/null & } }

def -hidden lsp-did-change %{ try %{
    %sh{
        if [ $kak_opt_lsp_timestamp -eq $kak_timestamp ]; then
            echo "fail"
        else
            draft=$(mktemp)
            printf 'eval -no-hooks %%{write "%s"; set buffer lsp_timestamp %d; set buffer lsp_draft "%s"}\n' "${draft}" "${kak_timestamp}" "${draft}"
        fi
    }
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/didChange"
[params]
draft   = "%s"
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" "${kak_opt_lsp_draft}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}}

def -hidden lsp-completion -docstring "Request completions for the main cursor position" %{ try %{
    # fail if preceding character is a whitespace
    execute-keys -draft %opt{lsp_completion_trigger}

    decl -hidden str lsp_completion_offset

    eval -draft %{ try %{
        execute-keys <esc><a-h>s\w+.\z<ret>
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

def lsp-diagnostics -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "textDocument/diagnostics"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-capabilities -docstring "List available commands for current filetype" %{
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

def -hidden lsp-exit-editor-session %{
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

def lsp-inline-diagnostics-enable -docstring "Enable inline diagnostics highlighting" %{
    add-highlighter global/ ranges lsp_errors
}

def lsp-inline-diagnostics-disable -docstring "Disable inline diagnostics highlighting"  %{
    remove-highlighter global/hlranges_lsp_errors
}

def lsp-auto-hover-enable -docstring "Enable auto-requesting hover info for current position" %{
    hook -group lsp-auto-hover global NormalIdle .* %{
        lsp-hover
    }
}

def lsp-auto-hover-disable -docstring "Disable auto-requesting hover info for current position" %{
    remove-hooks global lsp-auto-hover
}

def -hidden lsp-show-hover -params 2 -docstring "Command responsible for rendering hover info" %{ %sh{
    case $kak_opt_lsp_hover_anchor in
        true) echo 'info -anchor %arg{1} %arg{2}';;
        *)    echo 'info %arg{2}';;
    esac
}}

def lsp-stop-on-exit-enable -docstring "End kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-stop
}

def lsp-stop-on-exit-disable -docstring "Don't end kak-lsp session on Kakoune session end" %{
    alias global lsp-exit lsp-exit-editor-session
}

lsp-stop-on-exit-enable

def -hidden lsp-enable -docstring "Default integration with kak-lsp" %{
    set global completers "option=lsp_completions:%opt{completers}"
    add-highlighter global/ ranges cquery_semhl
    lsp-inline-diagnostics-enable

    map global goto d '<esc>:lsp-definition<ret>' -docstring 'definition'

    hook -group lsp global BufCreate .* %{
        lsp-did-open
    }
    hook -group lsp global BufClose .* lsp-did-close
    hook -group lsp global BufWritePost .* lsp-did-save
    hook -group lsp global InsertIdle .* %{
        lsp-did-change
        lsp-completion
    }
    hook -group lsp global NormalIdle .* lsp-did-change
    hook -group lsp global KakEnd .* lsp-exit
}

lsp-enable
