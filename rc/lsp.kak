declare-option -docstring %{TOML table of language servers to use

This option is usually set in buffer scope by the hooks in the 'lsp-filetype-.*' groups. Feel
free to add your own hooks to override the default ones. If you prefer to opt in to each language
server, remove or disable the 'lsp-filetype-*' hooks.

To add a language server, add a sub-table with the language server's name.
For example

    hook global BufSetOption filetype=(?:c|cpp) %{
        set-option buffer lsp_servers %{
            [clangd]
            args = ["--log=error"]
            root_globs = ["compile_commands.json", ".clangd", ".git", ".hg"]
        }
    }

Each language server table needs to define exactly one of
- root       = root directory to use for the given language server and buffer
- root_globs = detect 'root' based on the first glob that matches in a parent
               directory of the buffer's file.

Additionally, optional configuration options can be added:
- args             = arguments to pass to the language server process at startup
- settings         = table of arbitrary server-specific settings
- settings_section = name of a sub-table of above settings; that sub-table will be actively
                     sent on initialization and settings changes. The language server can
                     request other keys via 'workspace/configuration'.
- experimental     = table of arbitrary server-specific experimental features to enable during
                     initialization.

} str lsp_servers %{}

declare-option -docstring %{LSP languageId, usually same as filetype option

See https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocumentItem
} str lsp_language_id

declare-option -docstring %{
    By default, only errors, warnings and some infrequent events are logged to the *debug* buffer.
    Set this to true to enable debug logs as well.
} bool lsp_debug false

declare-option -docstring %{
    Exit session if no requests were received during given period in seconds
    set to 0 to disable
} int lsp_timeout 18000

# Feel free to update path and arguments according to your setup when sourcing lsp.kak directly.
declare-option -docstring "Command with which lsp is run" str lsp_cmd kak-lsp

declare-option -docstring "Snippet support (completions with placeholders)" bool lsp_snippet_support true
declare-option -docstring "File watcher support" bool lsp_file_watch_support false

# Faces

declare-option -docstring %{
    Faces to apply by token kind and modifiers
    Each line is of the form <face> <token> [<modifiers>...]

    See https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_semanticTokens
    for the default list of tokens and modifiers.
    However, many language servers implement their own values.
    Make sure to check the output of `lsp-capabilities` and each server's documentation and source code as well.
    Examples:
    - TypeScript: https://github.com/microsoft/vscode-languageserver-node/blob/main/client/src/common/semanticTokens.ts
    - Rust Analyzer: https://github.com/rust-analyzer/rust-analyzer/blob/master/crates/ide/src/syntax_highlighting.rs
} str lsp_semantic_tokens %{
    [
        {face="documentation", token="comment", modifiers=["documentation"]},
        {face="comment", token="comment"},
        {face="function", token="function"},
        {face="function", token="method"},
        {face="keyword", token="keyword"},
        {face="module", token="namespace"},
        {face="operator", token="operator"},
        {face="string", token="string"},
        {face="type", token="type"},
        {face="default+d", token="variable", modifiers=["readonly"]},
        {face="default+d", token="variable", modifiers=["constant"]},
        {face="variable", token="variable"},
    ]
}

# Faces used by inline diagnostics.
set-face global DiagnosticError red
set-face global DiagnosticHint default
set-face global DiagnosticInfo default
set-face global DiagnosticWarning yellow
set-face global DiagnosticTagDeprecated +s
set-face global DiagnosticTagUnnecessary +d
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
set-face global InlayCodeLens cyan+d

# Options for tuning LSP behaviour.

# Display hover info anchored to the hovered position.
declare-option -docstring "Display hover info anchored to the hovered position" bool lsp_hover_anchor false
# Completions request is sent only when this expression doesn't fail.
# By default, it ensures that preceding character is not a whitespace.
declare-option -docstring "Completion request is sent only when this expression does not fail" str lsp_completion_trigger %{execute-keys '<a-h><a-k>\S.\z<ret>'}
# Kakoune requires completions to point fragment start rather than cursor position.
# This variable provides a way to customise how fragment start is detected.
# By default, it tracks back to the first punctuation or whitespace.
declare-option -docstring "Select from cursor to the start of the term being completed" str lsp_completion_fragment_start %{execute-keys "<esc><a-h>s\$?[\w%opt{lsp_extra_word_chars}]+.\z<ret>"}
declare-option -hidden str lsp_extra_word_chars
# Update lsp_extra_word_chars whenever extra_word_chars changes.
# We could avoid this if we are not enabled, but this doesn't trigger that often and that would complicate initialization.
hook -group lsp-extra-word-chars global WinSetOption extra_word_chars=.* %{
    set-option window lsp_extra_word_chars %sh{
        eval set -- "$kak_quoted_opt_extra_word_chars"
        for char; do
            case "$char" in
                (-) printf '\\-' ;;
                (\<) printf '<lt>' ;;
                (\\) printf '\\\\' ;;
                (]) printf '\\]' ;;
                (^) printf '\\^' ;;
                (*) printf %s "$char" ;;
            esac
        done
    }
}
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
# Set to true to highlight when code actions are available.
declare-option -docstring "Show available code actions (default: a 💡 in the modeline)" bool lsp_auto_show_code_actions true
# Set it to a positive number to limit the size of the lsp-hover output. Use 0 to disable the limit.
declare-option -docstring "Set it to a positive number to limit the information in the lsp hover output. Use 0 to disable the limit" int lsp_hover_max_info_lines 20
declare-option -hidden -docstring "DEPRECATED, use %opt{lsp_hover_max_info_lines}. Set it to a positive number to limit the information in the lsp hover output. Use 0 to disable the limit. Use -1 to use lsp_hover_max_info_lines instead." int lsp_hover_max_lines -1
declare-option -docstring "Set it to a positive number to limit the diagnostics in the lsp hover output. Use 0 to disable the limit" int lsp_hover_max_diagnostic_lines 20

declare-option -hidden -docstring "DEPRECATED, use %opt{lsp_servers}. TOML table with server-specific settings. Must declare [language_server.<filetype>.settings]" str lsp_config
declare-option -hidden -docstring "DEPRECATED, use %opt{lsp_servers}. Configuration to send in workspace/didChangeConfiguration messages" str-to-str-map lsp_server_configuration
declare-option -hidden -docstring "DEPRECATED, use %opt{lsp_servers}. Configuration to send in initializationOptions of Initialize messages." str-to-str-map lsp_server_initialization_options
# Line flags for inline diagnostics.
declare-option -docstring "Character to signal an error in the gutter" str lsp_diagnostic_line_error_sign '*'
declare-option -docstring "Character to signal a hint in the gutter" str lsp_diagnostic_line_hint_sign '-'
declare-option -docstring "Character to signal an info in the gutter" str lsp_diagnostic_line_info_sign 'i'
declare-option -docstring "Character to signal a warning in the gutter" str lsp_diagnostic_line_warning_sign '!'
declare-option -docstring "Character to signal a code lens in the gutter" str lsp_code_lens_sign '>'
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
info=$lsp_info \
    diagnostics=$lsp_diagnostics \
    code_lenses=$lsp_code_lenses \
    awk 'BEGIN {
        max_info_lines = ENVIRON["kak_opt_lsp_hover_max_lines"]

        # If lsp_hover_max_info_lines is a sentinel value (e.g. -1) then it is
        # likely the user has not set the value themselves, use the value of the
        # new lsp_hover_max_info_lines.
        if (max_info_lines < 0)
            max_info_lines = ENVIRON["kak_opt_lsp_hover_max_info_lines"]

        info_lines = split(ENVIRON["info"], info_line, /\n/)

        info_truncated = 0

        # Will the info lines need to be truncated
        if (is_truncated(max_info_lines, info_lines)) {
            # Only output max_info_lines amount of info lines
            info_lines = max_info_lines
            info_truncated = 1
        }

        if (info_lines) {
            print_at_least_one_line(info_line, info_lines)
        }

        diagnostics = ENVIRON["diagnostics"]

        diagnostics_truncated = 0

        if (diagnostics) {
            print "{+b@InfoDefault}Diagnostics{InfoDefault} (shortcut e):"

            max_diagnostic_lines = ENVIRON["kak_opt_lsp_hover_max_diagnostic_lines"]

            diagnostic_lines = split(diagnostics, diagnostic_line, /\n/)

            # Will the diagnostic lines need to be truncated
            if (is_truncated(max_diagnostic_lines, diagnostic_lines)) {
                # Only output max_diagnostic_lines amount of diagnostic lines
                diagnostic_lines = max_diagnostic_lines
                diagnostics_truncated = 1
            }

            if (diagnostic_lines) {
                print_at_least_one_line(diagnostic_line, diagnostic_lines)
            }
        }

        if (info_truncated == 1 || diagnostics_truncated == 1)
            print "{+i@InfoDefault}Hover info truncated, use lsp-hover-buffer (shortcut H) for full hover info"
        if (ENVIRON["code_lenses"])
            print "Code Lenses available (shortcut l)"
        if (ENVIRON["kak_opt_lsp_modeline_code_actions"])
            print "Code Actions available (shortcut a)"
        if (ENVIRON["kak_opt_lsp_modeline_message_requests"])
            print "There are unread messages (use lsp-show-message-request-next to read)"
    }

    function print_at_least_one_line(data, lines) {
        for (i = 1; i <= lines; i++) {
            print data[i]
        }
    }

    function is_truncated(max, lines) {
        return max > 0 && lines > max
    }'
}
# If you want to see only hover info, try
# set-option global lsp_show_hover_format 'printf %s "${lsp_info}"'

# Callback functions. May override these.

declare-option -hidden str lsp_code_action_indicator

define-command -hidden lsp-show-code-actions -params 1.. -docstring "Called when code actions are available for the main cursor position" %{
    set-option buffer lsp_modeline_code_actions %opt{lsp_code_action_indicator}
}

define-command -hidden lsp-hide-code-actions -docstring "Called when no code action is available for the main cursor position" %{
    set-option buffer lsp_modeline_code_actions ""
}

define-command -hidden lsp-perform-code-action -params 1.. -docstring "Called on :lsp-code-actions" %{
    lsp-menu %arg{@}
}

define-command -hidden lsp-perform-code-lens -params 1.. -docstring "Called on :lsp-code-lens" %{
    lsp-menu %arg{@}
}

# stdlib backports
define-command -hidden lsp-menu -params 1.. %{
    evaluate-commands -save-regs a %{
        set-register a %arg{@}
        lsp-menu-impl
    }
}
define-command -hidden lsp-menu-impl %{
    evaluate-commands %sh{
        if ! command -v perl > /dev/null; then
            echo "lsp-show-error %{'perl' must be installed to use the 'lsp-menu' command}"
            exit
        fi
        echo >$kak_command_fifo "echo -to-file $kak_response_fifo -quoting kakoune -- %reg{a}"
        perl < $kak_response_fifo -we '
            use strict;
            my $Q = "'\''";
            my @args = ();
            {
                my $arg = undef;
                my $prev_is_quote = 0;
                my $state = "before-arg";
                while (not eof(STDIN)) {
                    my $c = getc(STDIN);
                    if ($state eq "before-arg") {
                        ($c eq $Q) or die "bad char: $c";
                        $state = "in-arg";
                        $arg = "";
                    } elsif ($state eq "in-arg") {
                        if ($prev_is_quote) {
                            $prev_is_quote = 0;
                            if ($c eq $Q) {
                                $arg .= $Q;
                                next;
                            }
                            ($c eq " ") or die "bad char: $c";
                            push @args, $arg;
                            $state = "before-arg";
                            next;
                        } elsif ($c eq $Q) {
                            $prev_is_quote = 1;
                            next;
                        }
                        $arg .= $c;
                    }
                }
                ($state eq "in-arg") or die "expected $Q as last char";
                push @args, $arg;
            }

            my $auto_single = 0;
            my $select_cmds = 0;
            my $on_abort = "";
            while (defined $args[0] and $args[0] =~ m/^-/) {
                if ($args[0] eq "--") {
                    shift @args;
                    last;
                }
                if ($args[0] eq "-auto-single") {
                    $auto_single = 1;
                }
                if ($args[0] eq "-select-cmds") {
                    $select_cmds = 1;
                }
                if ($args[0] eq "-on-abort") {
                    if (not defined $args[1]) {
                        print "fail %{menu: missing argument to -on-abort}";
                        exit;
                    }
                    $on_abort = $args[1];
                    shift @args;
                }
                shift @args;
            }
            my $stride = 2 + $select_cmds;
            if (scalar @args == 0 or scalar @args % $stride != 0) {
                print "fail %{menu: wrong argument count}";
                exit;
            }
            if ($auto_single and scalar @args == $stride) {
                print $args[1];
                exit;
            }

            sub shellquote {
                my $arg = shift;
                $arg =~ s/$Q/$Q\\$Q$Q/g;
                return "$Q$arg$Q";
            }
            sub kakquote {
                my $arg = shift;
                $arg =~ s/$Q/$Q$Q/g;
                return "$Q$arg$Q";
            }

            my $accept_cases = "";
            my $select_cases = "";
            my $completions = "";
            sub case_clause {
                my $name = shellquote shift;
                my $command = shellquote shift;
                return "($name)\n"
                     . " printf \"%s\n\" $command ;;\n";
            }
            for (my $i = 0; $i < scalar @args; $i += $stride) {
                my $name = $args[$i];
                my $command = $args[$i+1];
                $accept_cases .= case_clause $name, $command;
                $select_cases .= case_clause $name, $args[$i+2] if $select_cmds;
                $completions .= "$name\n";
            }
            use File::Temp qw(tempdir);
            my $tmpdir = tempdir;
            sub put {
                my $name = shift;
                my $contents = shift;
                my $filename = "$tmpdir/$name";
                open my $fh, ">", "$filename" or die "failed to open $filename: $!";
                print $fh $contents or die "write: $!";
                close $fh or die "close: $!";
                return $filename;
            };
            my $on_accept = put "on-accept",
                "case \"\$kak_text\" in\n" .
                "$accept_cases" .
                "(*) echo fail -- no such item: \"$Q\$(printf %s \"\$kak_text\" | sed \"s/$Q/$Q$Q/g\")$Q\";\n" .
                "esac\n";
            my $on_change = put "on-change",
                "case \"\$kak_text\" in\n" .
                "$select_cases" .
                "esac\n";
            my $shell_script_candidates = put "shell-script-candidates", $completions;

            print "prompt %{} %{ evaluate-commands %sh{. $on_accept kak_text; rm -r $tmpdir} }";
            print  " -on-abort " . kakquote "nop %sh{rm -r $tmpdir}; $on_abort";
            if ($select_cmds) {
                print " -on-change %{ evaluate-commands %sh{. $on_change kak_text} }";
            }
            print " -menu -shell-script-candidates %{cat $shell_script_candidates}";
        ' ||
            echo 'lsp-show-error %{lsp-menu: encountered an error, see *debug* buffer}';
    }
}
define-command -hidden lsp-with-option -params 3.. -docstring %{
    lsp-with-option <option_name> <new_value> <command> [<arguments>]: evaluate a command with a modified option
} %{
    evaluate-commands -save-regs s %{
        evaluate-commands set-register s "%%opt{%arg{1}}"
        set-option current %arg{1} %arg{2}
        try %{
            evaluate-commands %sh{
                shift 2
                for arg
                do
                    printf "'%s' " "$(printf %s "$arg" | sed "s/'/''/g")"
                done
            }
        } catch %{
            set-option current %arg{1} %reg{s}
            fail "lsp-with-option: %val{error}"
        }
        set-option current %arg{1} %reg{s}
    }
}

# Options for information exposed by kakoune-lsp.

# Count of diagnostics published for the current buffer.
declare-option -docstring "Number of errors" int lsp_diagnostic_error_count 0
declare-option -docstring "Number of hints" int lsp_diagnostic_hint_count 0
declare-option -docstring "Number of infos" int lsp_diagnostic_info_count 0
declare-option -docstring "Number of warnings" int lsp_diagnostic_warning_count 0

# Internal variables.

declare-option -hidden completions lsp_completions
declare-option -hidden int lsp_completions_timestamp -1
declare-option -hidden int lsp_completions_selected_item
declare-option -hidden range-specs lsp_inline_diagnostics
declare-option -hidden range-specs lsp_inline_diagnostics_deprecated
declare-option -hidden range-specs lsp_inline_diagnostics_unnecessary
declare-option -hidden line-specs lsp_diagnostic_lines 0 '0| '
declare-option -hidden line-specs lsp_inlay_diagnostics
declare-option -hidden range-specs cquery_semhl
declare-option -hidden range-specs lsp_references
declare-option -hidden range-specs lsp_semantic_tokens_ranges
declare-option -hidden range-specs lsp_inlay_hints
declare-option -hidden line-specs lsp_inlay_code_lenses
declare-option -hidden line-specs lsp_code_lenses 0 '0| '
declare-option -hidden str lsp_project_root
declare-option -hidden str lsp_buffile

declare-option -hidden str lsp_modeline_breadcrumbs ""
declare-option -hidden str lsp_modeline_code_actions
declare-option -hidden str lsp_modeline_progress ""
declare-option -hidden str lsp_modeline_message_requests ""
declare-option -hidden str lsp_modeline '%opt{lsp_modeline_breadcrumbs}%opt{lsp_modeline_code_actions}%opt{lsp_modeline_progress}%opt{lsp_modeline_message_requests}'
set-option global modelinefmt "%opt{lsp_modeline} %opt{modelinefmt}"

hook -group lsp-option-changed global GlobalSetOption lsp_debug=.* %{
    try %{ lsp-send kakoune/did-change-option %val{hook_param} }
}

declare-option -hidden -docstring %{
    echo %sh{eval "$kak_opt_lsp_find_root" <globs>... $(: kak_buffile)}: detect root directory based on the given shell globs.

    Prints the first among <globs> that matches in a parent directory of the current buffer's file.
} str lsp_find_root %{
    find_root() {
        project_root=$(dirname "$kak_buffile")
        if [ "$project_root" = . ]; then
            exit # Ignore scratch buffers.
        fi
        until cd "$project_root" 2>/dev/null; do
            project_root=${project_root%/*}
            project_root=/${project_root#/}
        done
        for glob; do
            until [ "$(pwd -P)" = / ]; do
                set -- $glob
                if [ -e "$1" ]; then
                    break 2
                fi
                cd ..
            done
            cd "$project_root"
        done
        printf %s "$PWD" | sed 's/\\/\\\\/g; s/"/\\"/g'
    }
    find_root}

define-command -hidden lsp-if-no-servers -docstring %{
    Run the given command if the 'lsp_servers' option is empty
} -params 1.. %{
    evaluate-commands -save-regs a %{
        try %{
            "lsp-nop-with-0%opt{lsp_servers}"
            set-register a %arg{@}
        } catch %{
            set-register a nop
        }
        %reg{a}
    }
}

### Requests ###

declare-option -hidden -docstring 'Acceptor for LSP commands' str lsp_fifo
declare-option -hidden -docstring 'PID file for kak-lsp server' str lsp_pid_file

define-command lsp-start -docstring "Start kakoune-lsp session" %{
    evaluate-commands %sh{
        if [ -e "${kak_opt_lsp_fifo}" ]; then
            printf 'fail lsp-start: fifo already exists at %s\n' "${kak_opt_lsp_fifo}"
            exit
        fi
        if [ -e "${kak_opt_lsp_pid_file}" ]; then
            printf 'fail lsp-start: PID file already exists at %s\n' "${kak_opt_lsp_pid_file}"
            exit
        fi
        # kak_session
        # kak_client (for reporting startup errors)
        # kak_opt_lsp_debug
        # kak_opt_lsp_timeout
        # kak_opt_lsp_snippet_support
        # kak_opt_lsp_file_watch_support
        fifo=$(eval "${kak_opt_lsp_cmd} --daemonize")
        echo set-option global lsp_fifo ${fifo}
        echo set-option global lsp_pid_file ${fifo}.pid
    }
}

define-command -hidden lsp-send -params 1.. %{
    lsp-unless-blocked lsp-do-send %arg{@}
}

define-command -hidden lsp-do-send -params 1.. %{
    %opt{lsp_fail_if_disabled} "lsp-send: run lsp-enable or lsp-enable-window first"
    try %{
        evaluate-commands "nop %%file{%opt{lsp_pid_file}}"
    } catch %{
        lsp-start
    }
    evaluate-commands -save-regs abhst %{
        try %{
            nop %val{hook_param}
            set-register h true
        } catch %{
            set-register h false
        }
        try %{
            nop %val{source}
            set-register s true
        } catch %{
            set-register s false
        }
        # KakEnd has no buffer in context
        try %{
            set-register b %val{buffile}
        } catch %{
            set-register b ''
        }
        try %{
            set-register t %val{timestamp}
        } catch %{
            set-register t 0
        }
        set-register a \
            %val{session} \
            %val{client} \
            %reg{h} \
            %reg{s} \
            %reg{b} \
            %reg{t} \
            %opt{filetype} \
            %opt{lsp_language_id} \
            %opt{lsp_servers} \
            %opt{lsp_semantic_tokens} \
            %arg{@}
        %opt{lsp_do_send_maybe_async}
    }
}

declare-option -hidden str lsp_do_send_maybe_async lsp-do-send-async

define-command -hidden lsp-do-send-async %{
    evaluate-commands %sh{
        $(command -v timeout 2>/dev/null && echo 1.5) sh -c '
            exec >${kak_opt_lsp_fifo}
            trap : INT TERM
            printf %s "${kak_quoted_reg_a}"
        '
        if [ $? -eq 124 ]; then
            echo fail "Timed out trying to reach kak-lsp"
        fi
    }
}

define-command -hidden lsp-do-send-sync %{
    unset-option buffer lsp_do_send_maybe_async
    evaluate-commands %sh{
        $(command -v timeout 2>/dev/null && echo 1.5) sh -c '
            exec >${kak_opt_lsp_fifo}
            trap : INT TERM
            printf %s "${kak_quoted_reg_a}"
            printf %s " '${kak_command_fifo}' '${kak_response_fifo}'"
        '
        if [ $? -eq 124 ]; then
            echo >${kak_command_fifo} "fail Timed out trying to reach kak-lsp"
            echo fail "Timed out trying to reach kak-lsp"
            exit
        fi
        cat ${kak_response_fifo}
    }
}

declare-option -hidden str-list lsp_unless_blocked

define-command -hidden lsp-unless-blocked -params 1.. %{
    try %{
        # Extra optimization for a common case: if we're in a file with no associated language
        # server, we can simply ignore all requests that come from hooks. Testing for a file
        # is important to still allow KakEnd hooks.
        nop %val{buffile} %val{hook_param}
        "lsp-nop-with-0%opt{lsp_servers}"
    } catch %{
        %opt{lsp_unless_blocked} %arg{@}
    }
}

define-command -hidden lsp-blocked -params .. %{
    try %{
        nop %val{hook_param}
    } catch %{
        %arg{@}
    }
}

define-command -hidden lsp-block-in-buffer %{
    set-option buffer lsp_unless_blocked lsp-blocked
}

define-command -hidden lsp-unblock-in-buffer %{
    unset-option buffer lsp_unless_blocked
}

try %{ evaluate-commands -buffer *debug* lsp-block-in-buffer }
hook -group lsp-scratch-buffers global BufCreate [^/].* %{
    lsp-block-in-buffer
}
hook -group lsp-scratch-buffers global WinDisplay \*debug\* %{
    lsp-block-in-buffer
}

declare-option -hidden int lsp_timestamp -1
define-command -hidden lsp-nop-with-0 nop
define-command -hidden lsp-if-changed-since -params 3 -docstring %{
    lsp-if-changed-since <option_name> <option_value> <commands>
} %{
    declare-option -hidden int lsp_elapsed_time
    set-option buffer lsp_elapsed_time %val{timestamp}
    set-option -remove buffer lsp_elapsed_time %arg{2}
    try %{
        evaluate-commands "lsp-nop-with-%opt{lsp_elapsed_time}"
    } catch %{
        set-option buffer %arg{1} %val{timestamp}
        evaluate-commands %arg{3}
    }
}

define-command -hidden lsp-did-change -docstring "Notify language server about buffer change" %{
    lsp-unless-blocked lsp-if-changed-since lsp_timestamp %opt{lsp_timestamp} %{
        evaluate-commands %sh{
            file=$(mktemp -q -t 'kak-lsp-buffer.XXXXXX' 2>/dev/null || mktemp -q)
            echo "
                evaluate-commands -no-hooks %{ write -force $file }
                lsp-send textDocument/didChange $file
            "
        }
    }
}

define-command -hidden lsp-completion -docstring "Request completions for the main cursor position" %{
    try %{
        # Fail if preceding character is a whitespace (by default; the trigger could be customized).
        evaluate-commands -draft %opt{lsp_completion_trigger}

        # Kakoune requires completions to point fragment start rather than cursor position.
        # We try to detect it and put into lsp_completion_offset and then pass via completion.offset
        # parameter to the kakoune-lsp server so it can use it when sending completions back.
        declare-option -hidden str lsp_completion_offset

        set-option window lsp_completion_offset %val{cursor_column}
        evaluate-commands -draft %{
            try %{
                evaluate-commands %opt{lsp_completion_fragment_start}
                set-option window lsp_completion_offset %val{cursor_column}
            }
        }

        lsp-send textDocument/completion %val{cursor_line} %val{cursor_column} \
            %opt{lsp_completion_offset}
    }
}

declare-option -hidden str-list lsp_completion_inserted_ranges

define-command -hidden lsp-completion-accepted -docstring "Called when a completion is accepted" %{
    evaluate-commands set-option window lsp_completion_inserted_ranges %val{hook_param}
    trigger-user-hook LSPCompletionAccepted
    remove-hooks window lsp-completion-accepted
}

define-command -hidden lsp-completion-on-accept -params 1 -docstring %{
    lsp-completion-on-accept <command>: run <command> when the completion menu is closed

    The inserted range is available as %opt{lsp_completion_inserted_ranges} in the format expected by "select".
} %{
    hook -once -group lsp-completion-accepted window User LSPCompletionAccepted %arg{1}
}

# Is called when a completion item is selected
define-command -hidden lsp-completion-item-selected -params 1 %{
    set-option window lsp_completions_selected_item %arg{1}
    remove-hooks window lsp-completion-accepted
}

# Call the resolve request for the current completion, and queue up the closing request on dismiss
define-command -hidden lsp-completion-item-resolve %{
    lsp-completion-item-resolve-request true
    lsp-completion-on-accept %{
        lsp-completion-item-resolve-request false
    }
}

define-command -hidden lsp-completion-item-resolve-request -params 1 \
    -docstring "Request additional attributes for a completion item" %{
    lsp-send completionItem/resolve %opt{lsp_completions_timestamp} \
        %opt{lsp_completions_selected_item} %arg{1} # pager active
}

define-command lsp-hover -docstring "Request hover info for the main cursor position" %{
    lsp-hover-request
}

define-command lsp-hover-buffer -params 0..1 -client-completion \
    -docstring "lsp-hover-buffer [<client>]: request hover info for the main cursor position in a scratch buffer.

The hover buffer is activated in the given client, or the client referred to by the 'docsclient' option, or the current client.
" %{
    lsp-hover-request %sh{echo ${1-"${kak_opt_docsclient:-"$kak_client"}"}}
}

define-command -hidden lsp-hover-request -params 0..1 -docstring "Request hover info for the main cursor position" %{
    lsp-send textDocument/hover %val{selection_desc} %opt{tabstop} \
        %arg{1} # optional hover client
}

declare-option -hidden str lsp_symbol_kind_completion %{
    symbol_kinds="\
        Array
        Boolean
        Class
        Constant
        Constructor
        Enum
        EnumMember
        Event
        Field
        File
        Function
        Interface
        Key
        Method
        Module
        Namespace
        Null
        Number
        Object
        Operator
        Package
        Property
        String
        Struct
        TypeParameter
        Variable
    "
    printf '%s\n' ${symbol_kinds}
}

define-command lsp-previous-symbol -params 0.. -shell-script-candidates %opt{lsp_symbol_kind_completion} \
    -docstring "lsp-previous-symbol [<symbol-kinds>...]: goto the buffer's previous symbol of a type in <symbol-kinds>, or of any type" %{
    lsp-next-or-previous-symbol previous goto %arg{@}
}

define-command lsp-next-symbol -params 0.. -shell-script-candidates %opt{lsp_symbol_kind_completion} \
    -docstring "lsp-next-symbol [<symbol-kinds>...]: goto the buffer's next symbol of a type in <symbol-kinds>, or of any type" %{
    lsp-next-or-previous-symbol next goto %arg{@}
}

define-command lsp-hover-previous-symbol -params 0.. -shell-script-candidates %opt{lsp_symbol_kind_completion} \
    -docstring "lsp-hover-previous-symbol [<symbol-kinds>...]: show hover of the buffer's current or previous symbol of a type in <symbol-kinds>, or of any type" %{
    lsp-next-or-previous-symbol previous hover %arg{@}
}

define-command lsp-hover-next-symbol -params 0.. -shell-script-candidates %opt{lsp_symbol_kind_completion} \
    -docstring "lsp-hover-next-symbol [<symbol-kinds>...]: show hover of the buffer's next symbol of a type in <symbol-kinds>, or of any type" %{
    lsp-next-or-previous-symbol next hover %arg{@}
}

# Requests for hover/goto next/previous symbol are funneled through this command
define-command lsp-next-or-previous-symbol -hidden -params 2.. %{
    lsp-send kakoune/next-or-previous-symbol \
        %sh{echo $(( $# - 2 ))} \
        %val{cursor_line} %val{cursor_column} \
        %arg{@} # next/previous hover/goto [symbol-kinds...]
} -shell-script-candidates %{
    case $# in
        # Search forward or backward?
        (1) printf '%s\n' next previous ;;
        # Show hover info or goto symbol?
        (2) printf '%s\n' hover goto ;;
        # Which symbol types?
        (*) eval "$kak_opt_lsp_symbol_kind_completion" ;;
    esac
}

## Convenience methods

define-command lsp-hover-next-function -docstring "Show hover of the next function in the buffer" %{
     lsp-hover-next-symbol Method Function
}

define-command lsp-hover-previous-function -docstring "Show hover of the current or previous function in the buffer" %{
     lsp-hover-previous-symbol Method Function
}

define-command lsp-next-function -docstring "Goto the next function in the buffer" %{
    lsp-next-symbol Method Function
}

define-command lsp-previous-function -docstring "Goto the current or previous function in the buffer" %{
    lsp-previous-symbol Method Function
}

declare-option -hidden str lsp_object_mode
define-command lsp-object -params .. -shell-script-candidates %opt{lsp_symbol_kind_completion} \
    -docstring "lsp-object [<symbol-kinds>...]: select adjacent or surrounding symbol of a type in <symbol-kinds>, or of any type

This must be run from object mode
https://github.com/mawww/kakoune/blob/master/doc/pages/modes.asciidoc#object-mode

See also the 'lsp-next-*' and 'lsp-previous-*' commands.
" %{
    lsp-send kakoune/object %val{count} %opt{lsp_object_mode} \
        %val{selection_count} %val{selections_desc} \
        %sh{echo $(( $# ))} %arg{@} # symbol-kinds
}

define-command -hidden lsp-get-word-regex %{
    try %{
        execute-keys "<space><esc>,<esc><a-i>c\A|[^\w%opt{lsp_extra_word_chars}],\z|[^\w%opt{lsp_extra_word_chars}]<ret>"
        execute-keys %{"a*}
    } catch %{
        set-register a %{}
    }
}

define-command lsp-definition -docstring "Go to definition" %{
    evaluate-commands -draft -save-regs a %{
        lsp-get-word-regex
        lsp-send textDocument/definition %reg{a} %val{cursor_line} %val{cursor_column}
    }
}

define-command lsp-declaration -docstring "Go to declaration" %{
    lsp-send textDocument/declaration %val{cursor_line} %val{cursor_column}
}

define-command lsp-implementation -docstring "Go to implementation" %{
    lsp-send textDocument/implementation %val{cursor_line} %val{cursor_column}
}

define-command lsp-type-definition -docstring "Go to type-definition" %{
    lsp-send textDocument/typeDefinition %val{cursor_line} %val{cursor_column}
}

define-command lsp-code-actions -params 0.. -docstring %{
    lsp-code-actions [-auto-single] [<code-action-kinds>...]: Perform code actions for the main cursor position

    If <code-action-kinds> is given, only show matching code actions.
    With -auto-single instantly validate if only one code action is available.
} %{
    lsp-code-actions-request true is-async only %arg{@}
} -shell-script-candidates %{
cat <<EOF
-auto-single
quickfix
refactor
refactor.extract
refactor.inline
refactor.rewrite
refactor.rewrite
source
source.fixAll
source.organizeImports
EOF
}

define-command lsp-code-actions-sync -params 0.. -docstring %{
    lsp-code-actions-sync [<code-action-kinds>...]: Perform the matching code action for the main cursor position, blocking Kakoune session until done.
} %{
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-code-actions-request true is-sync only %arg{@}
} -shell-script-candidates %{
cat <<EOF
quickfix
refactor
refactor.extract
refactor.inline
refactor.rewrite
refactor.rewrite
source
source.fixAll
source.organizeImports
EOF
}

define-command -hidden lsp-code-action -params 1 -docstring "DEPRECATED lsp-code-action <pattern>: perform the code action that matches the given regex" %{
    lsp-code-actions-request true is-async matching %arg{1}
}

define-command -hidden lsp-code-action-sync -params 1 -docstring "DEPRECATED lsp-code-action-sync <pattern>: perform the code action that matches the given regex, blocking Kakoune session until done" %{
    lsp-did-change
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-code-actions-request true is-sync matching %arg{1}
}

define-command -hidden lsp-code-actions-request -params 3.. -docstring "Request code actions for the main cursor position" %{
    lsp-send textDocument/codeAction %val{selection_desc} \
        %sh{echo $(( $# - 3 ))} \
        %arg{@} # do_perform sync filter-mode [pattern | kind...]
}

define-command -hidden lsp-code-actions-background-request %{
    lsp-send textDocument/codeAction %val{selection_desc} \
        0 \
        false false only # do_perform sync filter-mode
}

define-command -hidden lsp-code-action-resolve-request -params 1 \
    -docstring "Request additional attributes for a code action" %{
    lsp-send codeAction/resolve %arg{1} # code-action
}

define-command lsp-code-lens -docstring "apply a code lens from the current selection" %{
    lsp-send kakoune/textDocument/codeLens %val{selection_desc}
}

define-command -hidden lsp-code-lens-request %{
    declare-option -hidden int lsp_code_lens_timestamp -1
    lsp-if-changed-since lsp_code_lens_timestamp %opt{lsp_code_lens_timestamp} %{
        lsp-send textDocument/codeLens
    }
}

define-command lsp-execute-command -params 2 -docstring "lsp-execute-command <command> <args>: execute a server-specific command" %{
    lsp-execute-command-request is-async %arg{@}
}

define-command -hidden lsp-execute-command-sync -params 2 -docstring "lsp-execute-command <command> <args>: execute a server-specific command, blocking Kakoune session until done" %{
    lsp-did-change
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-execute-command-request is-sync %arg{@}
}
define-command -hidden lsp-execute-command-request -params 3 %{
    lsp-send workspace/executeCommand %arg{1} %arg{2} %arg{3} # sync command arguments
}

define-command lsp-references -docstring "Open buffer with symbol references" %{
    evaluate-commands -draft -save-regs a %{
        lsp-get-word-regex
        lsp-send textDocument/references %reg{a} %val{cursor_line} %val{cursor_column}
    }
}

define-command lsp-highlight-references -docstring "Highlight symbol references" %{
    try %{
        nop %val{hook_param}
        lsp-send textDocument/documentHighlight %{} %val{cursor_line} %val{cursor_column}
    } catch %{
        evaluate-commands -draft -save-regs a %{
            lsp-get-word-regex
            lsp-send textDocument/documentHighlight %reg{a} %val{cursor_line} %val{cursor_column}
        }
    }
}

define-command lsp-rename -params 1 -docstring "lsp-rename <new-name>: rename symbol under the main cursor" %{
    lsp-send textDocument/rename %val{cursor_line} %val{cursor_column} %arg{1} # new name
}

define-command lsp-rename-prompt -docstring "Rename symbol under the main cursor (prompt for a new name)" %{
    evaluate-commands -save-regs ^s %{
        execute-keys -save-regs "" Z
        try %{
            execute-keys <space><esc>,<esc><a-i>w
            # include a leading single-quote for Rust lifetime specifiers
            execute-keys <a-semicolon>Hs'?\w+<ret><a-semicolon>
        } catch %{
            fail "lsp-rename-prompt: no identifier at cursor"
        }
        set-register s %val{selection}
        execute-keys z
        prompt -init %reg{s} 'New name: ' %{ lsp-rename %val{text} }
    }
}

define-command lsp-selection-range -params 0..1 \
    -docstring "lsp-selection-range [cached]: select interesting ranges around each selection

If cached is given, reuse the ranges from a previous invocation." %{
    evaluate-commands %sh{
        if [ "$1" = cached ] && [ $kak_opt_lsp_selection_range_selected -ne 0 ]; then
            echo "lsp-selection-range-select $kak_opt_lsp_selection_range_selected"
            echo "lsp-selection-range-show"
            exit
        fi
        echo 'lsp-send textDocument/selectionRange %val{cursor_line} %val{cursor_column} \
                %val{selection_count} %val{selections_desc}'
    }
}

declare-option -hidden str-list lsp_selection_ranges
declare-option -hidden int lsp_selection_range_selected
define-command -hidden lsp-selection-range-show %{
    evaluate-commands %sh{
        eval set -- "$kak_quoted_opt_lsp_selection_ranges"
        if [ $# -eq 1 ]; then
            echo "info 'lsp-selection-range: no parent node'"
        else
            echo "enter-user-mode -lock lsp-selection-range"
        fi
    }
}

define-command lsp-selection-range-select -params 1 \
    -docstring "lsp-selection-range [up|down|top|bottom|<n>]: select parent or child range

A numeric argument selects by absolute index, where 1 is the innermost child" %{
    evaluate-commands %sh{
        arg=$1
        eval set -- "$kak_quoted_opt_lsp_selection_ranges"
        index=$kak_opt_lsp_selection_range_selected
        case "$arg" in
            (up) [ $index -lt $# ] && index=$(($index + 1)) ;;
            (down) [ $index -gt 1 ] && index=$(($index - 1)) ;;
            (top) index=$# ;;
            (bottom) index=1 ;;
            (0) echo "fail lsp-selection-range-select: invalid argument" ;;
            (*) [ $arg -lt $# ] && index=$arg || index=$# ;;
        esac
        echo "set-option window lsp_selection_range_selected $index"
        eval echo select \$\{$index\}
    }
}

define-command lsp-signature-help -docstring "Request signature help for the main cursor position" %{
    lsp-send textDocument/signatureHelp %val{cursor_line} %val{cursor_column}
}

define-command lsp-diagnostics -docstring "Open buffer with project-wide diagnostics for current filetype" %{
    lsp-send textDocument/diagnostics
}

define-command lsp-document-symbol -docstring "Open buffer with document symbols" %{
    lsp-send textDocument/documentSymbol
}

define-command lsp-goto-document-symbol -params 0..1 -docstring "lsp-goto-document-symbol [<name>]: pick a symbol from current buffer to jump to

If <name> is given, jump to the symbol of that name." %{
    lsp-send kakoune/goto-document-symbol %arg{1} # symbol name
}

define-command -hidden lsp-workspace-symbol-buffer -params 3 -docstring %{
    buffile timestamp query
    Open buffer with a list of project-wide symbols matching the query
    on behalf of the buffile at timestamp
} %{ try %{
    evaluate-commands %sh{
        if [ -z "${3}" ];
        then echo "fail"
        fi
    }
    lsp-send workspace/symbol \
        %arg{1} %arg{2} %arg{3} # buffile version query
}}

define-command lsp-capabilities -docstring "List available commands for current filetype" %{
    lsp-send capabilities
}

define-command -hidden lsp-did-open %{
    lsp-unless-blocked evaluate-commands %{
        evaluate-commands %sh{
            file=$(mktemp -q -t 'kak-lsp-buffer.XXXXXX' 2>/dev/null || mktemp -q)
            echo "
                evaluate-commands -no-hooks %{ write -force $file }
                lsp-send textDocument/didOpen $file
            "
        }
        set-option buffer lsp_timestamp %val{timestamp}
        lsp-code-lens-request
    }
}

define-command -hidden lsp-did-close %{
    lsp-send textDocument/didClose
}

define-command -hidden lsp-did-save %{
    lsp-send textDocument/didSave
}

define-command -hidden lsp-did-change-config %{
    lsp-send workspace/didChangeConfiguration %opt{lsp_config} \
        %opt{lsp_server_configuration} map-end
}

define-command -hidden lsp-exit -params 0..1 -docstring %{
    lsp-exit: shutdown language servers associated with current editor session
} %{
    lsp-send exit
}

define-command lsp-restart -docstring "Restart kak-lsp and language servers" %{
    lsp-exit
    evaluate-commands %sh{
        for attempt in $(seq 30); do
            if ! [ -e ${kak_opt_lsp_pid_file} ] && ! [ -e ${kak_opt_lsp_fifo} ]; then
                exit
            fi
            sleep .1
        done
        echo "fail lsp-restart: timed out waiting for kak-lsp to exit"
    }
    lsp-did-change-config
    lsp-did-open
    echo -markup {Information}Restarted LSP servers
}

define-command lsp-cancel-progress -params 1 -docstring "lsp-cancel-progress <token>: cancel a cancelable progress item." %{
    lsp-send window/workDoneProgress/cancel %arg{1} # token
}

define-command lsp-apply-workspace-edit -params 1 -hidden %{
    lsp-apply-workspace-edit-request is-async %arg{1}
}
define-command lsp-apply-workspace-edit-sync -params 1 -hidden %{
    lsp-did-change
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-apply-workspace-edit-request is-sync %arg{1}
}
define-command lsp-apply-workspace-edit-request -params 2 -hidden %{
    lsp-send apply-workspace-edit %arg{1} %arg{2} # sync edit
}

define-command lsp-formatting -params 0..1 -docstring "lsp-formatting [<server_name>]: format document" %{
    lsp-formatting-request is-async %arg{1}
}

define-command lsp-formatting-sync -params 0..1 -docstring "lsp-formatting-sync [<server_name>]: format document, blocking Kakoune session until done" %{
    lsp-did-change
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-formatting-request is-sync %arg{1}
}

define-command -hidden lsp-formatting-request -params 1..2 %{
    lsp-send textDocument/formatting %opt{tabstop} %opt{lsp_insert_spaces} \
        %arg{1} %arg{2} # sync [server]
}

define-command lsp-range-formatting -params 0..1 -docstring "lsp-range-formatting [<server_name>]: format selections" %{
    lsp-range-formatting-request is-async %arg{1}
}

define-command lsp-range-formatting-sync -params 0..1 -docstring "lsp-range-formatting-sync [<server_name>]: format selections, blocking Kakoune session until done" %{
    lsp-did-change
    set-option buffer lsp_do_send_maybe_async lsp-do-send-sync
    lsp-range-formatting-request is-sync %arg{1}
}

define-command -hidden lsp-range-formatting-request -params 1..2 %{
    lsp-send textDocument/rangeFormatting %opt{tabstop} %opt{lsp_insert_spaces} \
        %val{selection_count} %val{selections_desc} \
        %arg{1} %arg{2} # sync [server]
}

define-command lsp-incoming-calls -docstring "Open buffer with calls to the function at the main cursor position" %{
    lsp-call-hierarchy-request true
}

define-command lsp-outgoing-calls -docstring "Open buffer with calls by the function at the main cursor position" %{
    lsp-call-hierarchy-request false
}

define-command -hidden lsp-call-hierarchy-request -params 1 %{
    lsp-send textDocument/prepareCallHierarchy %val{cursor_line} %val{cursor_column} \
        %arg{1} # incoming-or-outgoing
}

define-command -hidden lsp-breadcrumbs-request -docstring "request updating modeline breadcrumbs for the window" %{
    lsp-send kakoune/breadcrumbs %val{cursor_line}
}

define-command -hidden lsp-inlay-hints -docstring "lsp-inlay-hints: request inlay hints" %{
    declare-option -hidden int lsp_inlay_hints_timestamp -1
    lsp-if-changed-since lsp_inlay_hints_timestamp %opt{lsp_inlay_hints_timestamp} %{
        lsp-send textDocument/inlayHint %val{buf_line_count}
    }
}

# CCLS Extension

define-command ccls-navigate -docstring "Navigate C/C++/ObjectiveC file" -params 1 %{
    lsp-send $ccls/navigate %val{cursor_line} %val{cursor_column} %arg{1} # direction
}

define-command ccls-vars -docstring "ccls-vars: Find instances of symbol at point." %{
    lsp-send $ccls/vars %val{cursor_line} %val{cursor_column}
}

define-command ccls-inheritance -params 1..2 -docstring "ccls-inheritance <derived|base> [levels]: Find base- or derived classes of symbol at point." %{
    lsp-send $ccls/inheritance %val{cursor_line} %val{cursor_column} \
        %arg{1} %arg{2} # derived [levels]
}

define-command ccls-call -params 1 -docstring "ccls-call <caller|callee>: Find callers or callees of symbol at point." %{
    lsp-send $ccls/call %val{cursor_line} %val{cursor_column} %arg{1} # callee
}

define-command ccls-member -params 1 -docstring "ccls-member <vars|types|functions>: Find member variables/types/functions of symbol at point." %{
    lsp-send $ccls/member %val{cursor_line} %val{cursor_column} %arg{1} # kind
}

# clangd Extensions

define-command clangd-switch-source-header -docstring "clangd-switch-source-header: Switch source/header." %{
    lsp-send textDocument/switchSourceHeader
}

# eclipse.jdt.ls Extension
#
define-command ejdtls-organize-imports -docstring "ejdtls-organize-imports: Organize imports." %{
    lsp-send eclipse.jdt.ls/organizeImports
}

# rust-analyzer extensions

define-command rust-analyzer-expand-macro -docstring "Expand macro recursively" %{
    lsp-send rust-analyzer/expandMacro %val{cursor_line} %val{cursor_column}
}

define-command -hidden rust-analyzer-inlay-hints -docstring "DEPRECATED, use lsp-inlay-hints-enable. request inlay hints" %{
    lsp-inlay-hints
}

# texlab extensions

define-command texlab-forward-search -docstring "Request SyncTeX Forward Search for current line from the texlab language server

This will focus the current line in your PDF viewer, starting one if necessary.
To configure the PDF viewer, use texlab's options 'forwardSearch.executable' and 'forwardSearch.args'." %{
    declare-option -hidden str texlab_client %val{client}
    lsp-send textDocument/forwardSearch %val{cursor_line} %val{cursor_column}
}

define-command texlab-build -docstring "Ask the texlab language server to build the LaTeX document" %{
    lsp-send textDocument/build
}

# semantic tokens

define-command lsp-semantic-tokens -docstring "lsp-semantic-tokens: Request semantic tokens" %{
    declare-option -hidden int lsp_semantic_tokens_timestamp -1
    lsp-if-changed-since lsp_semantic_tokens_timestamp %opt{lsp_semantic_tokens_timestamp} %{
        lsp-send textDocument/semanticTokens/full
    }
}

### Response handling ###

# Feel free to override these commands in your config if you need to customise response handling.

define-command -hidden lsp-show-hover -params 4 -docstring %{
    lsp-show-hover <anchor> <info> <diagnostics> <code_lenses>
    Render hover info.
} %{ evaluate-commands %sh{
    lsp_info=$2
    lsp_diagnostics=$3
    lsp_code_lenses=$4
    content=$(eval "${kak_opt_lsp_show_hover_format}") # kak_opt_lsp_hover_max_lines kak_opt_lsp_hover_max_info_lines kak_opt_lsp_hover_max_diagnostic_lines kak_opt_lsp_modeline_code_actions kak_opt_lsp_modeline_message_requests
    # remove leading whitespace characters
    content="${content#"${content%%[![:space:]]*}"}"

    content=$(printf %s "$content" | sed s/\'/\'\'/g)

    case "$1" in
        modal) printf "info -markup -style modal -- '%s'" "$content";;
        *) case $kak_opt_lsp_hover_anchor in
               true) printf "info -markup -anchor %%arg{1} -- '%s'" "$content";;
               *)    printf "info -markup -- '%s'" "$content";;
           esac;;
    esac

}}

define-command -hidden lsp-show-error -params 1 -docstring "Render error" %{
    echo -debug "LSP:" %arg{1}
    info "LSP: %arg{1}"
}

define-command -hidden lsp-show-goto-buffer -params 4 %{
    evaluate-commands -save-regs '"' -try-client %opt[toolsclient] %{
        edit! -scratch %arg{1}
        set-option buffer filetype %arg{2}
        set-option buffer jump_current_line 0
        set-option buffer lsp_project_root "%arg{3}/"
        set-register '"' %arg{4}
        execute-keys Rgg
    }
}

define-command -hidden lsp-show-goto-choices -params 2 -docstring "Render goto choices" %{
    lsp-show-goto-buffer *goto* lsp-goto %arg{@}
}

define-command -hidden lsp-show-document-symbol -params 3 -docstring "Render document symbols" %{
    lsp-show-goto-buffer *goto* lsp-document-symbol %arg{1} %arg{3}
    evaluate-commands -try-client %opt[toolsclient] %{
        set-option -add buffer path %arg{1} # for gf on the file name
        set-option buffer lsp_buffile %arg{2}
    }
}

define-command -hidden lsp-show-incoming-calls -params 2 -docstring "Render callers" %{
    lsp-show-goto-buffer *callers* lsp-goto %arg{@}
}

define-command -hidden lsp-show-outgoing-calls -params 2 -docstring "Render callees" %{
    lsp-show-goto-buffer *callees* lsp-goto %arg{@}
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
        else echo 'lsp-show-goto-buffer *symbols* lsp-goto %arg{1} %arg{2}';
        fi
    }
}

define-command -hidden lsp-show-signature-help -params 2 -docstring "Render signature help" %{
    info -markup -anchor %arg{1} -style above -- %arg{2}
}

define-command -hidden lsp-show-message-error -params 2 -docstring %{
    lsp-show-message-error <message>
    Render language server message of the "error" level.
} %{
    echo -debug "LSP: error from server %arg{1}: %arg{2}"
    info "LSP: error from server %arg{1}: %arg{2}"
}

define-command -hidden lsp-show-message-warning -params 2 -docstring %{
    lsp-show-message-warning <message>
    Render language server message of the "warning" level.
} %{
    echo -debug "LSP: warning from server %arg{1}: %arg{2}"
    echo "LSP: warning from server %arg{1}: %arg{2}"
}

define-command -hidden lsp-show-message-info -params 2 -docstring %{
    lsp-show-message-info <message>
    Render language server message of the "info" level.
} %{
    echo -debug "LSP: info from server %arg{1}: %arg{2}"
    echo "LSP: info from server %arg{1}: %arg{2}"
}

define-command -hidden lsp-show-message-log -params 2 -docstring %{
    lsp-show-message-log <message>
    Render language server message of the "log" level.
} %{
    echo -debug "LSP: log from %arg{1}: %arg{2}"
}

define-command -hidden lsp-show-message-request -params 4.. -docstring %{
    lsp-show-message-request <prompt> <on-abort> <opt> <command> [<opt> <command>]...
    Render a prompt message with options.
} %{
    evaluate-commands -try-client %opt{toolsclient} %{
        info -title "From language server" %arg{1}
        evaluate-commands %sh{
            on_abort="$2"
            shift 2
            # The double-evaluation is intentional, this was escaped twice for convenience.
            echo lsp-menu -on-abort "$on_abort" "$@"
        }
    }
}

define-command lsp-show-message-request-next -docstring "Show the next pending message request" %{
    lsp-send window/showMessageRequest/showNext
}

define-command lsp-show-message-request-respond -params 1..2 -hidden %{
    lsp-send window/showMessageRequest/respond %arg{1} %arg{2} # id [item]
    # Close the info
    execute-keys "<esc>"
}

declare-option -hidden str lsp_progress_indicator
define-command -hidden lsp-handle-progress -params 6 -docstring %{
  lsp-handle-progress <token> <title> <cancelable> <message> <percentage> <done>
  Handle progress messages sent from the language server. Override to handle this.
} %{
    set-option global lsp_modeline_progress %sh{
        if ! "$6"; then
            echo "$kak_opt_lsp_progress_indicator"
            # More verbose alternative that shows what the server is working on.  Don't show this in
            # the modeline by default because the modeline is part of the terminal title; changing
            # that too quickly can be noisy.
            # echo "$2${5:+" ($5%)"}${4:+": $4"}"
        fi
    }
}

### Handling requests from server ###

define-command -hidden lsp-get-server-initialization-options -params 1 -docstring %{
    lsp-get-server-initialization-options <fifo>
    Format lsp_server_initialization_options as TOML and write to the given <fifo> path.
} %{
    echo -to-file %arg{1} -quoting shell -- %opt{lsp_server_configuration} map-end
}

define-command -hidden lsp-get-config -params 1 -docstring %{
    lsp-get-config <fifo>
    Format lsp_config as TOML and write to the given <fifo> path.
} %{
    echo -to-file %arg{1} -- %opt{lsp_config}
}

### Other commands ###

define-command lsp-diagnostic-object -docstring 'lsp-diagnostic-object [--include-warnings]: go to adjacent diagnostic from object mode' -params 0..1 %<
    evaluate-commands %sh<
        case "$kak_opt_lsp_object_mode" in
            ( [ | { | '<a-[>' | '<a-{>' ) previous=--previous ;;
            (*) previous= ;;
        esac
        echo lsp-find-error $previous $1
    >
>

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
        eval set -- "${kak_quoted_opt_lsp_inline_diagnostics}"

        less_than() {
            line1=$1
            column1=$2
            line2=$3
            column2=$4
            [ "$line1" -lt "$line2" ] || {
                [ "$line1" -eq "$line2" ] && [ "$column1" -lt "$column2" ]
            }
        }

        min() {
            line1=$1 column1=$2
            line2=$3 column2=$4
            less_than "$@" &&
                echo "$line1 $column1" ||
                echo "$line2 $column2"
        }

        max() {
            line1=$1 column1=$2
            line2=$3 column2=$4
            less_than "$@" &&
                echo "$line2 $column2" ||
                echo "$line1 $column1"
        }

        anchor=${kak_selection_desc%,*}
        anchor_line=${anchor%.*}
        anchor_column=${anchor#*.}
        cursor=${kak_selection_desc#*,}
        cursor_line=${cursor%.*}
        cursor_column=${cursor#*.}
        selection_start=$(min $anchor_line $anchor_column $cursor_line $cursor_column)
        selection_end=$(max $anchor_line $anchor_column $cursor_line $cursor_column)

        first=""
        current=""
        prev=""
        selection=""
        prev_no_overlap=true
        for e in "$@"; do
            if [ -z "${e##*DiagnosticError*}" ] || {
                $includeWarnings && [ -z "${e##*DiagnosticWarning*}" ]
            } then # e is an error or warning
                range=${e%|*}
                start=${range%,*}
                end=${range#*,}
                start_line=${start%.*}
                start_column=${start#*.}
                end_line=${end%.*}
                end_column=${end#*.}

                current="$range"
                if [ -z "$first" ]; then
                    first="$current"
                fi
                intersection_start=$(max $selection_start $start_line $start_column)
                intersection_end=$(min $selection_end $end_line $end_column)
                no_overlap=$(less_than $intersection_end $intersection_start && echo true || echo false)
                range_is_after_selection=$(less_than $selection_start $end_line $end_column && echo true || echo false)
                if "$range_is_after_selection" && {
                    $previous && $prev_no_overlap || $no_overlap
                } then
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
                    prev_no_overlap=$no_overlap
                fi
            fi
        done
        if [ -z "$first" ]; then
            # if nothing found
            echo "echo -markup '{Error}No errors found'"
            exit
        fi
        if [ -z "$selection" ]; then #if nothing found past the cursor
            if $previous; then
                selection="$current"
            else
                selection="$first"
            fi
        fi
        printf 'select %s\n' "$selection"
    }
}

define-command lsp-workspace-symbol -params 1 -docstring "lsp-workspace-symbol <query>: open buffer with matching project-wide symbols" %{
    lsp-workspace-symbol-buffer %val{buffile} %val{timestamp} %arg{1}
}

define-command lsp-workspace-symbol-incr -docstring "Open buffer with an incrementally updated list of project-wide symbols matching the query" %{
    declare-option -hidden str lsp_ws_buffile %val{buffile}
    declare-option -hidden int lsp_ws_timestamp %val{timestamp}
    declare-option -hidden str lsp_ws_query
    evaluate-commands -save-regs is %{
        set-register s %opt{lsp_servers}
        set-register i %opt{lsp_language_id}
        evaluate-commands -try-client %opt[toolsclient] %{
            lsp-show-goto-buffer *symbols* lsp-goto %{} %{}
            set-option buffer lsp_servers %reg{s}
            set-option buffer lsp_language_id %reg{s}
            lsp-unblock-in-buffer
            prompt -on-change %{ try %{
                # lsp-show-workspace-symbol triggers on-change somehow which causes inifinite loop
                # the following check prevents it
                evaluate-commands %sh{
                    if [ "${kak_opt_lsp_ws_query}" = "${kak_text}" ];
                    then echo 'fail';
                    else echo 'set current lsp_ws_query %val{text}';
                    fi
                }
                lsp-workspace-symbol-buffer %opt{lsp_ws_buffile} %opt{lsp_ws_timestamp} %val{text}
            }} -on-abort %{execute-keys ga} 'Query: ' nop
            focus %val{client}
        }
    }
}

### Hooks and highlighters ###

define-command lsp-inline-diagnostics-enable -params 1 -docstring "lsp-inline-diagnostics-enable <scope>: Enable inline diagnostics highlighting for <scope>" %{
    add-highlighter "%arg{1}/lsp_inline_diagnostics" ranges lsp_inline_diagnostics
    add-highlighter "%arg{1}/lsp_inline_diagnostics_deprecated" ranges lsp_inline_diagnostics_deprecated
    add-highlighter "%arg{1}/lsp_inline_diagnostics_unnecessary" ranges lsp_inline_diagnostics_unnecessary
} -shell-script-candidates %{ printf '%s\n' global buffer window }

define-command lsp-inline-diagnostics-disable -params 1 -docstring "lsp-inline-diagnostics-disable <scope>: Disable inline diagnostics highlighting for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_inline_diagnostics"
    remove-highlighter "%arg{1}/lsp_inline_diagnostics_unnecessary"
    remove-highlighter "%arg{1}/lsp_inline_diagnostics_deprecated"
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-diagnostic-lines-enable -params 1 -docstring "lsp-diagnostic-lines-enable <scope>: Show flags on lines with diagnostics in <scope>" %{
    add-highlighter "%arg{1}/lsp_diagnostic_lines" flag-lines LineNumbers lsp_diagnostic_lines
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-diagnostic-lines-disable -params 1 -docstring "lsp-diagnostic-lines-disable <scope>: Hide flags on lines with diagnostics in <scope>"  %{
    remove-highlighter "%arg{1}/lsp_diagnostic_lines"
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-diagnostics-enable -params 1 -docstring "lsp-inlay-diagnostics-enable <scope>: Enable inlay diagnostics highlighting for <scope>" %{
    try %{
        add-highlighter "%arg{1}/lsp_inlay_diagnostics" flag-lines -after Default lsp_inlay_diagnostics
    } catch %{
        fail -- "%val{error} (NOTE: lsp-inlay-diagnostics-enable requires Kakoune >= 2024)"
    }
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-diagnostics-disable -params 1 -docstring "lsp-inlay-diagnostics-disable <scope>: Disable inlay diagnostics highlighting for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_inlay_diagnostics"
    remove-hooks %arg{1} lsp-inlay-diagnostics
} -shell-script-candidates %{ printf '%s\n' buffer global window }

declare-option -hidden str lsp_auto_hover_selection
define-command -hidden lsp-check-auto-hover -params 1 %{
  evaluate-commands %sh{
    [ "$kak_selection_desc" = "${kak_opt_lsp_auto_hover_selection}" ] && exit
    echo "$1"
    echo "set-option window lsp_auto_hover_selection ${kak_selection_desc}"
  }
}

define-command lsp-auto-hover-enable -docstring "enable auto-requesting hover info box for current position" %{
    remove-hooks global lsp-auto-hover
    hook -group lsp-auto-hover global NormalIdle .* %{ lsp-check-auto-hover lsp-hover }
}

define-command lsp-auto-hover-disable -docstring "Disable auto-requesting hover info for current position" %{
    remove-hooks global lsp-auto-hover
}

define-command lsp-auto-hover-buffer-enable \
    -docstring "lsp-auto-hover-buffer-enable: enable auto-requesting hover info buffer for current position

This will continuously update the '*hover*' buffer, keeping it visible.
Additionally, the buffer will be activated in the client referred to by the 'docsclient' option." %{
    remove-hooks global lsp-auto-hover-buffer
    hook -group lsp-auto-hover-buffer global NormalIdle .* %{ lsp-check-auto-hover %{lsp-hover-buffer %opt{docsclient}} }
}

define-command lsp-auto-hover-buffer-disable -docstring "Disable auto-requesting hover info in docsclient for current position" %{
    remove-hooks global lsp-auto-hover-buffer
}

define-command lsp-auto-hover-insert-mode-enable -params 0..1 -client-completion \
    -docstring "lsp-auto-hover-insert-mode-enable [<client>]: enable auto-requesting hover info for current function in insert mode

If a client is given, show hover in a scratch buffer in that client instead of the info box" %{
    remove-hooks global lsp-auto-hover-insert-mode
    evaluate-commands %sh{
        hover=lsp-hover
        [ $# -eq 1 ] && hover="lsp-hover-buffer $1"
        printf %s "hook -group lsp-auto-hover-insert-mode global InsertIdle .* %{
            try %{ evaluate-commands -draft %{
                evaluate-commands %opt{lsp_hover_insert_mode_trigger}
                $hover
            }}
        }"
    }
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

define-command lsp-inlay-hints-enable -params 1 -docstring "lsp-inlay-hints-enable <scope>: enable inlay hints for <scope>" %{
    add-highlighter "%arg{1}/lsp_inlay_hints" replace-ranges lsp_inlay_hints
    hook -group lsp-inlay-hints %arg{1} BufReload .* lsp-inlay-hints
    hook -group lsp-inlay-hints %arg{1} NormalIdle .* lsp-inlay-hints
    hook -group lsp-inlay-hints %arg{1} InsertIdle .* lsp-inlay-hints
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-hints-disable -params 1 -docstring "lsp-inlay-hints-disable <scope>: disable inlay hints for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_inlay_hints"
    remove-hooks %arg{1} lsp-inlay-hints
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-code-lenses-enable -params 1 -docstring "lsp-inlay-code-lenses-enable <scope>: enable inlay code lenses for <scope>" %{
    try %{
        add-highlighter "%arg{1}/lsp_inlay_code_lenses" flag-lines -after Default lsp_inlay_code_lenses
    } catch %{
        fail -- "%val{error} (NOTE: lsp-inlay-code-lenses-enable requires Kakoune >= 2024)"
    }
} -shell-script-candidates %{ printf '%s\n' buffer global window }

define-command lsp-inlay-code-lenses-disable -params 1 -docstring "lsp-inlay-code-lenses-disable <scope>: disable inlay code lenses for <scope>"  %{
    remove-highlighter "%arg{1}/lsp_inlay_code_lenses"
    remove-hooks %arg{1} lsp-inlay-code-lenses
} -shell-script-candidates %{ printf '%s\n' buffer global window }

### User mode ###

declare-user-mode lsp
map global lsp a '<esc>: lsp-code-actions<ret>'            -docstring 'show code actions for current position'
map global lsp c '<esc>: lsp-capabilities<ret>'            -docstring 'show language server capabilities'
map global lsp d '<esc>: lsp-definition<ret>'              -docstring 'go to definition'
map global lsp e '<esc>: lsp-diagnostics<ret>'             -docstring 'list project errors, info, hints and warnings'
map global lsp f '<esc>: lsp-formatting<ret>'              -docstring 'format buffer'
map global lsp h '<esc>: lsp-hover<ret>'                   -docstring 'show info for current position'
map global lsp H '<esc>: lsp-hover-buffer<ret>'            -docstring 'show info for current position in a scratch buffer'
map global lsp i '<esc>: lsp-implementation<ret>'          -docstring 'go to implementation'
map global lsp j '<esc>: lsp-outgoing-calls<ret>'          -docstring 'list outgoing call for function at cursor'
map global lsp k '<esc>: lsp-incoming-calls<ret>'          -docstring 'list incoming call for function at cursor'
map global lsp l '<esc>: lsp-code-lens<ret>'               -docstring 'apply a code lens from the current selection'
map global lsp o '<esc>: lsp-workspace-symbol-incr<ret>'   -docstring 'search project symbols'
map global lsp n '<esc>: lsp-find-error<ret>'              -docstring 'find next error'
map global lsp p '<esc>: lsp-find-error --previous<ret>'   -docstring 'find previous error'
map global lsp q '<esc>: lsp-disable<ret>'                 -docstring 'disable LSP integration'
map global lsp r '<esc>: lsp-references<ret>'              -docstring 'list symbol references'
map global lsp R '<esc>: lsp-rename-prompt<ret>'           -docstring 'rename symbol'
map global lsp s '<esc>: lsp-goto-document-symbol<ret>'    -docstring 'jump to document symbol'
map global lsp S '<esc>: lsp-document-symbol<ret>'         -docstring 'list document symbols'
map global lsp v '<esc>: lsp-selection-range<ret>'         -docstring 'select inner/outer nodes'
map global lsp V '<esc>: lsp-selection-range cached<ret>'  -docstring 'select inner/outer nodes (re-use previous call)'
map global lsp y '<esc>: lsp-type-definition<ret>'         -docstring 'go to type definition'
map global lsp [ '<esc>: lsp-hover-previous-symbol<ret>'   -docstring 'show hover for previous symbol'
map global lsp ] '<esc>: lsp-hover-next-symbol<ret>'       -docstring 'show hover for next symbol'
map global lsp { '<esc>: lsp-previous-symbol<ret>'         -docstring 'goto previous symbol'
map global lsp } '<esc>: lsp-next-symbol<ret>'             -docstring 'goto next symbol'
map global lsp 9 '<esc>: lsp-hover-previous-function<ret>' -docstring 'show hover for previous function'
map global lsp 0 '<esc>: lsp-hover-next-function<ret>'     -docstring 'show hover for next function'
map global lsp ( '<esc>: lsp-previous-function<ret>'       -docstring 'goto previous function'
map global lsp ) '<esc>: lsp-next-function<ret>'           -docstring 'goto next function'
map global lsp & '<esc>: lsp-highlight-references<ret>'    -docstring 'lsp-highlight-references'
map global lsp = '<esc>: lsp-range-formatting<ret>'        -docstring 'format selections'

declare-user-mode lsp-selection-range
map global lsp-selection-range j '<esc>: lsp-selection-range-select down<ret>'   -docstring 'select inner node'
map global lsp-selection-range k '<esc>: lsp-selection-range-select up<ret>'     -docstring 'select outer node'
map global lsp-selection-range b '<esc>: lsp-selection-range-select bottom<ret>' -docstring 'select innermost node'
map global lsp-selection-range t '<esc>: lsp-selection-range-select top<ret>'    -docstring 'select outermost node'

map global goto d '<esc>:lsp-definition<ret>' -docstring 'definition'
map global goto r '<esc>:lsp-references<ret>' -docstring 'references'
map global goto y '<esc>:lsp-type-definition<ret>' -docstring 'type definition'

### Default integration ###

define-command lsp-enable -docstring "Default LSP integration" %{
    hook -group lsp global BufSetOption filetype=(.*) %{
        lsp-if-no-servers trigger-user-hook "LSPDefaultConfig=%val{hook_param_capture_1}"
    }
    lsp-enable-impl global
    evaluate-commands -buffer * %{
        # For kak somefile -e 'eval %sh{kak-lsp}'
        lsp-if-no-servers trigger-user-hook "LSPDefaultConfig=%opt{filetype}"
        hook -group lsp buffer BufSetOption (?:lsp_servers|lsp_config|lsp_server_configuration)=.* lsp-did-change-config
    }
    hook -group lsp global BufClose .* lsp-did-close
    # lsp-enable is expected to not be called from autoload, so this hook should run after most
    # filetype detection hooks.
    hook -group lsp global BufCreate .* %{
        lsp-did-open
        hook -group lsp buffer BufSetOption (?:lsp_servers|lsp_config|lsp_server_configuration)=.* lsp-did-change-config
    }
    lsp-did-change-config
}

define-command lsp-disable -docstring "Disable LSP" %{
    evaluate-commands -buffer * %{
        remove-hooks buffer lsp
        lsp-unblock-in-buffer
        unset-option buffer lsp_modeline_code_actions
    }
    set-option global lsp_modeline_progress ""
    set-option global lsp_modeline_message_requests ""
    lsp-disable-impl global
    set-option global lsp_fail_if_disabled fail
}

define-command lsp-enable-window -docstring "Default LSP integration in the window scope" %{
    lsp-if-no-servers trigger-user-hook "LSPDefaultConfig=%opt{filetype}"
    lsp-enable-impl window
    hook -group lsp window WinClose .* lsp-did-close
    hook -group lsp window WinSetOption (?:lsp_servers|lsp_config|lsp_server_configuration)=.* lsp-did-change-config
    lsp-did-open
    lsp-did-change-config
}

define-command lsp-disable-window -docstring "Disable LSP in the window scope" %{
    lsp-disable-impl window
    unset-option window lsp_fail_if_disabled
}

define-command -hidden lsp-enable-impl -params 1 %{
    try "
        add-highlighter %arg{1}/cquery_semhl ranges cquery_semhl
    " catch "
        fail 'LSP already enabled at %arg{1} scope'
    "
    add-highlighter "%arg{1}/lsp_references" ranges lsp_references
    add-highlighter "%arg{1}/lsp_semantic_tokens_ranges" ranges lsp_semantic_tokens_ranges
    add-highlighter "%arg{1}/lsp_snippets_placeholders" ranges lsp_snippets_placeholders
    lsp-inline-diagnostics-enable %arg{1}
    lsp-diagnostic-lines-enable %arg{1}

    set-option %arg{1} completers option=lsp_completions %opt{completers}
    set-option %arg{1} lsp_fail_if_disabled nop

    hook -group lsp %arg{1} BufWritePost .* lsp-did-save
    hook -group lsp %arg{1} InsertIdle .* %{ lsp-did-change; lsp-completion }
    hook -group lsp %arg{1} ModeChange pop:insert:.* %{
        set-option window lsp_snippets_placeholders
        set-option window lsp_snippets_placeholder_groups
    }
    # A non-empty hook parameter means some completion was inserted.
    hook -group lsp %arg{1} InsertCompletionHide .+ lsp-completion-accepted
    hook -group lsp %arg{1} NormalIdle .* %{
        lsp-did-change
        lsp-code-lens-request
        evaluate-commands %sh{
            if $kak_opt_lsp_auto_highlight_references; then echo lsp-highlight-references; fi
            if $kak_opt_lsp_auto_show_code_actions; then echo "lsp-code-actions-background-request"; fi
        }
    }
    hook -group lsp %arg{1} NormalKey (<a-i>|<a-a>|\[|\]|\{|\}|<a-\[>|<a-\]>|<a-\{>|<a-\}>) %{
        set-option window lsp_object_mode %val{hook_param}
    }
    hook -group lsp-breadcrumbs %arg{1} BufReload .* lsp-breadcrumbs-request
    hook -group lsp-breadcrumbs %arg{1} NormalIdle .* lsp-breadcrumbs-request
    hook -group lsp-breadcrumbs %arg{1} InsertIdle .* lsp-breadcrumbs-request
}

define-command -hidden lsp-disable-impl -params 1 %{
    try "
        remove-highlighter %arg{1}/cquery_semhl
    " catch "
        fail 'LSP already disabled at %arg{1} scope'
    "
    remove-highlighter "%arg{1}/lsp_references"
    remove-highlighter "%arg{1}/lsp_semantic_tokens_ranges"
    remove-highlighter "%arg{1}/lsp_snippets_placeholders"
    lsp-inline-diagnostics-disable %arg{1}
    lsp-diagnostic-lines-disable %arg{1}
    try %{ set-option -remove %arg{1} completers option=lsp_completions }
    remove-hooks %arg{1} lsp
    remove-hooks %arg{1} lsp-breadcrumbs
    remove-hooks global lsp-auto-hover
    remove-hooks global lsp-auto-hover-insert-mode
    remove-hooks global lsp-auto-signature-help
    lsp-exit
}

declare-option -hidden str lsp_fail_if_disabled fail

hook -always global KakEnd .* %{
    remove-hooks global lsp # BufClose
    set-option global lsp_fail_if_disabled nop # hack for lsp-enable-window
    try lsp-exit
    set-option global lsp_unless_blocked lsp-blocked
}
try %{
    hook -group lsp-session-renamed global SessionRenamed .* %{
        try lsp-exit
        set-option global lsp_pid_file %{}
        set-option global lsp_fifo %{}
    }
}

# SNIPPETS
# This is a slightly modified version of occivink/kakoune-snippets

declare-option -hidden range-specs lsp_snippets_placeholders
declare-option -hidden int-list lsp_snippets_placeholder_groups

set-face global SnippetsNextPlaceholders black,green+F
set-face global SnippetsOtherPlaceholders black,yellow+F

declare-option -hidden str lsp_snippet_to_insert ""
define-command -hidden lsp-snippets-insert-completion -params 1 %{ evaluate-commands %{
    set-option window lsp_snippet_to_insert %arg{1}
    lsp-completion-on-accept %{
        # Delete the inserted text.
        select %opt{lsp_completion_inserted_ranges}
        execute-keys '<a-;>d'
        evaluate-commands -save-regs y %{
            set-register y nop
            evaluate-commands -draft -verbatim lsp-snippets-insert %opt[lsp_snippet_to_insert]
            try %reg{y}
        }
    }
}}

define-command lsp-snippets-insert -hidden -params 1 %[
    evaluate-commands %sh{
        if ! command -v perl > /dev/null; then
            printf "fail %{'perl' must be installed to use the 'lsp-snippets-insert' command'}"
        fi
    }
    evaluate-commands -draft -save-regs '^"' %[
        set-register '"' %arg{1}
        execute-keys <a-P>
        # replace leading tabs with the appropriate indent
        try %{
            set-register '"' %sh{
                if [ $kak_opt_indentwidth -eq 0 ]; then
                    printf '\t'
                else
                    printf "%${kak_opt_indentwidth}s"
                fi
            }
            execute-keys -draft '<a-s>s\A\t+<ret>s.<ret>R'
        }
        # align everything with the current line
        evaluate-commands -draft -itersel -save-regs '"' %{
            try %{
                execute-keys -draft -save-regs '/' '<a-s>)<space><esc>,<esc><semicolon>xs^\s+<ret>y'
                execute-keys -draft '<a-s>)<a-space><esc><a-,><esc>P'
            }
        }
        try %[
            # select things that look like placeholders
            # this regex is not as bad as it looks
            evaluate-commands -save-regs x -draft %[
                execute-keys s((?<lt>!\\)(\\\\)*|\A)\K(\$(\d+|\{(\d+(:(\\\}|[^}])*)?)\}))<ret>)
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
        try %[
            # Unescape snippet metacharacters.
            execute-keys 's\\[$}\\,|]<ret><a-:><a-semicolon><semicolon>d'
        ]
    ]
]

define-command -hidden lsp-snippets-insert-perl-impl %[
    set-register x nop
    evaluate-commands %sh[ # $kak_quoted_selections
        perl -e '
use strict;
use warnings;
use Text::ParseWords();

my @sel_content = Text::ParseWords::shellwords($ENV{"kak_quoted_selections"});
my @existing_placeholder_ids = split(/ /, $ENV{"kak_opt_lsp_snippets_placeholder_groups"});
my @existing_placeholders = split(/ /, $ENV{"kak_opt_lsp_snippets_placeholders"});
my @selections_desc = split(/ /, $ENV{"kak_selections_desc"});

if (scalar @existing_placeholders > 1) {
    $existing_placeholders[1] =~ m{^(\d+)\.(\d+)} or die "failed to parse: $existing_placeholders[1]";
    my $existing_line = $1 and my $existing_column = $2 ;
    $selections_desc[0] =~ m{^(\d+)\.(\d+)} or die "failed to parse: $selections_desc[0]";
    my $new_line = $1 and my $new_column = $2;
    if ($new_line > $existing_line || ($new_line == $existing_line && $new_column > $existing_column)) {
        print "set-option window lsp_snippets_placeholders\n";
        @existing_placeholder_ids = ();
    }
}

my @placeholder_defaults;
my %placeholder_id_to_default;
my @placeholder_ids;
my %placeholder_id_to_compacted_id;

for my $i (0 .. $#sel_content) {
    my $sel = $sel_content[$i];
    $sel =~ s/\A\$\{?|\}\Z//g;
    my ($placeholder_id, $placeholder_default) = ($sel =~ /^(\d+)(?::(.*))?$/);
    $placeholder_ids[$i] = $placeholder_id + 0;
    if (defined($placeholder_default)) {
        $placeholder_defaults[$i] = $placeholder_default;
    }
}

my $next_placeholder_id = 0;
for my $i (0 .. $#sel_content) {
    my $placeholder_id = $placeholder_ids[$i];
    if (not exists $placeholder_id_to_compacted_id{$placeholder_id}) {
        my $id;
        if ($placeholder_id == 0) {
            if (scalar @existing_placeholder_ids) {
                # If we are a recusive snippet there probably already is an end anchor, so
                # ignore this one.
                print "set-register x execute-keys %[" . $i . ")<a-,>]\n";
                $id = -1;
            } else {
                $id = scalar @placeholder_ids - 1;
            }
        } else {
            $id = $next_placeholder_id++;
        }
        $placeholder_id_to_compacted_id{$placeholder_id} = $id;
        if (defined($placeholder_defaults[$i])) {
            $placeholder_id_to_default{$id} = $placeholder_defaults[$i];
        }
    }
}

print("set-option window lsp_snippets_placeholder_groups");
foreach (@placeholder_ids) {
    if ($placeholder_id_to_compacted_id{$_} != -1) {
        print " $placeholder_id_to_compacted_id{$_}";
    }
}
foreach (@existing_placeholder_ids) {
    print " " . ($_ + scalar @placeholder_ids);
}
print "\n";

foreach (@placeholder_ids) {
    if ($placeholder_id_to_compacted_id{$_} != -1) {
        print "set-register y lsp-snippets-select-next-placeholders\n";
        last;
    }
}


print("set-register dquote");
foreach my $placeholder_id (@placeholder_ids) {
    my $def = "";
    $placeholder_id = $placeholder_id_to_compacted_id{$placeholder_id};
    if ($placeholder_id != -1) {
        if (exists $placeholder_id_to_default{$placeholder_id}) {
            $def = $placeholder_id_to_default{$placeholder_id};
            # de-double up closing braces
            $def =~ s/\}\}/}/g;
            # double up single-quotes
            $def =~ s/'\''/'\'''\''/g;
        }
        # make sure that the placeholder is non-empty so we can select it
        if (length $def == 0) { $def = " " }
    }
    print(" '\''$def'\''");
}
print("\n");
'
    ]
    execute-keys R
    %reg{x}
    update-option window lsp_snippets_placeholders
    # no need to set the NextPlaceholders face yet, select-next-placeholders will take care of that
    evaluate-commands -itersel %{ set-option -add window lsp_snippets_placeholders "%val{selections_desc}|SnippetsOtherPlaceholders" }
]

define-command lsp-snippets-select-next-placeholders %{
    # Make sure to accept any completion, so we consider its placeholders.
    execute-keys -with-hooks i<backspace>
    update-option window lsp_snippets_placeholders
    evaluate-commands %sh{
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
        printf 'set-option window lsp_snippets_placeholder_groups'
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
        printf 'set-option window lsp_snippets_placeholders'
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
    # Delete the placeholder text
    execute-keys '<a-;>d'
}

hook -group lsp-goto-highlight global WinSetOption filetype=(lsp-(?:diagnostics|document-symbol|goto)) %{
    add-highlighter "window/%val{hook_param_capture_1}" group
    add-highlighter "window/%val{hook_param_capture_1}/" regex ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+)) 1:cyan 2:green 3:green
    add-highlighter "window/%val{hook_param_capture_1}/" line %{%opt{jump_current_line}} default+b
    hook -once -always window WinSetOption filetype=.* "remove-highlighter window/%val{hook_param_capture_1}"
}

hook -group lsp-document-symbol-highlight global WinSetOption filetype=lsp-document-symbol %{
    add-highlighter window/lsp-symbols-kind regex \([\w+]+\)$ 0:+di@type
    add-highlighter window/lsp-symbols-tree regex [├└─│] 0:comment
    hook -once -always window WinSetOption filetype=.* %{
        remove-highlighter window/lsp-symbols-kind
        remove-highlighter window/lsp-symbols-tree
    }
}

define-command -hidden lsp-select-next %{
        set-register / ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))
        execute-keys ge %opt{jump_current_line}g<a-l> /<ret>
}
define-command -hidden lsp-select-previous %{
        set-register / ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))
        execute-keys ge %opt{jump_current_line}g<a-h> <a-/><ret>
}

hook -group lsp-goto global WinSetOption filetype=(lsp-(?:diagnostics|document-symbol|goto)) %{
    map window normal <ret> ":jump # %val{hook_param_capture_1}<ret>"
    hook -once -always window WinSetOption filetype=.* "
        unmap window normal <ret> ':jump # %val{hook_param_capture_1}<ret>'
    "
    alias buffer jump "%val{hook_param_capture_1}-jump"
    alias buffer jump-select-next lsp-select-next
    alias buffer jump-select-previous lsp-select-previous
}

define-command -hidden lsp-make-register-relative-to-root %{
    evaluate-commands -save-regs / %{
        try %{
            # Is it an absolute path?
            execute-keys <a-k>\A/<ret>
        } catch %{
            set-register a "%opt{lsp_project_root}%reg{a}"
        }
    }
}

define-command -hidden lsp-goto-jump -docstring %{
    Same as jump except

    1. apply lsp_project_root to relative paths
    2. tolerate leading whitespace, currently for *callers* and *callees*.
} %{
    evaluate-commands -save-regs abc %{
        try %{
            evaluate-commands -draft -save-regs / %{
                set-register / ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))
                execute-keys <semicolon>xs<ret>
                set-register a "%reg{1}"
                set-register b "%reg{2}"
                set-register c "%reg{3}"
                lsp-make-register-relative-to-root
            }
            set-option buffer jump_current_line %val{cursor_line}
            evaluate-commands -try-client %opt{jumpclient} -verbatim -- edit -existing -- %reg{a} %reg{b} %reg{c}
            try %{ focus %opt{jumpclient} }
        }
    }
}

define-command -hidden lsp-document-symbol-jump -docstring %{
    Same as lsp-goto-jump except this uses a buffer-scoped filename option
} %{
    evaluate-commands -save-regs bc %{
        try %{
            evaluate-commands -draft -save-regs / %{
                set-register / ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))
                execute-keys <semicolon>xs<ret>
                set-register b "%reg{2}"
                set-register c "%reg{3}"
            }
            set-option buffer jump_current_line %val{cursor_line}
            evaluate-commands -try-client %opt{jumpclient} -verbatim -- edit -existing -- %opt{lsp_buffile} %reg{b} %reg{c}
            try %{ focus %opt{jumpclient} }
        }
    }
}

define-command -hidden lsp-diagnostics-jump %{
    evaluate-commands -save-regs abcd %{
        evaluate-commands -draft -save-regs / %{
            set-register / ^\h*\K([^:\n]+):(\d+)\b(?::(\d+)\b)?(?::([^\n]+))
            execute-keys <semicolon>xs<ret>
            set-register a "%reg{1}"
            set-register b "%reg{2}"
            set-register c "%reg{3}"
            set-register d "%reg{4}"
            lsp-make-register-relative-to-root
        }
        set-option buffer jump_current_line %val{cursor_line}
        lsp-diagnostics-open-error %reg{a} "%reg{b}" "%reg{c}" "%reg{d}"
    }
}

define-command -hidden lsp-diagnostics-open-error -params 4 %{
    evaluate-commands -try-client %opt{jumpclient} %{
        edit -existing -- "%arg{1}" %arg{2} %arg{3}
        echo -markup "{Information}{\}%arg{4}"
        try %{ focus }
    }
}

declare-option -docstring 'name of the client in which documentation is to be displayed' str docsclient
declare-option -docstring "maximum amount of characters per line, after which a newline character will be inserted" int autowrap_column 80

hook global -once KakBegin .* %{
    try %{
        require-module jump
    } catch %{
        declare-option -docstring "name of the client in which all source code jumps will be executed" \
            str jumpclient
        declare-option -docstring "name of the client in which utilities display information" \
            str toolsclient

        declare-option -hidden int jump_current_line 0

        define-command -hidden jump %{
            evaluate-commands -save-regs a %{ # use evaluate-commands to ensure jumps are collapsed
                try %{
                    evaluate-commands -draft %{
                        execute-keys ',xs^([^:\n]+):(\d+):(\d+)?<ret>'
                        set-register a %reg{1} %reg{2} %reg{3}
                    }
                    set-option buffer jump_current_line %val{cursor_line}
                    evaluate-commands -try-client %opt{jumpclient} -verbatim -- edit -existing -- %reg{a}
                    try %{ focus %opt{jumpclient} }
                }
            }
        }

        define-command jump-next -params 1 -docstring %{
            jump-next <bufname>: jump to next location listed in the given buffer
        } %{
            evaluate-commands -try-client %opt{jumpclient} -save-regs / %{
                buffer %arg{1}
                jump-select-next
                jump
            }
            try %{
                evaluate-commands -client %opt{toolsclient} %{
                    buffer %arg{1}
                    execute-keys gg %opt{jump_current_line}g
                }
            }
        }
        complete-command jump-next buffer
        define-command -hidden jump-select-next %{
            # First jump to end of buffer so that if jump_current_line == 0
            # 0g<a-l> will be a no-op and we'll jump to the first result.
            # Yeah, thats ugly...
            execute-keys ge %opt{jump_current_line}g<a-l> /^[^:\n]+:\d+:<ret>
        }

        define-command jump-previous -params 0..1 -docstring %{
            jump-previous <bufname>: jump to previous location listed in the given buffer
        } %{
            evaluate-commands -try-client %opt{jumpclient} -save-regs / %{
                buffer %arg{1}
                jump-select-previous
                jump
            }
            try %{
                evaluate-commands -client %opt{toolsclient} %{
                    buffer %arg{1}
                    execute-keys gg %opt{jump_current_line}g
                }
            }
        }
        complete-command jump-previous buffer
        define-command -hidden jump-select-previous %{
            # See comment in jump-select-next
            execute-keys ge %opt{jump_current_line}g<a-h> <a-/>^[^:\n]+:\d+:<ret>
        }
    }
}

# Deprecated commands.

define-command -hidden lsp -params 1.. %{ evaluate-commands "lsp-%arg{1}" }

define-command -hidden lsp-next-location -params 1 -docstring %{DEPRECATED, use "jump-next"} %{
    jump-next %arg{1}
}

define-command -hidden lsp-previous-location -params 1 -docstring %{DEPRECATED, use "jump-previous"} %{
    jump-previous %arg{1}
}

define-command -hidden lsp-symbols-next-match -docstring 'DEPRECATED, use jump-next. Jump to the next symbols match' %{
    jump-next '*symbols*'
}

define-command -hidden lsp-symbols-previous-match -docstring 'DEPRECATED, use jump-previous. Jump to the previous symbols match' %{
    jump-previous '*symbols*'
}

define-command -hidden lsp-goto-next-match -docstring 'DEPRECATED, use jump-next. Jump to the next goto match' %{
    jump-next '*goto*'
}

define-command -hidden lsp-goto-previous-match -docstring 'DEPRECATED, use jump-previous. Jump to the previous goto match' %{
    jump-previous '*goto*'
}

define-command -hidden lsp-stop-on-exit-enable -docstring "DEPRECATED. End kakoune-lsp session on Kakoune session end" %{}
define-command -hidden lsp-stop-on-exit-disable -docstring "DEPRECATED. Don't end kakoune-lsp session on Kakoune session end" %{}
define-command -hidden lsp-stop -docstring "DEPRECATED: lsp-stop: terminate the given kakoune-lsp session" lsp-exit
