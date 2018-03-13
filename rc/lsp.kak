decl str lsp_cmd 'nc localhost 31337'
decl -hidden completions lsp_completions

def lsp-did-change %{
    decl -hidden str lsp_draft %sh{ mktemp }
    eval -no-hooks write %opt{lsp_draft}
    nop %sh{ printf '{
            "meta": {
                "session" : "%s",
                "client"  : "%s",
                "buffile" : "%s"
            },
            "call": {
                "jsonrpc" : "2.0",
                "method"  : "textDocument/didChange",
                "params"  : {
                    "textDocument" : {
                        "uri"      : "file://%s",
                        "version"  : %d,
                        "draft"    : "%s"
                    }
                }
            }
        }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" "${kak_timestamp}" ${kak_opt_lsp_draft} | ${kak_opt_lsp_cmd} }
}

def lsp-completion %{
    decl -hidden str lsp_completion_offset
    eval -draft %{ try %{
        execute-keys <esc><a-h>s\w+.\z<ret>
        set buffer lsp_completion_offset %sh{echo $(expr ${#kak_selection} - 1)}
    } catch %{
        set buffer lsp_completion_offset "0"
    }}
    nop %sh{ printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/completion",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s",
                    "version"  : %d
                },
                "position": {
                    "line"     : %d,
                    "character": %d
                },
                "completion" : {
                    "offset" : %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" "${kak_timestamp}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) ${kak_opt_lsp_completion_offset} | ${kak_opt_lsp_cmd} }
}

def lsp-hover %{
    nop %sh{ printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/hover",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s"
                },
                "position": {
                    "line"     : %d,
                    "character": %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) | ${kak_opt_lsp_cmd} }
}

def lsp-definition %{
    nop %sh{ printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/definition",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s"
                },
                "position": {
                    "line"     : %d,
                    "character": %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" $(expr ${kak_cursor_line} - 1) $(expr ${kak_cursor_column} - 1) | ${kak_opt_lsp_cmd} }
}

def lsp-did-open %{
    nop %sh{printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/didOpen",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s",
                    "version"  : %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}}
}

def lsp-did-close %{
    nop %sh{printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/didClose",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s",
                    "version"  : %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}}
}

def lsp-did-save %{
    nop %sh{printf '{
        "meta": {
            "session" : "%s",
            "client"  : "%s",
            "buffile" : "%s"
        },
        "call" : {
            "jsonrpc" : "2.0",
            "method"  : "textDocument/didSave",
            "params"  : {
                "textDocument" : {
                    "uri"      : "file://%s",
                    "version"  : %d
                }
            }
        }
    }\n' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_buffile}" "${kak_timestamp}" | ${kak_opt_lsp_cmd}}
}

def lsp-enable %{
    set buffer completers "option=lsp_completions:%opt{completers}"

    lsp-did-open

    map buffer goto d '<esc>:lsp-definition<ret>' -docstring 'definition'

    hook -group lsp buffer BufClose .* lsp-did-close
    hook -group lsp buffer BufWritePost .* lsp-did-save
    hook -group lsp window InsertIdle .* %{
        lsp-did-change
        lsp-completion
    }
    hook -group lsp window NormalIdle .* %{
        lsp-hover
    }
}

