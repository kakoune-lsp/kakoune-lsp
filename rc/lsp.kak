decl str lsp_cmd 'nc {{ip}} {{port}} -c'
decl -hidden completions lsp_completions
decl -hidden range-specs lsp_errors

decl -hidden range-specs cquery_semhl
decl -hidden str lsp_draft
decl -hidden int lsp_timestamp -1

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

def -hidden lsp-completion %{
    decl -hidden str lsp_completion_offset
    eval -draft %{ try %{
        execute-keys <esc><a-h>s\w+.\z<ret>
        set window lsp_completion_offset %sh{echo $(expr ${#kak_selection} - 1)}
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) ${kak_opt_lsp_completion_offset} | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-hover %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/hover"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-definition %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/definition"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def lsp-references %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
version   = %d
method    = "textDocument/references"
[params.position]
line      = %d
character = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
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

def -hidden lsp-exit %{
    nop %sh{ (printf '
session = "%s"
client  = "%s"
buffile = "%s"
version = %d
method  = "exit"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & }
}

def -hidden lsp-enable %{
    set global completers "option=lsp_completions:%opt{completers}"
    add-highlighter global/ ranges lsp_errors
    add-highlighter global/ ranges cquery_semhl

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
    hook -group lsp global NormalIdle .* %{
        lsp-did-change
        lsp-hover
    }
    hook -group lsp global KakEnd .* %{
        lsp-exit
    }
}

lsp-enable
