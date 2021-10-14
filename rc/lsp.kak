### Options and faces ###

# Feel free to update path and arguments according to your setup when sourcing lsp.kak directly.
# Sourcing via `kak-lsp --kakoune` does it automatically.
declare-option -docstring "Command with which lsp is run" str lsp_cmd "kak-lsp -s %val{session}"

# Faces

# Faces used by inline diagnostics.
set-face global DiagnosticError red
set-face global DiagnosticHint default
set-face global DiagnosticInfo default
set-face global DiagnosticWarning yellow
# Faces used by inlay diagnostics.
set-face global InlayDiagnosticError DiagnosticError
set-face global InlayDiagnosticHint DiagnosticHint
set-face global InlayDiagnosticInfo DiagnosticInfo
set-face global InlayDiagnosticWarning DiagnosticWarning
# Faces used by line flags
set-face global LineFlagError red
set-face global LineFlagHint default
set-face global LineFlagInfo default
set-face global LineFlagWarning yellow
# Face for highlighting references.
set-face global Reference MatchingChar
set-face global ReferenceBind +u@Reference
# Face for inlay hints.
set-face global InlayHint cyan+d

# Options for tuning kak-lsp behaviour.

# Display hover info anchored to the hovered position.
declare-option -docstring "Display hover info anchored to the hovered position" bool lsp_hover_anchor false
# Completions request is sent only when this expression doesn't fail.
# By default, it ensures that preceding character is not a whitespace.
declare-option -docstring "Completion request is sent only when this expression does not fail" str lsp_completion_trigger %{execute-keys '<a-h><a-k>\S.\z<ret>'}
# Kakoune requires completions to point fragment start rather than cursor position.
# This variable provides a way to customise how fragment start is detected.
# By default, it tracks back to the first punctuation or whitespace.
declare-option -docstring "Select from cursor to the start of the term being completed" str lsp_completion_fragment_start %{execute-keys <esc><a-h>s\$?\w+.\z<ret>}
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

declare-option -docstring "Dynamic TOML configuration string. Currently supports
- [language.<filetype>.settings]
" str lsp_config
# Highlight TOML keys in kakrc if they are supported by dynamic configuration.
try %{
    add-highlighter shared/kakrc/code/lsp_keywords regex \[(language\.[a-z_]+\.settings(?:\.[^\]]*)?)\] 1:title
} catch %{
    hook global -once ModuleLoaded kak %{
        add-highlighter shared/kakrc/code/lsp_keywords regex \[(language\.[a-z_]+\.settings(?:\.[^\]]*)?)\] 1:title
    }
}
declare-option -docstring "DEPRECATED, use %opt{lsp_config}. Configuration to send in workspace/didChangeConfiguration messages" str-to-str-map lsp_server_configuration
declare-option -docstring "DEPRECATED, use %opt{lsp_config}. Configuration to send in initializationOptions of Initialize messages." str-to-str-map lsp_server_initialization_options
# Line flags for inline diagnostics.
declare-option -docstring "Character to signal an error in the gutter" str lsp_diagnostic_line_error_sign '*'
declare-option -docstring "Character to signal a hint in the gutter" str lsp_diagnostic_line_hint_sign '-'
declare-option -docstring "Character to signal an info in the gutter" str lsp_diagnostic_line_info_sign 'i'
declare-option -docstring "Character to signal a warning in the gutter" str lsp_diagnostic_line_warning_sign '!'
# Visual settings for inlay diagnostics
declare-option -docstring "Character to represent a single inlay diagnostic of many on a line. May not contain '|'" str lsp_inlay_diagnostic_sign '■'
declare-option -docstring "Character(s) to separate the actual line contents from the inlay diagnostics. May not contain '|'" str lsp_inlay_diagnostic_gap '     '
# Another good default:
# set-option global lsp_diagnostic_line_error_sign '▓'
# set-option global lsp_diagnostic_line_warning_sign '▒'

# This is used to render lsp-hover responses.
# By default it shows both hover info and diagnostics.
# The string is `eval`ed to produce the content to display, so anything sent to stdout will
# show up in the info box.
declare-option -docstring "Format hover info" str lsp_show_hover_format %{
printf "%s\n\n" "${lsp_info}"
if [ -n "${lsp_diagnostics}" ]; then
    printf "{+b@InfoDefault}Diagnostics:{InfoDefault}\n%s" "${lsp_diagnostics}"
fi
}
# If you want to see only hover info, try
# set-option global lsp_show_hover_format 'printf %s "${lsp_info}"'

declare-option -docstring %{Defines location patterns for lsp-next-location and lsp-previous-location.
Default locations look like "file:line[:column][:message]"

Capture groups must be:
    1: filename
    2: line number
    3: optional column
    4: optional message
} regex lsp_location_format ^\h*([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))

# Callback functions. Override these to tune kak-lsp's behavior.

define-command -hidden lsp-show-code-actions -params 1.. -docstring "Called when code actions are available for the main cursor position" %{
    set-option buffer lsp_modeline "💡 "
}

define-command -hidden lsp-hide-code-actions -docstring "Called when no code action is available for the main cursor position" %{
    set-option buffer lsp_modeline ""
}

define-command -hidden lsp-perform-code-action -params 1.. -docstring "Called on :lsp-code-actions" %{
    menu %arg{@}
}

# Options for information exposed by kak-lsp.

# Count of diagnostics published for the current buffer.
declare-option -docstring "Number of errors" int lsp_diagnostic_error_count 0
declare-option -docstring "Number of hints" int lsp_diagnostic_hint_count 0
declare-option -docstring "Number of infos" int lsp_diagnostic_info_count 0
declare-option -docstring "Number of warnings" int lsp_diagnostic_warning_count 0

# Internal variables.

declare-option -hidden completions lsp_completions
declare-option -hidden range-specs lsp_errors
declare-option -hidden line-specs lsp_error_lines 0 '0| '
declare-option -hidden range-specs cquery_semhl
declare-option -hidden int lsp_timestamp -1
declare-option -hidden range-specs lsp_references
declare-option -hidden range-specs lsp_semantic_tokens
declare-option -hidden range-specs rust_analyzer_inlay_hints
declare-option -hidden range-specs lsp_diagnostics
declare-option -hidden str lsp_project_root

declare-option -hidden str lsp_modeline
set-option global modelinefmt "%%opt{lsp_modeline}%opt{modelinefmt}"

### Requests ###

define-command lsp-start -docstring "Start kak-lsp session" %{ nop %sh{ (eval ${kak_opt_lsp_cmd}) > /dev/null 2>&1 < /dev/null & } }

define-command -hidden lsp-did-change -docstring "Notify language server about buffer change" %{
    lsp-did-change-and-then nop
}

define-command -hidden lsp-did-change-and-then -params 1 -docstring %{
    Notify language server about buffer change and eval another command afterwards.
} %{ try %{
    evaluate-commands %sh{
        if [ $kak_opt_lsp_timestamp -eq $kak_timestamp ]; then
            echo "fail"
        fi
    }
    declare-option -hidden str lsp_callback "evaluate-commands -client %val{client} %arg{1}"
    set-option buffer lsp_timestamp %val{timestamp}
    evaluate-commands -save-regs '|' %{
        set-register '|' %{
# dump stdin synchronously
# append a . to the end, otherwise the subshell strips trailing newlines
lsp_draft=$(cat; printf '.')
# and process it asynchronously
(
# replace \ with \\
#         " with \"
#     <tab> with \t
lsp_draft=$(printf '%s' "$lsp_draft" | sed 's/\\/\\\\/g ; s/"/\\"/g ; s/'"$(printf '\t')"'/\\t/g')
# remove the trailing . we added earlier
lsp_draft=${lsp_draft%.}
printf '
session  = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/didChange"
[params]
draft    = """
%s"""
' "${kak_session}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${lsp_draft}" | eval ${kak_opt_lsp_cmd} --request
printf %s "${kak_opt_lsp_callback}" | kak -p "${kak_session}"
) > /dev/null 2>&1 < /dev/null & }
        execute-keys -draft '%<a-|><ret>'
    }
} catch %{
    evaluate-commands %arg{1}
}}

define-command -hidden lsp-completion -docstring "Request completions for the main cursor position" %{
    lsp-did-change-and-then lsp-completion-request
}

define-command -hidden lsp-completion-request -docstring "Request completions for the main cursor position" %{
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
            evaluate-commands %opt{lsp_completion_fragment_start}
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} ${kak_opt_lsp_completion_offset} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}}

define-command lsp-hover -docstring "Request hover info for the main cursor position" %{
    lsp-did-change-and-then lsp-hover-request
}

define-command -hidden lsp-hover-request -docstring "Request hover info for the main cursor position" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-definition -docstring "Go to definition" %{
    lsp-did-change-and-then lsp-definition-request
}

define-command -hidden lsp-definition-request -docstring "Go to definition" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-implementation -docstring "Go to implementation" %{
    lsp-did-change-and-then lsp-implementation-request
}

define-command -hidden lsp-implementation-request -docstring "Go to implementation" %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/implementation"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-type-definition -docstring "Go to type-definition" %{
    lsp-did-change-and-then lsp-type-definition-request
}

define-command -hidden lsp-type-definition-request -docstring "Go to type definition" %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/typeDefinition"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-code-actions -docstring "Perform code actions for the main cursor position" %{
    lsp-did-change-and-then 'lsp-code-actions-request true'
}

define-command -hidden lsp-code-actions-request -params 1 -docstring "Request code actions for the main cursor position" %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/codeAction"
[params]
position.line     = %d
position.column   = %d
performCodeAction = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} "$1" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-execute-command -params 2 -docstring "Execute a command" %{
    declare-option -hidden str lsp_execute_command_command %arg{1}
    declare-option -hidden str lsp_execute_command_arguments %arg{2}
    lsp-did-change-and-then %{lsp-execute-command-request %opt{lsp_execute_command_command} %opt{lsp_execute_command_arguments}}
}

define-command -hidden lsp-execute-command-request -params 2 -docstring "Execute a command" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" "$2" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-references -docstring "Open buffer with symbol references" %{
    lsp-did-change-and-then lsp-references-request
}

define-command -hidden lsp-references-request -docstring "Open buffer with symbol references" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-highlight-references -docstring "Highlight symbol references" %{
    lsp-did-change-and-then lsp-highlight-references-request
}

define-command -hidden lsp-highlight-references-request -docstring "Highlight symbol references" %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/documentHighlight"
[params.position]
line      = %d
column    = %d
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-rename -params 1 -docstring "Rename symbol under the main cursor" %{
    lsp-did-change-and-then "lsp-rename-request ""%arg{1}"""
}

define-command -hidden lsp-rename-request -params 1 -docstring "Rename symbol under the main cursor" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-rename-prompt -docstring "Rename symbol under the main cursor (prompt for a new name)" %{
    evaluate-commands -save-regs ^s %{
        execute-keys -save-regs "" Z
        try %{
            execute-keys <space><a-i>w
            # include a leading single-quote for Rust lifetime specifiers
            execute-keys <a-semicolon>Hs'?\w+<ret><a-semicolon>
        } catch %{
            fail "lsp-rename-propt: no identifier at cursor"
        }
        set-register s %val{selection}
        execute-keys z
        prompt -init %reg{s} 'New name: ' %{ lsp-rename %val{text} }
    }
}

define-command lsp-signature-help -docstring "Request signature help for the main cursor position" %{
    lsp-did-change-and-then lsp-signature-help-request
}

define-command -hidden lsp-signature-help-request -docstring "Request signature help for the main cursor position" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-diagnostics -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    lsp-did-change-and-then lsp-diagnostics-request
}

define-command -hidden lsp-diagnostics-request -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/diagnostics"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-document-symbol -docstring "Open buffer with document symbols" %{
    lsp-did-change-and-then lsp-document-symbol-request
}

define-command -hidden lsp-document-symbol-request -docstring "Open buffer with document symbols" %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "textDocument/documentSymbol"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-workspace-symbol-buffer -params 4 -docstring %{
    buffile filetype timestamp query
    Open buffer with a list of project-wide symbols matching the query
    on behalf of the buffile at timestamp
} %{
    lsp-did-change-and-then "lsp-workspace-symbol-buffer-request '%arg{1}' '%arg{2}' '%arg{3}' '%arg{4}'"
}

define-command -hidden lsp-workspace-symbol-buffer-request -params 4 -docstring %{
    buffile filetype timestamp query
    Open buffer with a list of project-wide symbols matching the query
    on behalf of the buffile at timestamp
} %{ try %{
    evaluate-commands %sh{
        if [ -z "${4}" ];
        then echo "fail"
        else echo "nop"
        fi
    }
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "workspace/symbol"
[params]
query    = "%s"
' "${kak_session}" "${kak_client}" "${1}" "${2}" "${3}" "${4}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}}

define-command lsp-capabilities -docstring "List available commands for current filetype" %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "capabilities"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-semantic-available-scopes -docstring "List available scopes for current filetype" %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "semantic-scopes"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-open %{
    # see lsp-did-change
    set-option buffer lsp_timestamp %val{timestamp}
    evaluate-commands -save-regs '|' %{
        set-register '|' %{
lsp_draft=$(cat; printf '.')
(
lsp_draft=$(printf '%s' "$lsp_draft" | sed 's/\\/\\\\/g ; s/"/\\"/g ; s/'"$(printf '\t')"'/\\t/g')
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${lsp_draft}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
        execute-keys -draft '%<a-|><ret>'
    }
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command -hidden lsp-did-change-config %{
    echo -debug "kak-lsp: config-change detected:" %opt{lsp_config}
    nop %sh{ ((printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "workspace/didChangeConfiguration"
[params.settings]
lsp_config = """%s"""
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" \
    "$(printf %s "${kak_opt_lsp_config}" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
eval "set -- $kak_quoted_opt_lsp_server_configuration"
while [ $# -gt 0 ]; do
    key=${1%%=*}
    value=${1#*=}
    value="$(printf %s "$value"|sed -e 's/\\=/=/g')"
    quotedkey='"'$(printf %s "$key"|sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')'"'

    printf '%s = %s\n' "$quotedkey" "$value"

    shift
done
) | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-apply-workspace-edit -params 1 -hidden %{
    lsp-did-change-and-then %sh{
        printf "lsp-apply-workspace-edit-request '%s'" "$(printf %s "$1" | sed "s/'/''/g")"
    }
}

define-command lsp-apply-workspace-edit-request -params 1 -hidden %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "apply-workspace-edit"
[params]
edit     = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-apply-text-edits -params 1 -hidden %{
    lsp-did-change-and-then "lsp-apply-text-edits-request '%arg{1}'"
}

define-command lsp-apply-text-edits-request -params 1 -hidden %{
    nop %sh{ (printf '
session  = "%s"
client   = "%s"
buffile  = "%s"
filetype = "%s"
version  = %d
method   = "apply-text-edits"
[params]
edit     = %s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command lsp-formatting -docstring "Format document" %{
    lsp-did-change-and-then lsp-formatting-request
}

define-command -hidden lsp-formatting-request -docstring "Format document" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null }
}

define-command lsp-range-formatting -docstring "Format selections" %{
    lsp-did-change-and-then lsp-range-formatting-request
}

define-command -hidden lsp-range-formatting-request -docstring "Format selections" %{
    nop %sh{
ranges_str="$(for range in ${kak_selections_char_desc}; do
    IFS=, read start end <<END
    $range
END
    IFS=. read startline startcolumn <<END
    $start
END
    IFS=. read endline endcolumn <<END
    $end
END
    printf '
[[ranges]]
  [ranges.start]
  line = %d
  character = %d
  [ranges.end]
  line = %d
  character = %d
' $((startline - 1)) $((startcolumn - 1)) $((endline - 1)) $((endcolumn - 1))
done)"

(printf '
session      = "%s"
client       = "%s"
buffile      = "%s"
filetype     = "%s"
version      = %d
method       = "textDocument/rangeFormatting"
[params]
tabSize      = %d
insertSpaces = %s
%s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" "${ranges_str}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null
}}

define-command lsp-formatting-sync -docstring "Format document, blocking Kakoune session until done" %{
    lsp-did-change-and-then lsp-formatting-sync-request
}

define-command -hidden lsp-formatting-sync-request -docstring "Format document, blocking Kakoune session until done" %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${pipe} "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null

cat ${pipe}
rm -rf ${tmp}
}}

define-command lsp-range-formatting-sync -docstring "Format selections, blocking Kakoune session until done" %{
    lsp-did-change-and-then lsp-range-formatting-sync-request
}

define-command -hidden lsp-range-formatting-sync-request -docstring "Format selections, blocking Kakoune session until done" %{
    evaluate-commands -no-hooks %sh{
range=${kak_selection_desc}
tmp=$(mktemp -q -d -t 'lsp-formatting.XXXXXX' 2>/dev/null || mktemp -q -d)
pipe=${tmp}/fifo
mkfifo ${pipe}
ranges_str="$(for range in ${kak_selections_char_desc}; do
    IFS=, read start end <<< $range
    IFS=. read startline startcolumn <<< $start
    IFS=. read endline endcolumn <<< $end
    printf '
[[ranges]]
  [ranges.start]
  line = %d
  character = %d
  [ranges.end]
  line = %d
  character = %d
' $((startline - 1)) $((startcolumn - 1)) $((endline - 1)) $((endcolumn - 1))
done)"

(printf '
session      = "%s"
client       = "%s"
buffile      = "%s"
filetype     = "%s"
version      = %d
fifo         = "%s"
method       = "textDocument/rangeFormatting"
[params]
tabSize      = %d
insertSpaces = %s
%s
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${pipe} "${kak_opt_tabstop}" "${kak_opt_lsp_insert_spaces}" "${ranges_str}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null

cat ${pipe} | tee /tmp/pipe
rm -rf ${tmp}
}}

# CCLS Extension

define-command ccls-navigate -docstring "Navigate C/C++/ObjectiveC file" -params 1 %{
    lsp-did-change-and-then "ccls-navigate-request '%arg{1}'"
}

define-command -hidden ccls-navigate-request -docstring "Navigate C/C++/ObjectiveC file" -params 1 %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$1" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-vars -docstring "ccls-vars: Find instances of symbol at point." %{
    lsp-did-change-and-then ccls-vars-request
}

define-command -hidden ccls-vars-request -docstring "ccls-vars: Find instances of symbol at point." %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-inheritance -params 1..2 -docstring "ccls-inheritance <derived|base> [levels]: Find base- or derived classes of symbol at point." %{
    lsp-did-change-and-then "ccls-inheritance-request '%arg{1}' '%arg{2}'"
}

define-command -hidden ccls-inheritance-request -params 1..2 -docstring "ccls-inheritance <derived|base> [levels]: Find base- or derived classes of symbol at point." %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$derived" "$levels" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-call -params 1 -docstring "ccls-call <caller|callee>: Find callers or callees of symbol at point." %{
    lsp-did-change-and-then "ccls-call-request '%arg{1}'"
}

define-command -hidden ccls-call-request -params 1 -docstring "ccls-call <caller|callee>: Find callers or callees of symbol at point." %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" "$callee" ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

define-command ccls-member -params 1 -docstring "ccls-member <vars|types|functions>: Find member variables/types/functions of symbol at point." %{
    lsp-did-change-and-then "ccls-member-request '%arg{1}'"
}

define-command -hidden ccls-member-request -params 1 -docstring "ccls-member <vars|types|functions>: Find member variables/types/functions of symbol at point." %{
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
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" $kind ${kak_cursor_line} ${kak_cursor_column} | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

# clangd Extensions

define-command clangd-switch-source-header -docstring "clangd-switch-source-header: Switch source/header." %{
    lsp-did-change-and-then clangd-switch-source-header-request
}

define-command -hidden clangd-switch-source-header-request -docstring "clangd-switch-source-header: Switch source/header." %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/switchSourceHeader"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

# eclipse.jdt.ls Extension
#
define-command ejdtls-organize-imports -docstring "ejdtls-organize-imports: Organize imports." %{
    lsp-did-change-and-then ejdtls-organize-imports-request
}

define-command -hidden ejdtls-organize-imports-request -docstring "ejdtls-organize-imports: Organize imports." %{
    nop %sh{ (printf '
session   = "%s"
client    = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "eclipse.jdt.ls/organizeImports"
[params]
' "${kak_session}" "${kak_client}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

# rust-analyzer extensions

define-command rust-analyzer-inlay-hints -docstring "rust-analyzer-inlay-hints: Request inlay hints (rust-analyzer)" %{
  lsp-did-change-and-then rust-analyzer-inlay-hints-request
}

define-command -hidden rust-analyzer-inlay-hints-request %{
    nop %sh{ (printf '
session   = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "rust-analyzer/inlayHints"
[params]
' "${kak_session}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

# semantic tokens

define-command lsp-semantic-tokens -docstring "semantic-tokens-update: Request semantic tokens" %{
  lsp-did-change-and-then lsp-semantic-tokens-request
}

define-command -hidden lsp-semantic-tokens-request %{
    nop %sh{ (printf '
session   = "%s"
buffile   = "%s"
filetype  = "%s"
version   = %d
method    = "textDocument/semanticTokens/full"
[params]
' "${kak_session}" "${kak_buffile}" "${kak_opt_filetype}" "${kak_timestamp}" | eval ${kak_opt_lsp_cmd} --request) > /dev/null 2>&1 < /dev/null & }
}

### Response handling ###

# Feel free to override these commands in your config if you need to customise response handling.

define-command -hidden lsp-show-hover -params 3 -docstring %{
    lsp-show-hover <anchor> <info> <diagnostics>
    Render hover info.
} %{ evaluate-commands %sh{
    lsp_info=$2
    lsp_diagnostics=$3

    # To make sure we always show diagnostics, restrict only the info portion based
    # on the configured maximum line count
    if [ $kak_opt_lsp_hover_max_lines -gt 0 ]; then
        diagnostics_count=$(printf %s "$lsp_diagnostics" | wc -l)
        if [ $diagnostics_count -gt 0 ]; then
            # By default, we print blank lines before diagnostics, plus the "Diagnostics:"
            # header, so subtract 3.
            lsp_info=$(printf %s "$lsp_info" | head -n $(($kak_opt_lsp_hover_max_lines - 3 - $diagnostics_count)))
        else
            lsp_info=$(printf %s "$lsp_info" | head -n $kak_opt_lsp_hover_max_lines)
        fi
    fi

    content=$(eval "${kak_opt_lsp_show_hover_format}")
    # remove leading whitespace characters
    content="${content#"${content%%[![:space:]]*}"}"
    # remove trailing whitespace characters
    content="${content%"${content##*[![:space:]]}"}"

    content=$(printf %s "$content" | sed s/\'/\'\'/g)

    case $kak_opt_lsp_hover_anchor in
        true) printf "info -markup -anchor %%arg{1} -- '%s'" "$content";;
        *)    printf "info -markup -- '%s'" "$content";;
    esac
}}

define-command -hidden lsp-show-error -params 1 -docstring "Render error" %{
    echo -debug "kak-lsp:" %arg{1}
    info "kak-lsp: %arg{1}"
}

define-command -hidden lsp-show-diagnostics -params 2 -docstring "Render diagnostics" %{
    evaluate-commands -save-regs '"' -try-client %opt[toolsclient] %{
        edit! -scratch *diagnostics*
        set-option buffer filetype lsp-goto
        set-option buffer lsp_project_root "%arg{1}/"
        alias buffer lsp-jump lsp-diagnostics-jump
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command -hidden lsp-show-goto-choices -params 2 -docstring "Render goto choices" %{
    evaluate-commands -save-regs '"' -try-client %opt[toolsclient] %{
        edit! -scratch *goto*
        set-option buffer filetype lsp-goto
        set-option buffer grep_current_line 0
        set-option buffer lsp_project_root "%arg{1}/"
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command -hidden lsp-show-document-symbol -params 2 -docstring "Render document symbols" %{
    evaluate-commands -save-regs '"' -try-client %opt[toolsclient] %{
        edit! -scratch *symbols*
        set-option buffer filetype lsp-goto
        set-option buffer grep_current_line 0
        set-option buffer lsp_project_root "%arg{1}/"
        set-register '"' %arg{2}
        execute-keys Pgg
    }
}

define-command lsp-next-location -params 1 -docstring %{
    lsp-next-location <bufname>
    Jump to next location listed in the given grep-like buffer, usually one of
    *diagnostics* *goto* *grep* *implementations* *lint-output* *make* *references* *symbols*

    %opt{lsp_location_format} determines matching locations.
} -buffer-completion %{
    evaluate-commands -try-client %opt{jumpclient} %{
        buffer %arg{1}
        execute-keys "ge %opt{grep_current_line}g<a-l> /%opt{lsp_location_format}<ret>"
        lsp-jump
    }
    try %{
        evaluate-commands -client %opt{toolsclient} %{
            buffer %arg{1}
            execute-keys gg %opt{grep_current_line}g
        }
    }
}

define-command lsp-previous-location -params 1 -docstring %{
    lsp-previous-location <bufname>
    Jump to previous location listed in the given grep-like buffer, usually one of
    *diagnostics* *goto* *grep* *implementations* *lint-output* *make* *references* *symbols*

    %opt{lsp_location_format} determines matching locations.
} -buffer-completion %{
    evaluate-commands -try-client %opt{jumpclient} %{
        buffer %arg{1}
        execute-keys "ge %opt{grep_current_line}g<a-h> <a-/>%opt{lsp_location_format}<ret>"
        lsp-jump
    }
    try %{
        evaluate-commands -client %opt{toolsclient} %{
            buffer %arg{1}
            execute-keys gg %opt{grep_current_line}g
        }
    }
}

define-command -hidden lsp-update-workspace-symbol -params 2 -docstring "Update workspace symbols buffer" %{
    evaluate-commands -save-regs '"' %{
        set-option buffer lsp_project_root "%arg{1}/"
        execute-keys '<a-;>%<a-;>d'
        set-register '"' %arg{2}
        execute-keys '<a-;>P<a-;>gg'
    }
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

define-command -hidden lsp-show-message-error -params 1 -docstring %{
    lsp-show-message-error <message>
    Render language server message of the "error" level.
} %{
    echo -debug "kak-lsp: error from server:" %arg{1}
    evaluate-commands -try-client %opt{toolsclient} %{
        info "kak-lsp: error from server: %arg{1}"
    }
}

define-command -hidden lsp-show-message-warning -params 1 -docstring %{
    lsp-show-message-warning <message>
    Render language server message of the "warning" level.
} %{
    echo -debug "kak-lsp: warning from server:" %arg{1}
    evaluate-commands -try-client %opt{toolsclient} %{
        echo "kak-lsp: warning from server:" %arg{1}
    }
}

define-command -hidden lsp-show-message-info -params 1 -docstring %{
    lsp-show-message-info <message>
    Render language server message of the "info" level.
} %{
    echo -debug "kak-lsp: info from server:" %arg{1}
    evaluate-commands -try-client %opt{toolsclient} %{
        echo "kak-lsp: info from server:" %arg{1}
    }
}

define-command -hidden lsp-show-message-log -params 1 -docstring %{
    lsp-show-message-log <message>
    Render language server message of the "log" level.
} %{
    echo -debug "kak-lsp: log:" %arg{1}
}

define-command -hidden lsp-insert-before-selection -params 1 -docstring %{
    Insert content before current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    declare-option -hidden str lsp_text_edit_tmp %sh{ mktemp }
    echo -to-file %opt{lsp_text_edit_tmp} %arg{1}
    execute-keys "!cat $kak_opt_lsp_text_edit_tmp; rm $kak_opt_lsp_text_edit_tmp<ret>"
}

define-command -hidden lsp-replace-selection -params 1 -docstring %{
    Replace content of current selections while keeping cursor intact.
    It is used to apply text edits from language server.
} %{
    declare-option -hidden str lsp_text_edit_tmp %sh{ mktemp }
    echo -to-file %opt{lsp_text_edit_tmp} %arg{1}
    try %{
        execute-keys -draft <a-k>\n\z<ret>
        execute-keys "|cat $kak_opt_lsp_text_edit_tmp; rm $kak_opt_lsp_text_edit_tmp<ret>"
    } catch %{
        # The input has no newline. Kakoune would add one to the input, but also strip it from
        # the output. Account for that.
        execute-keys "|cat $kak_opt_lsp_text_edit_tmp; echo; rm $kak_opt_lsp_text_edit_tmp<ret>"
    }
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
(eval "set -- $kak_quoted_opt_lsp_server_initialization_options"
while [ $# -gt 0 ]; do
    key=${1%%=*}
    value=${1#*=}
    quotedkey='"'$(printf %s "$key"|sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')'"'

    printf '%s = %s\n' "$quotedkey" "$value"

    shift
done
) > $1 }
}

define-command -hidden lsp-get-config -params 1 -docstring %{
    lsp-get-config <fifo>
    Format lsp_config as TOML and write to the given <fifo> path.
} %{
    echo -to-file %arg{@} %opt{lsp_config}
}

### Other commands ###

define-command lsp-find-error -params 0..2 -docstring "lsp-find-error [--previous] [--include-warnings]
Jump to the next or previous diagnostic error" %{
    evaluate-commands %sh{
        previous=false
        if [ "$1" = "--previous" ]; then
            previous=true
            shift
        fi
        includeWarnings=false
        if [ "$1" = "--include-warnings" ]; then
            includeWarnings=true
        fi
        #expand quoting, stores option in $@
        eval set -- "${kak_quoted_opt_lsp_errors}"

        first=""
        current=""
        prev=""
        selection=""
        for e in "$@"; do
            if [ -z "${e##*DiagnosticError*}" ] || {
                $includeWarnings && [ -z "${e##*DiagnosticWarning*}" ]
            } then # e is an error or warning
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
        printf 'edit "%b" %b' "$kak_buffile" "$selection"
    }
}

define-command lsp-workspace-symbol -params 1 -docstring "Open buffer with a list of project-wide symbols matching the query" %{ lsp-workspace-symbol-buffer %val{buffile} %opt{filetype} %val{timestamp} %arg{1} }

define-command lsp-workspace-symbol-incr -docstring "Open buffer with an incrementally updated list of project-wide symbols matching the query" %{
    declare-option -hidden str lsp_ws_buffile %val{buffile}
    declare-option -hidden str lsp_ws_filetype %opt{filetype}
    declare-option -hidden int lsp_ws_timestamp %val{timestamp}
    declare-option -hidden str lsp_ws_query
    evaluate-commands -try-client %opt[toolsclient] %{
        edit! -scratch *symbols*
        set-option buffer filetype lsp-goto
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
}

### Hooks and highlighters ###

define-command lsp-inline-diagnostics-enable -params 1 -docstring "lsp-inline-diagnostics-enable <scope>: Enable inline diagnostics highlighting for <scope>" %{
    add-highlighter "%arg{1}/lsp_errors" ranges lsp_errors
} -shell-script-candidates %{ printf '%s\n' global buffer window }

define-command lsp-inline-diagnostics-disable -params 1 -docstring "lsp-inline-diagnostics-disable <scope>: Disable inline diagnostics highlighting for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_errors"
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-diagnostic-lines-enable -params 1 -docstring "lsp-diagnostic-lines-enable <scope>: Show flags on lines with diagnostics in <scope>" %{
    add-highlighter "%arg{1}/lsp_error_lines" flag-lines LineNumbers lsp_error_lines
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-diagnostic-lines-disable -params 1 -docstring "lsp-diagnostic-lines-disable <scope>: Hide flags on lines with diagnostics in <scope>"  %{
    remove-highlighter "%arg{1}/lsp_error_lines"
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-diagnostics-enable -params 1 -docstring "lsp-inlay-diagnostics-enable <scope>: Enable inlay diagnostics highlighting for <scope>" %{
    add-highlighter "%arg{1}/lsp_diagnostics" replace-ranges lsp_diagnostics
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-diagnostics-disable -params 1 -docstring "lsp-inlay-diagnostics-disable <scope>: Disable inlay diagnostics highlighting for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_diagnostics"
} -shell-script-candidates %{ printf '%s\n' buffer global window }

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
    diagnostic-lines-enable diagnostic-lines-disable auto-hover-enable auto-hover-disable\
    auto-hover-insert-mode-enable auto-hover-insert-mode-disable auto-signature-help-enable\
    auto-signature-help-disable stop-on-exit-enable stop-on-exit-disable\
    find-error implementation;
        do echo $cmd;
    done
} %{ evaluate-commands "lsp-%arg{1}" }


### User mode ###

declare-user-mode lsp
map global lsp a '<esc>: lsp-code-actions<ret>'           -docstring 'show code actions for current position'
map global lsp c '<esc>: lsp-capabilities<ret>'           -docstring 'show language server capabilities'
map global lsp d '<esc>: lsp-definition<ret>'             -docstring 'go to definition'
map global lsp e '<esc>: lsp-diagnostics<ret>'            -docstring 'list project errors, info, hints and warnings'
map global lsp f '<esc>: lsp-formatting<ret>'             -docstring 'format buffer'
map global lsp h '<esc>: lsp-hover<ret>'                  -docstring 'show info for current position'
map global lsp i '<esc>: lsp-implementation<ret>'         -docstring 'go to implementation'
map global lsp r '<esc>: lsp-references<ret>'             -docstring 'list symbol references'
map global lsp R '<esc>: lsp-rename-prompt<ret>'          -docstring 'rename symbol'
map global lsp s '<esc>: lsp-signature-help<ret>'         -docstring 'show function signature help'
map global lsp S '<esc>: lsp-document-symbol<ret>'        -docstring 'list document symbols'
map global lsp o '<esc>: lsp-workspace-symbol-incr<ret>'  -docstring 'search project symbols'
map global lsp n '<esc>: lsp-find-error<ret>'             -docstring 'find next error'
map global lsp p '<esc>: lsp-find-error --previous<ret>'  -docstring 'find previous error'
map global lsp q '<esc>: lsp-exit<ret>'                   -docstring 'exit session'
map global lsp y '<esc>: lsp-type-definition<ret>'        -docstring 'go to type definition'
map global lsp <&> '<esc>: lsp-highlight-references<ret>' -docstring 'lsp-highlight-references'
map global lsp = '<esc>: lsp-range-formatting<ret>'       -docstring 'format selections'

### Default integration ###

define-command -hidden lsp-enable -docstring "Default integration with kak-lsp" %{
    try %{
        add-highlighter global/cquery_semhl ranges cquery_semhl
    } catch %{
        fail 'lsp-enable: already enabled'
    }
    add-highlighter global/lsp_references ranges lsp_references
    add-highlighter global/lsp_semantic_tokens ranges lsp_semantic_tokens
    add-highlighter global/rust_analyzer_inlay_hints replace-ranges rust_analyzer_inlay_hints
    add-highlighter global/lsp_snippets_placeholders ranges lsp_snippets_placeholders
    lsp-inline-diagnostics-enable global
    lsp-diagnostic-lines-enable global

    set-option global completers option=lsp_completions %opt{completers}

    map global goto d '<esc>: lsp-definition<ret>' -docstring 'definition'
    map global goto r '<esc>: lsp-references<ret>' -docstring 'references'
    map global goto y '<esc>: lsp-type-definition<ret>' -docstring 'type definition'

    hook -group lsp global BufCreate .* %{
        lsp-did-open
        lsp-did-change-config
    }
    hook -group lsp global BufClose .* lsp-did-close
    hook -group lsp global BufWritePost .* lsp-did-save
    hook -group lsp global BufSetOption lsp_config=.* lsp-did-change-config
    hook -group lsp global BufSetOption lsp_server_configuration=.* lsp-did-change-config
    hook -group lsp global InsertIdle .* lsp-completion
    hook -group lsp global NormalIdle .* %{
        lsp-did-change-and-then 'lsp-code-actions-request false'
        %sh{if $kak_opt_lsp_auto_highlight_references; then echo "lsp-highlight-references"; else echo "nop"; fi}
    }

    lsp-did-change-config
}

define-command -hidden lsp-disable -docstring "Disable kak-lsp" %{
    remove-highlighter global/cquery_semhl
    remove-highlighter global/lsp_references
    remove-highlighter global/lsp_semantic_tokens
    remove-highlighter global/rust_analyzer_inlay_hints
    remove-highlighter global/lsp_snippets_placeholders
    lsp-inline-diagnostics-disable global
    lsp-diagnostic-lines-disable global
    unmap global goto d '<esc>: lsp-definition<ret>'
    unmap global goto r '<esc>: lsp-references<ret>'
    unmap global goto y '<esc>: lsp-type-definition<ret>'
    remove-hooks global lsp
    remove-hooks global lsp-auto-hover
    remove-hooks global lsp-auto-hover-insert-mode
    remove-hooks global lsp-auto-signature-help
    lsp-exit
}

define-command lsp-enable-window -docstring "Default integration with kak-lsp in the window scope" %{
    try %{
        add-highlighter window/cquery_semhl ranges cquery_semhl
    } catch %{
        fail 'lsp-enable-window: already enabled'
    }
    add-highlighter window/lsp_references ranges lsp_references
    add-highlighter window/lsp_semantic_tokens ranges lsp_semantic_tokens
    add-highlighter window/rust_analyzer_inlay_hints replace-ranges rust_analyzer_inlay_hints
    add-highlighter window/lsp_snippets_placeholders ranges lsp_snippets_placeholders

    set-option window completers option=lsp_completions %opt{completers}

    lsp-inline-diagnostics-enable window
    lsp-diagnostic-lines-enable window

    map window goto d '<esc>: lsp-definition<ret>' -docstring 'definition'
    map window goto r '<esc>: lsp-references<ret>' -docstring 'references'
    map window goto y '<esc>: lsp-type-definition<ret>' -docstring 'type definition'

    hook -group lsp window WinClose .* lsp-did-close
    hook -group lsp window BufWritePost .* lsp-did-save
    hook -group lsp window WinSetOption lsp_config=.* lsp-did-change-config
    hook -group lsp window WinSetOption lsp_server_configuration=.* lsp-did-change-config
    hook -group lsp window InsertIdle .* lsp-completion
    hook -group lsp window NormalIdle .* %{
        lsp-did-change-and-then 'lsp-code-actions-request false'
        %sh{if $kak_opt_lsp_auto_highlight_references; then echo "lsp-highlight-references"; else echo "nop"; fi}
    }

    lsp-did-open
    lsp-did-change-config
}

define-command lsp-disable-window -docstring "Disable kak-lsp in the window scope" %{
    remove-highlighter window/cquery_semhl
    remove-highlighter window/lsp_references
    remove-highlighter window/lsp_semantic_tokens
    remove-highlighter window/rust_analyzer_inlay_hints
    remove-highlighter window/lsp_snippets_placeholders
    lsp-inline-diagnostics-disable window
    lsp-diagnostic-lines-disable window
    unmap window goto d '<esc>: lsp-definition<ret>'
    unmap window goto r '<esc>: lsp-references<ret>'
    unmap window goto y '<esc>: lsp-type-definition<ret>'
    remove-hooks window lsp
    remove-hooks global lsp-auto-hover
    remove-hooks global lsp-auto-hover-insert-mode
    remove-hooks global lsp-auto-signature-help
    lsp-exit
}

lsp-stop-on-exit-enable
hook -always -group lsp global KakEnd .* lsp-exit

# SNIPPETS
# This is a slightly modified version of occivink/kakoune-snippets

decl -hidden range-specs lsp_snippets_placeholders
decl -hidden int-list lsp_snippets_placeholder_groups

face global SnippetsNextPlaceholders black,green+F
face global SnippetsOtherPlaceholders black,yellow+F

# First param is the text that was inserted in the completion, which will be deleted
# Second param is the actual snippet
def -hidden lsp-snippets-insert-completion -params 2 %{ eval -save-regs "a" %{
    reg 'a' "%arg{1}"
    exec -draft "<a-;><a-/><c-r>a<ret>d"
    eval -draft -verbatim lsp-snippets-insert %arg{2}
    remove-hooks window lsp-post-completion
    hook -once -group lsp-post-completion window InsertCompletionHide .* %{
        try %{
            lsp-snippets-select-next-placeholders
            exec '<a-;>d'
        }
    }
}}

def lsp-snippets-insert -hidden -params 1 %[
    eval %sh{
        if ! command -v perl > /dev/null 2>&1; then
            printf "fail '''perl'' must be installed to use the ''snippets-insert'' command'"
        fi
    }
    eval -draft -save-regs '^"' %[
        reg '"' %arg{1}
        exec <a-P>
        # replace leading tabs with the appropriate indent
        try %{
            reg '"' %sh{
                if [ $kak_opt_indentwidth -eq 0 ]; then
                    printf '\t'
                else
                    printf "%${kak_opt_indentwidth}s"
                fi
            }
            exec -draft '<a-s>s\A\t+<ret>s.<ret>R'
        }
        # align everything with the current line
        eval -draft -itersel -save-regs '"' %{
            try %{
                exec -draft -save-regs '/' '<a-s>)<space><a-x>s^\s+<ret>y'
                exec -draft '<a-s>)<a-space>P'
            }
        }
        try %[
            # select things that look like placeholders
            # this regex is not as bad as it looks
            eval -draft %[
                exec s((?<lt>!\\)(\\\\)*|\A)\K(\$(\d+|\{(\d+(:(\\\}|[^}])*)?)\}))<ret>
                # tests
                # $1                - ok
                # ${2}              - ok
                # ${3:}             - ok
                # $1$2$3            - ok x3
                # $1${2}$3${4}      - ok x4
                # $1\${2}\$3${4}    - ok, not ok, not ok, ok
                # \\${3:abc}        - ok
                # \${3:abc}         - not ok
                # \\\${3:abc}def    - not ok
                # ${4:a\}b}def      - ok
                # ${5:ab\}}def      - ok
                # ${6:ab\}cd}def    - ok
                # ${7:ab\}\}cd}def  - ok
                # ${8:a\}b\}c\}}def - ok
                lsp-snippets-insert-perl-impl
            ]
        ]
        try %{
            # unescape $
            exec 's\\\$<ret>;d'
        }
    ]
]

def -hidden lsp-snippets-insert-perl-impl %[
    eval %sh[ # $kak_quoted_selections
        perl -e '
use strict;
use warnings;
use Text::ParseWords();

my @sel_content = Text::ParseWords::shellwords($ENV{"kak_quoted_selections"});

my %placeholder_id_to_default;
my @placeholder_ids;

print("set window lsp_snippets_placeholder_groups");
for my $i (0 .. $#sel_content) {
    my $sel = $sel_content[$i];
    $sel =~ s/\A\$\{?|\}\Z//g;
    my ($placeholder_id, $placeholder_default) = ($sel =~ /^(\d+)(?::(.*))?$/);
    if ($placeholder_id eq "0") {
        $placeholder_id = "9999";
    }
    $placeholder_ids[$i] = $placeholder_id;
    print(" $placeholder_id");
    if (defined($placeholder_default)) {
        $placeholder_id_to_default{$placeholder_id} = $placeholder_default;
    }
}
print("\n");

print("reg dquote");
foreach my $i (0 .. $#sel_content) {
    my $placeholder_id = $placeholder_ids[$i];
    if (exists $placeholder_id_to_default{$placeholder_id}) {
        my $def = $placeholder_id_to_default{$placeholder_id};
        # de-double up closing braces
        $def =~ s/\}\}/}/g;
        # double up single-quotes
        $def =~ s/'\''/'\'''\''/g;
        print(" '\''$def'\''");
    } else {
        print(" '\'''\''");
    }
}
print("\n");
'
    ]
    exec R
    set window lsp_snippets_placeholders %val{timestamp}
    # no need to set the NextPlaceholders face yet, select-next-placeholders will take care of that
    eval -itersel %{ set -add window lsp_snippets_placeholders "%val{selections_desc}|SnippetsOtherPlaceholders" }
]

def lsp-snippets-select-next-placeholders %{
    update-option window lsp_snippets_placeholders
    eval %sh{
        eval set -- "$kak_quoted_opt_lsp_snippets_placeholder_groups"
        if [ $# -eq 0 ]; then printf "fail 'There are no next placeholders'"; exit; fi
        next_id=9999
        second_next_id=9999
        for placeholder_id do
            if [ "$placeholder_id" -lt "$next_id" ]; then
                second_next_id="$next_id"
                next_id="$placeholder_id"
            elif [ "$placeholder_id" -lt "$second_next_id" ] && [ "$placeholder_id" -ne "$next_id" ]; then
                second_next_id="$placeholder_id"
            fi
        done
        next_descs_id=''
        second_next_descs_id='' # for highlighting purposes
        desc_id=0
        printf 'set window lsp_snippets_placeholder_groups'
        for placeholder_id do
            if [ "$placeholder_id" -eq "$next_id" ]; then
                next_descs_id="${next_descs_id} $desc_id"
            else
                printf ' %s' "$placeholder_id"
            fi
            if [ "$placeholder_id" -eq "$second_next_id" ]; then
                second_next_descs_id="${second_next_descs_id} $desc_id"
            fi
            desc_id=$((desc_id+1))
        done
        printf '\n'

        eval set -- "$kak_quoted_opt_lsp_snippets_placeholders"
        printf 'set window lsp_snippets_placeholders'
        printf ' %s' "$1"
        shift
        selections=''
        desc_id=0
        for desc do
            found=0
            for candidate_desc_id in $next_descs_id; do
                if [ "$candidate_desc_id" -eq "$desc_id" ]; then
                    found=1
                    selections="${selections} ${desc%%\|*}"
                    break
                fi
            done
            if [ $found -eq 0 ]; then
                for candidate_desc_id in $second_next_descs_id; do
                    if [ "$candidate_desc_id" -eq "$desc_id" ]; then
                        found=1
                        printf ' %s' "${desc%%\|*}|SnippetsNextPlaceholders"
                        break
                    fi
                done
                if [ $found -eq 0 ]; then
                    printf ' %s' "$desc"
                fi
            fi
            desc_id=$((desc_id+1))
        done
        printf '\n'

        printf "select %s\n" "$selections"
    }
}

hook -group lsp-goto-highlight global WinSetOption filetype=lsp-goto %{ # from grep.kak
    add-highlighter window/lsp-goto group
    add-highlighter window/lsp-goto/ regex %opt{lsp_location_format} 1:cyan 2:green 3:green
    add-highlighter window/lsp-goto/ line %{%opt{grep_current_line}} default+b
    hook -once -always window WinSetOption filetype=.* %{ remove-highlighter window/lsp-goto }
}

hook global WinSetOption filetype=lsp-goto %{
    hook buffer -group lsp-goto-hooks NormalKey <ret> lsp-jump
    hook -once -always window WinSetOption filetype=.* %{ remove-hooks buffer lsp-goto-hooks }
}

define-command -hidden lsp-make-register-relative-to-root %{
    evaluate-commands -save-regs / %{
        try %{
            # Is it an absolute path?
            execute-keys s\A/.*<ret>
        } catch %{
            set-register a "%opt{lsp_project_root}%reg{a}"
        }
    }
}

define-command -hidden lsp-jump %{ # from grep.kak
    evaluate-commands -save-regs abc %{ # use evaluate-commands to ensure jumps are collapsed
        try %{
            execute-keys "<a-x>s%opt{lsp_location_format}<ret>"
            set-register a "%reg{1}"
            set-register b "%reg{2}"
            set-register c "%reg{3}"
            lsp-make-register-relative-to-root
            set-option buffer grep_current_line %val{cursor_line}
            evaluate-commands -try-client %opt{jumpclient} -verbatim -- edit -existing %reg{a} %reg{b} %reg{c}
            try %{ focus %opt{jumpclient} }
        }
    }
}

define-command -hidden lsp-diagnostics-jump %{ # from make.kak
    evaluate-commands -save-regs abcd %{
        execute-keys "<a-x>s%opt{lsp_location_format}<ret>"
        set-register a "%reg{1}"
        set-register b "%reg{2}"
        set-register c "%reg{3}"
        set-register d "%reg{4}"
        lsp-make-register-relative-to-root
        set-option buffer grep_current_line %val{cursor_line}
        lsp-diagnostics-open-error %reg{a} "%reg{b}" "%reg{c}" "%reg{d}"
    }
}

define-command -hidden lsp-diagnostics-open-error -params 4 %{
    evaluate-commands -try-client %opt{jumpclient} %{
        edit -existing "%arg{1}" %arg{2} %arg{3}
        echo -markup "{Information}{\}%arg{4}"
        try %{ focus }
    }
}

# Deprecated commands.
define-command lsp-symbols-next-match -docstring 'DEPRECATED: use lsp-next-location. Jump to the next symbols match' %{
    lsp-next-location '*symbols*'
}

define-command lsp-symbols-previous-match -docstring 'DEPRECATED: use lsp-previous-location. Jump to the previous symbols match' %{
    lsp-previous-location '*symbols*'
}

define-command lsp-goto-next-match -docstring 'DEPRECATED: use lsp-next-location. Jump to the next goto match' %{
    lsp-next-location '*goto*'
}

define-command lsp-goto-previous-match -docstring 'DEPRECATED: use lsp-previous-location. Jump to the previous goto match' %{
    lsp-previous-location '*goto*'
}
