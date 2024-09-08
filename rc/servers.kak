### Language Servers ###
#
# For help, see the docstring of the 'lsp_servers' option.

hook -group lsp-filetype-c-family global User LSPDefaultConfig=(?:c|cpp|objc) %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [clangd]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" compile_commands.json .clangd .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-clojure global User LSPDefaultConfig=clojure %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [clojure-lsp]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" project.clj .git .hg $(: kak_buffile)}""
            settings_section = ""_""
            [clojure-lsp.settings._]
            # See https://clojure-lsp.io/settings/#all-settings
            # source-paths-ignore-regex = [""resources.*"", ""target.*""]
        "
    }
}

hook -group lsp-filetype-cmake global User LSPDefaultConfig=make %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [cmake-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" CMakeLists.txt .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-crystal global User LSPDefaultConfig=crystal %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [crystalline]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" shard.yml $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-css global User LSPDefaultConfig=(?:css|less|scss) %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [vscode-css-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json .git .hg $(: kak_buffile)}""
            args = [""--stdio""]
        "
    }
}

hook -group lsp-filetype-d global User LSPDefaultConfig=(?:d|di) %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [dls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git dub.sdl dub.json $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-dart global User LSPDefaultConfig=dart %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [dart-lsp]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" pubspec.yaml .git .hg $(: kak_buffile)}""
            # start shell to find path to dart analysis server source
            command = ""sh""
            args = [""-c"", ""dart \""$(dirname \""$(command -v dart)\"")\""/snapshots/analysis_server.dart.snapshot --lsp""]
        "
    }
}

hook -group lsp-filetype-elixir global User LSPDefaultConfig=elixir %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [elixir-ls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" mix.exs $(: kak_buffile)}""
            settings_section = ""elixirLS""
            [elixir-ls.settings.elixirLS]
            # See https://github.com/elixir-lsp/elixir-ls/blob/master/apps/language_server/lib/language_server/server.ex
            # dialyzerEnable = true
        "
    }
}

hook -group lsp-filetype-elm global User LSPDefaultConfig=elm %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [elm-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" elm.json $(: kak_buffile)}""
            args = [""--stdio""]
            settings_section = ""elmLS""
            [elm-language-server.settings.elmLS]
            # See https://github.com/elm-tooling/elm-language-server#server-settings
            runtime = ""node""
            elmPath = ""elm""
            elmFormatPath = ""elm-format""
            elmTestPath = ""elm-test""
        "
    }
}

hook -group lsp-filetype-elvish global User LSPDefaultConfig=elvish %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [elvish]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            args = [""-lsp""]
        "
    }
}

hook -group lsp-filetype-erlang global User LSPDefaultConfig=erlang %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [erlang_ls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" rebar.config erlang.mk .git .hg $(: kak_buffile)}""
            # See https://github.com/erlang-ls/erlang_ls.git for more information and
            # how to configure. This default config should work in most cases though.
        "
    }
}

hook -group lsp-filetype-go global User LSPDefaultConfig=go %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [gopls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" Gopkg.toml go.mod .git .hg $(: kak_buffile)}""
            [gopls.settings.gopls]
            # See https://github.com/golang/tools/blob/master/gopls/doc/settings.md
            # ""build.buildFlags"" = []
            hints.assignVariableTypes = true
            hints.compositeLiteralFields = true
            hints.compositeLiteralTypes = true
            hints.constantValues = true
            hints.functionTypeParameters = true
            hints.parameterNames = true
            hints.rangeVariableTypes = true
            usePlaceholders = true
        "
    }
}

hook -group lsp-filetype-haskell global User LSPDefaultConfig=haskell %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [haskell-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" hie.yaml cabal.project Setup.hs stack.yaml '*.cabal' $(: kak_buffile)}""
            command = ""haskell-language-server-wrapper""
            args = [""--lsp""]
            settings_section = ""_""
            [haskell-language-server.settings._]
            # See https://haskell-language-server.readthedocs.io/en/latest/configuration.html
            # haskell.formattingProvider = ""ormolu""
        "
    }
}

hook -group lsp-filetype-html global User LSPDefaultConfig=html %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [vscode-html-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json $(: kak_buffile)}""
            args = [""--stdio""]
            settings_section = ""_""
            [html-language-server.settings._]
            # quotePreference = ""single""
            # javascript.format.semicolons = ""insert""
        "
    }
}

hook -group lsp-filetype-javascript global User LSPDefaultConfig=(?:javascript|typescript) %{
    lsp-if-no-servers %{
        set-option -add buffer lsp_servers "
            [typescript-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json tsconfig.json jsconfig.json .git .hg $(: kak_buffile)}""
            args = [""--stdio""]
            settings_section = ""_""
            [typescript-language-server.settings._]
            # quotePreference = ""double""
            # typescript.format.semicolons = ""insert""
        "
        # set-option -add buffer lsp_servers "
        #     [deno]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json tsconfig.json .git .hg $(: kak_buffile)}""
        #     args = [""lsp""]
        #     settings_section = ""deno""
        #     [deno-lsp.settings.deno]
        #     enable = true
        #     lint = true
        # "
        # set-option -add buffer lsp_servers "
        #     [biome]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" biome.json package.json tsconfig.json jsconfig.json .git .hg $(: kak_buffile)}""
        #     args = [""lsp-proxy""]
        # "
        # set-option -add buffer lsp_servers "
        #     [eslint-language-server]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" .eslintrc .eslintrc.json $(: kak_buffile)}""
        #     args = [""--stdio""]
        #     workaround_eslint = true
        #     [eslint.settings]
        #     codeActionsOnSave = { mode = ""all"", ""source.fixAll.eslint"" = true }
        #     format = { enable = true }
        #     quiet = false
        #     rulesCustomizations = []
        #     run = ""onType""
        #     validate = ""on""
        #     experimental = {}
        #     problems = { shortenToSingleLine = false }
        #     codeAction.disableRuleComment = { enable = true, location = ""separateLine"" }
        #     codeAction.showDocumentation = { enable = false }
        # "
    }
}

hook -group lsp-filetype-java global User LSPDefaultConfig=java %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [jdtls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" mvnw gradlew .git .hg $(: kak_buffile)}""
            [jdtls.settings]
            # See https://github.dev/eclipse/eclipse.jdt.ls
            # ""java.format.insertSpaces"" = true
        "
    }
}

hook -group lsp-filetype-json global User LSPDefaultConfig=json %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [vscode-json-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json $(: kak_buffile)}""
            args = [""--stdio""]
        "
    }
}

hook -group lsp-filetype-julia global User LSPDefaultConfig=julia %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            # Requires Julia package ""LanguageServer""
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" Project.toml .git .hg $(: kak_buffile)}""
            # Run: `julia --project=@kak-lsp -e 'import Pkg; Pkg.add(""LanguageServer"")'` to install it
            # Configuration adapted from https://github.com/neovim/nvim-lspconfig/blob/bcebfac7429cd8234960197dca8de1767f3ef5d3/lua/lspconfig/julials.lua
            [julia-language-server]
            command = ""julia""
            args = [
                ""--startup-file=no"",
                ""--history-file=no"",
                ""-e"",
                """"""
                ls_install_path = joinpath(get(DEPOT_PATH, 1, joinpath(homedir(), "".julia"")), ""environments"", ""kak-lsp"");
                pushfirst!(LOAD_PATH, ls_install_path);
                using LanguageServer;
                popfirst!(LOAD_PATH);
                depot_path = get(ENV, ""JULIA_DEPOT_PATH"", """");
                server = LanguageServer.LanguageServerInstance(stdin, stdout, """", depot_path);
                server.runlinter = true;
                run(server);
                """""",
            ]
            [julia-language-server.settings]
            # See https://github.com/julia-vscode/LanguageServer.jl/blob/master/src/requests/workspace.jl
            # Format options. See https://github.com/julia-vscode/DocumentFormat.jl/blob/master/src/DocumentFormat.jl
            # ""julia.format.indent"" = 4
            # Lint options. See https://github.com/julia-vscode/StaticLint.jl/blob/master/src/linting/checks.jl
            # ""julia.lint.call"" = true
            # Other options, see https://github.com/julia-vscode/LanguageServer.jl/blob/master/src/requests/workspace.jl
            # ""julia.lint.run"" = ""true""
        "
    }
}

hook -group lsp-filetype-latex global User LSPDefaultConfig=latex %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [texlab]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            %{
                [texlab.settings.texlab]
                # See https://github.com/latex-lsp/texlab/wiki/Configuration
                #
                # Preview configuration for zathura with SyncTeX search.
                # For other PDF viewers see https://github.com/latex-lsp/texlab/wiki/Previewing
                forwardSearch.executable = ""zathura""
                forwardSearch.args = [
                    ""%p"",
                    ""--synctex-forward"", # Support texlab-forward-search
                    ""%l:1:%f"",
                    ""--synctex-editor-command"", # Inverse search: use Control+Left-Mouse-Button to jump to source.
                    """"""
                        sh -c '
                            echo ""
                                evaluate-commands -client %%opt{texlab_client} %%{
                                    evaluate-commands -try-client %%opt{jumpclient} %%{
                                        edit -- %%{input} %%{line}
                                    }
                                }
                            "" | kak -p $kak_session
                        '
                    """""",
                ]
            }
        "
    }
}

hook -group lsp-filetype-lua global User LSPDefaultConfig=lua %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [lua-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            settings_section = ""Lua""
            [lua-language-server.settings.Lua]
            # See https://github.com/sumneko/vscode-lua/blob/master/setting/schema.json
            # diagnostics.enable = true
        "
    }
}

hook -group lsp-filetype-markdown global User LSPDefaultConfig=markdown %{
    lsp-if-no-servers %{
        set-option -add buffer lsp_servers "
            [marksman]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .marksman.toml $(: kak_buffile)}""
            args = [""server""]
        "
        # set-option -add buffer lsp_servers "
        #     [zk]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" .zk $(: kak_buffile)}""
        #     args = [""lsp""]
        # "
    }
}

hook -group lsp-filetype-nim global User LSPDefaultConfig=nim %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [nimlsp]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" '*.nimble' .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-nix global User LSPDefaultConfig=nix %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [nil]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" flake.nix shell.nix .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-ocaml global User LSPDefaultConfig=ocaml %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [ocamllsp]
            # Often useful to simply do a `touch dune-workspace` in your project root folder if you have problems with root detection
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" dune-workspace dune-project Makefile opam '*.opam' esy.json .git .hg dune $(: kak_buffile)}""
            settings_section = ""_""
            [ocamllsp.settings._]
            # codelens.enable = false
        "
    }
}

hook -group lsp-filetype-php global User LSPDefaultConfig=php %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [intelephense]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .htaccess composer.json $(: kak_buffile)}""
            args = [""--stdio""]
            settings_section = ""intelephense""
            [intelephense.settings.intelephense]
            storagePath = ""/tmp/intelephense""
        "
    }
}

hook -group lsp-filetype-protobuf global User LSPDefaultConfig=protobuf %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [pls] # https://github.com/lasorda/protobuf-language-server
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-purescript global User LSPDefaultConfig=purescript %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [purescript-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" spago.dhall spago.yaml package.json .git .hg $(: kak_buffile)}""
            args = [""--stdio""]
        "
    }
}

hook -group lsp-filetype-python global User LSPDefaultConfig=python %{
    lsp-if-no-servers %{
        set-option -add buffer lsp_servers "
            [pylsp]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" requirements.txt setup.py pyproject.toml .git .hg $(: kak_buffile)}""
            settings_section = ""_""
            [pylsp.settings._]
            # See https://github.com/python-lsp/python-lsp-server#configuration
            # pylsp.configurationSources = [""flake8""]
            pylsp.plugins.jedi_completion.include_params = true
        "
        # set-option -add buffer lsp_servers "
        #     [pyright-langserver]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" requirements.txt setup.py pyproject.toml pyrightconfig.json .git .hg $(: kak_buffile)}""
        #     args = [""--stdio""]
        # "
        # set-option -add buffer lsp_servers "
        #     [ruff-lsp]
        #     root = ""%sh{eval ""$kak_opt_lsp_find_root"" requirements.txt setup.py pyproject.toml .git .hg $(: kak_buffile)}""
        #     settings_section = ""_""
        #     [ruff.settings._.globalSettings]
        #     organizeImports = true
        #     fixAll = true
        # "
    }
}

hook -group lsp-filetype-r global User LSPDefaultConfig=r %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [r-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" DESCRIPTION .git .hg $(: kak_buffile)}""
            command = ""R""
            args = [""--slave"", ""-e"", ""languageserver::run()""]
        "
    }
}

hook -group lsp-filetype-racket global User LSPDefaultConfig=racket %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [racket-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" info.rkt $(: kak_buffile)}""
            command = ""racket""
            args = [""-l"", ""racket-langserver""]
        "
    }
}

hook -group lsp-filetype-reason global User LSPDefaultConfig=reason %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [ocamllsp]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json Makefile .git .hg $(: kak_buffile)}""
        "
    }
}

hook -group lsp-filetype-rust global User LSPDefaultConfig=rust %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [rust-analyzer]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" Cargo.toml $(: kak_buffile)}""
            command = ""sh""
            args = [
                ""-c"",
                """"""
                    if path=$(rustup which rust-analyzer 2>/dev/null); then
                        exec ""$path""
                    else
                        exec rust-analyzer
                    fi
                """""",
            ]
            [rust-analyzer.experimental]
            commands.commands = [""rust-analyzer.runSingle""]
            hoverActions = true
            [rust-analyzer.settings.rust-analyzer]
            # See https://rust-analyzer.github.io/manual.html#configuration
            # cargo.features = []
            check.command = ""clippy""
        "
    }
}

hook -group lsp-filetype-ruby global User LSPDefaultConfig=ruby %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [solargraph]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" Gemfile $(: kak_buffile)}""
            args = [""stdio""]
            settings_section = ""_""
            [solargraph.settings._]
            # See https://github.com/castwide/solargraph/blob/master/lib/solargraph/language_server/host.rb
            # diagnostics = false
        "
    }
}

# See https://scalameta.org/metals/docs/integrations/new-editor
hook -group lsp-filetype-scala global User LSPDefaultConfig=scala %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [metals]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" build.sbt .scala-build $(: kak_buffile)}""
            args = [""-Dmetals.extensions=false""]
            settings_section = ""metals""
            [metals.settings.metals]
            icons = ""unicode""
            isHttpEnabled = true
            statusBarProvider = ""show-message""
            compilerOptions = { overrideDefFormat = ""unicode"" }
            inlayHints.hintsInPatternMatch.enable = true
            inlayHints.implicitArguments.enable = true
            inlayHints.implicitConversions.enable = true
            inlayHints.inferredTypes.enable = true
            inlayHints.typeParameters.enable = true
        "
    }
}

hook -group lsp-filetype-sh global User LSPDefaultConfig=sh %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [bash-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            args = [""start""]
        "
    }
}

hook -group lsp-filetype-svelte global User LSPDefaultConfig=svelte %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [svelteserver]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" package.json tsconfig.json jsconfig.json .git .hg $(: kak_buffile)}""
            args = [""--stdio""]
        "
    }
}

hook -group lsp-filetype-terraform global User LSPDefaultConfig=terraform %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [terraform-ls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" '*.tf' $(: kak_buffile)}""
            args = [""serve""]
            [terraform-ls.settings.terraform-ls]
            # See https://github.com/hashicorp/terraform-ls/blob/main/docs/SETTINGS.md
            # rootModulePaths = []
        "
    }
}

hook -group lsp-filetype-toml global User LSPDefaultConfig=toml %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [taplo]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            args = [""lsp"", ""stdio""]
        "
    }
}

hook -group lsp-filetype-yaml global User LSPDefaultConfig=yaml %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [yaml-language-server]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" .git .hg $(: kak_buffile)}""
            args = [""--stdio""]
            settings_section = ""yaml""
            [yaml-language-server.settings.yaml]
            # See https://github.com/redhat-developer/yaml-language-server#language-server-settings
            # Defaults are at https://github.com/redhat-developer/yaml-language-server/blob/master/src/yamlSettings.ts
            # format.enable = true
        "
    }
}

hook -group lsp-filetype-zig global User LSPDefaultConfig=zig %{
    lsp-if-no-servers %{
        set-option buffer lsp_servers "
            [zls]
            root = ""%sh{eval ""$kak_opt_lsp_find_root"" build.zig $(: kak_buffile)}""
        "
    }
}

### Language ID ###

declare-option -docstring %{LSP languageId, usually same as filetype option

See https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocumentItem
} str lsp_language_id

hook -group lsp global User LSPDefaultConfig=(.*) %{
    set-option buffer lsp_language_id %val{hook_param_capture_1}
}

hook -group lsp global User LSPDefaultConfig=(?:c|cpp) %{
    set-option buffer lsp_language_id c_cpp
}
hook -group lsp global User LSPDefaultConfig=javascript %{
    set-option buffer lsp_language_id javascriptreact
}
hook -group lsp global User LSPDefaultConfig=protobuf %{
    set-option buffer lsp_language_id proto
}
hook -group lsp global User LSPDefaultConfig=sh %{
    set-option buffer lsp_language_id shellscript
}
hook -group lsp global User LSPDefaultConfig=typescript %{
    set-option buffer lsp_language_id typescriptreact
}
