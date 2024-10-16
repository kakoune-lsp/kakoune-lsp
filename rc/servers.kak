### Language Servers ###
#
# For help, see the docstring of the 'lsp_servers' option.

define-command -hidden lsp-load-default-config -params 1 -docstring %{
    Register the given BufSetOption filetype hooks in a way that they only execute after LSP
    is enabled, and that they never overwrite a non-empty 'lsp_servers'.
} %{
    evaluate-commands %sh{
        printf %s "$1" |
            sed -e '/-group lsp-filetype/s/BufSetOption filetype=/User LSPDefaultConfig=/'
    }
}

lsp-load-default-config %{

hook -group lsp-filetype-c-family global BufSetOption filetype=(?:c|cpp|objc) %{
    set-option buffer lsp_servers %{
        [clangd]
        args = ["--log=error"]
        root_globs = ["compile_commands.json", ".clangd", ".git", ".hg"]
    }
}

hook -group lsp-filetype-clojure global BufSetOption filetype=clojure %{
    set-option buffer lsp_servers %{
        [clojure-lsp]
        root_globs = ["project.clj", ".git", ".hg"]
        settings_section = "_"
        [clojure-lsp.settings._]
        # See https://clojure-lsp.io/settings/#all-settings
        # source-paths-ignore-regex = ["resources.*", "target.*"]
    }
}

hook -group lsp-filetype-cmake global BufSetOption filetype=make %{
    set-option buffer lsp_servers %{
        [cmake-language-server]
        root_globs = ["CMakeLists.txt", ".git", ".hg"]
    }
}

hook -group lsp-filetype-crystal global BufSetOption filetype=crystal %{
    set-option buffer lsp_servers %{
        [crystalline]
        root_globs = ["shard.yml"]
    }
}

hook -group lsp-filetype-css global BufSetOption filetype=(?:css|less|scss) %{
    set-option buffer lsp_servers %{
        [vscode-css-language-server]
        root_globs = ["package.json", ".git", ".hg"]
        args = ["--stdio"]
    }
}

hook -group lsp-filetype-d global BufSetOption filetype=(?:d|di) %{
    set-option buffer lsp_servers %{
        [dls]
        root_globs = [".git", "dub.sdl", "dub.json"]
    }
}

hook -group lsp-filetype-dart global BufSetOption filetype=dart %{
    set-option buffer lsp_servers %{
        [dart-lsp]
        root_globs = ["pubspec.yaml", ".git", ".hg"]
        # start shell to find path to dart analysis server source
        command = "sh"
        args = ["-c", "dart \"$(dirname \"$(command -v dart)\")\"/snapshots/analysis_server.dart.snapshot --lsp"]
    }
}

hook -group lsp-filetype-elixir global BufSetOption filetype=elixir %{
    set-option buffer lsp_servers %{
        [elixir-ls]
        root_globs = ["mix.exs"]
        settings_section = "elixirLS"
        [elixir-ls.settings.elixirLS]
        # See https://github.com/elixir-lsp/elixir-ls/blob/master/apps/language_server/lib/language_server/server.ex
        # dialyzerEnable = true
    }
}

hook -group lsp-filetype-elm global BufSetOption filetype=elm %{
    set-option buffer lsp_servers %{
        [elm-language-server]
        root_globs = ["elm.json"]
        args = ["--stdio"]
        settings_section = "elmLS"
        [elm-language-server.settings.elmLS]
        # See https://github.com/elm-tooling/elm-language-server#server-settings
        runtime = "node"
        elmPath = "elm"
        elmFormatPath = "elm-format"
        elmTestPath = "elm-test"
    }
}

hook -group lsp-filetype-elvish global BufSetOption filetype=elvish %{
    set-option buffer lsp_servers %{
        [elvish]
        root_globs = [".git", ".hg"]
        args = ["-lsp"]
    }
}

hook -group lsp-filetype-erlang global BufSetOption filetype=erlang %{
    set-option buffer lsp_servers %{
        [erlang_ls]
        root_globs = ["rebar.config", "erlang.mk", ".git", ".hg"]
        # See https://github.com/erlang-ls/erlang_ls.git for more information and
        # how to configure. This default config should work in most cases though.
    }
}

hook -group lsp-filetype-go global BufSetOption filetype=go %{
    set-option buffer lsp_servers %{
        [gopls]
        root_globs = ["Gopkg.toml", "go.mod", ".git", ".hg"]
        [gopls.settings.gopls]
        # See https://github.com/golang/tools/blob/master/gopls/doc/settings.md
        # "build.buildFlags" = []
        hints.assignVariableTypes = true
        hints.compositeLiteralFields = true
        hints.compositeLiteralTypes = true
        hints.constantValues = true
        hints.functionTypeParameters = true
        hints.parameterNames = true
        hints.rangeVariableTypes = true
        usePlaceholders = true
    }
}

hook -group lsp-filetype-haskell global BufSetOption filetype=haskell %{
    set-option buffer lsp_servers %{
        [haskell-language-server]
        root_globs = ["hie.yaml", "cabal.project", "Setup.hs", "stack.yaml", "*.cabal"]
        command = "haskell-language-server-wrapper"
        args = ["--lsp"]
        settings_section = "_"
        [haskell-language-server.settings._]
        # See https://haskell-language-server.readthedocs.io/en/latest/configuration.html
        # haskell.formattingProvider = "ormolu"
    }
}

hook -group lsp-filetype-html global BufSetOption filetype=html %{
    set-option buffer lsp_servers %{
        [vscode-html-language-server]
        root_globs = ["package.json"]
        args = ["--stdio"]
        settings_section = "_"
        [vscode-html-language-server.settings._]
        # quotePreference = "single"
        # javascript.format.semicolons = "insert"
    }
}

hook -group lsp-filetype-javascript global BufSetOption filetype=(?:javascript|typescript) %{
    set-option -add buffer lsp_servers %{
        [typescript-language-server]
        root_globs = ["package.json", "tsconfig.json", "jsconfig.json", ".git", ".hg"]
        args = ["--stdio"]
        settings_section = "_"
        [typescript-language-server.settings._]
        # quotePreference = "double"
        # typescript.format.semicolons = "insert"
    }
    # set-option -add buffer lsp_servers %{
    #     [deno]
    #     root_globs = ["package.json", "tsconfig.json", ".git", ".hg"]
    #     args = ["lsp"]
    #     settings_section = "deno"
    #     [deno.settings.deno]
    #     enable = true
    #     lint = true
    # }
    # set-option -add buffer lsp_servers %{
    #     [biome]
    #     root_globs = ["biome.json", "package.json", "tsconfig.json", "jsconfig.json", ".git", ".hg"]
    #     args = ["lsp-proxy"]
    # }
    # set-option -add buffer lsp_servers %{
    #     [eslint-language-server]
    #     root_globs = [".eslintrc", ".eslintrc.json"]
    #     args = ["--stdio"]
    #     workaround_eslint = true
    #     [eslint-language-server.settings]
    #     codeActionsOnSave = { mode = "all", "source.fixAll.eslint" = true }
    #     format = { enable = true }
    #     quiet = false
    #     rulesCustomizations = []
    #     run = "onType"
    #     validate = "on"
    #     experimental = {}
    #     problems = { shortenToSingleLine = false }
    #     codeAction.disableRuleComment = { enable = true, location = "separateLine" }
    #     codeAction.showDocumentation = { enable = false }
    # }
}

hook -group lsp-filetype-java global BufSetOption filetype=java %{
    set-option buffer lsp_servers %{
        [jdtls]
        root_globs = ["mvnw", "gradlew", ".git", ".hg"]
        [jdtls.settings]
        # See https://github.dev/eclipse/eclipse.jdt.ls
        # "java.format.insertSpaces" = true
    }
}

hook -group lsp-filetype-json global BufSetOption filetype=json %{
    set-option buffer lsp_servers %{
        [vscode-json-language-server]
        root_globs = ["package.json"]
        args = ["--stdio"]
    }
}

hook -group lsp-filetype-julia global BufSetOption filetype=julia %{
    set-option buffer lsp_servers %{
        # Requires Julia package "LanguageServer"
        root_globs = ["Project.toml", ".git", ".hg"]
        # Run: `julia --project=@kak-lsp -e 'import Pkg; Pkg.add("LanguageServer")'` to install it
        # Configuration adapted from https://github.com/neovim/nvim-lspconfig/blob/bcebfac7429cd8234960197dca8de1767f3ef5d3/lua/lspconfig/julials.lua
        [julia-language-server]
        command = "julia"
        args = [
            "--startup-file=no",
            "--history-file=no",
            "-e",
            """
            ls_install_path = joinpath(get(DEPOT_PATH, 1, joinpath(homedir(), ".julia")), "environments", "kak-lsp");
            pushfirst!(LOAD_PATH, ls_install_path);
            using LanguageServer;
            popfirst!(LOAD_PATH);
            depot_path = get(ENV, "JULIA_DEPOT_PATH", "");
            server = LanguageServer.LanguageServerInstance(stdin, stdout, "", depot_path);
            server.runlinter = true;
            run(server);
            """,
        ]
        [julia-language-server.settings]
        # See https://github.com/julia-vscode/LanguageServer.jl/blob/master/src/requests/workspace.jl
        # Format options. See https://github.com/julia-vscode/DocumentFormat.jl/blob/master/src/DocumentFormat.jl
        # "julia.format.indent" = 4
        # Lint options. See https://github.com/julia-vscode/StaticLint.jl/blob/master/src/linting/checks.jl
        # "julia.lint.call" = true
        # Other options, see https://github.com/julia-vscode/LanguageServer.jl/blob/master/src/requests/workspace.jl
        # "julia.lint.run" = "true"
    }
}

hook -group lsp-filetype-latex global BufSetOption filetype=latex %{
    set-option buffer lsp_servers %{
        [texlab]
        root_globs = [".git", ".hg"]
        [texlab.settings.texlab]
        # See https://github.com/latex-lsp/texlab/wiki/Configuration
        #
        # Preview configuration for zathura with SyncTeX search.
        # For other PDF viewers see https://github.com/latex-lsp/texlab/wiki/Previewing
        forwardSearch.executable = "zathura"
        forwardSearch.args = [
            "%p",
            "--synctex-forward", # Support texlab-forward-search
            "%l:1:%f",
            "--synctex-editor-command", # Inverse search: use Control+Left-Mouse-Button to jump to source.
            """
                sh -c '
                    echo "
                        evaluate-commands -client %%opt{texlab_client} %%{
                            evaluate-commands -try-client %%opt{jumpclient} %%{
                                edit -- %%{input} %%{line}
                            }
                        }
                    " | kak -p $kak_session
                '
            """,
        ]
    }
}

hook -group lsp-filetype-lua global BufSetOption filetype=lua %{
    set-option buffer lsp_servers %{
        [lua-language-server]
        root_globs = [".git", ".hg"]
        settings_section = "Lua"
        [lua-language-server.settings.Lua]
        # See https://github.com/sumneko/vscode-lua/blob/master/setting/schema.json
        # diagnostics.enable = true
    }
}

hook -group lsp-filetype-markdown global BufSetOption filetype=markdown %{
    set-option -add buffer lsp_servers %{
        [marksman]
        root_globs = [".marksman.toml"]
        args = ["server"]
    }
    # set-option -add buffer lsp_servers %{
    #     [zk]
    #     root_globs = [".zk"]
    #     args = ["lsp"]
    # }
}

hook -group lsp-filetype-mojo global BufSetOption filetype=mojo %{
    set-option buffer lsp_servers %{
        [mojo-lsp-server]
        root_globs = [".git", ".hg"]
    }
}

hook -group lsp-filetype-nim global BufSetOption filetype=nim %{
    set-option buffer lsp_servers %{
        [nimlsp]
        root_globs = ["*.nimble", ".git", ".hg"]
    }
}

hook -group lsp-filetype-nix global BufSetOption filetype=nix %{
    set-option buffer lsp_servers %{
        [nil]
        root_globs = ["flake.nix", "shell.nix", ".git", ".hg"]
    }
}

hook -group lsp-filetype-ocaml global BufSetOption filetype=ocaml %{
    set-option buffer lsp_servers %{
        [ocamllsp]
        # Often useful to simply do a `touch dune-workspace` in your project root folder if you have problems with root detection
        root_globs = ["dune-workspace", "dune-project", "Makefile", "opam", "*.opam", "esy.json", ".git", ".hg", "dune"]
        settings_section = "_"
        [ocamllsp.settings._]
        # codelens.enable = false
    }
}

hook -group lsp-filetype-php global BufSetOption filetype=php %{
    set-option buffer lsp_servers %{
        [intelephense]
        root_globs = [".htaccess", "composer.json"]
        args = ["--stdio"]
        settings_section = "intelephense"
        [intelephense.settings.intelephense]
        storagePath = "/tmp/intelephense"
        # [phpactor]
        # root_globs = ["composer.json", ".phpactor.json", ".phpactor.yml", ".git", ".hg"]
        # args = ["language-server"]
    }
}

hook -group lsp-filetype-protobuf global BufSetOption filetype=protobuf %{
    set-option buffer lsp_servers %{
        [pls] # https://github.com/lasorda/protobuf-language-server
        root_globs = [".git", ".hg"]
    }
}

hook -group lsp-filetype-purescript global BufSetOption filetype=purescript %{
    set-option buffer lsp_servers %{
        [purescript-language-server]
        root_globs = ["spago.dhall", "spago.yaml", "package.json", ".git", ".hg"]
        args = ["--stdio"]
    }
}

hook -group lsp-filetype-python global BufSetOption filetype=python %{
    set-option -add buffer lsp_servers %{
        [pylsp]
        root_globs = ["requirements.txt", "setup.py", "pyproject.toml", ".git", ".hg"]
        settings_section = "_"
        [pylsp.settings._]
        # See https://github.com/python-lsp/python-lsp-server#configuration
        # pylsp.configurationSources = ["flake8"]
        pylsp.plugins.jedi_completion.include_params = true
    }
    # set-option -add buffer lsp_servers %{
    #     [pyright-langserver]
    #     root_globs = ["requirements.txt", "setup.py", "pyproject.toml", "pyrightconfig.json", ".git", ".hg"]
    #     args = ["--stdio"]
    # }
    # set-option -add buffer lsp_servers %{
    #     [ruff]
    #     args = ["server", "--quiet"]
    #     root_globs = ["requirements.txt", "setup.py", "pyproject.toml", ".git", ".hg"]
    #     settings_section = "_"
    #     [ruff.settings._.globalSettings]
    #     organizeImports = true
    #     fixAll = true
    # }
}

hook -group lsp-filetype-r global BufSetOption filetype=r %{
    set-option buffer lsp_servers %{
        [r-language-server]
        root_globs = ["DESCRIPTION", ".git", ".hg"]
        command = "R"
        args = ["--slave", "-e", "languageserver::run()"]
    }
}

hook -group lsp-filetype-racket global BufSetOption filetype=racket %{
    set-option buffer lsp_servers %{
        [racket-language-server]
        root_globs = ["info.rkt"]
        command = "racket"
        args = ["-l", "racket-langserver"]
    }
}

hook -group lsp-filetype-reason global BufSetOption filetype=reason %{
    set-option buffer lsp_servers %{
        [ocamllsp]
        root_globs = ["package.json", "Makefile", ".git", ".hg"]
    }
}

hook -group lsp-filetype-rust global BufSetOption filetype=rust %{
    set-option buffer lsp_servers %{
        [rust-analyzer]
        root_globs = ["Cargo.toml"]
        command = "sh"
        args = [
            "-c",
            """
                if path=$(rustup which rust-analyzer 2>/dev/null); then
                    exec "$path"
                else
                    exec rust-analyzer
                fi
            """,
        ]
        [rust-analyzer.experimental]
        commands.commands = ["rust-analyzer.runSingle"]
        hoverActions = true
        [rust-analyzer.settings.rust-analyzer]
        # See https://rust-analyzer.github.io/manual.html#configuration
        # cargo.features = []
        check.command = "clippy"
    }
}

hook -group lsp-filetype-ruby global BufSetOption filetype=ruby %{
    set-option buffer lsp_servers %{
        [solargraph]
        root_globs = ["Gemfile"]
        args = ["stdio"]
        settings_section = "_"
        [solargraph.settings._]
        # See https://github.com/castwide/solargraph/blob/master/lib/solargraph/language_server/host.rb
        # diagnostics = false
    }
}

# See https://scalameta.org/metals/docs/integrations/new-editor
hook -group lsp-filetype-scala global BufSetOption filetype=scala %{
    set-option buffer lsp_servers %{
        [metals]
        root_globs = ["build.sbt", ".scala-build"]
        args = ["-Dmetals.extensions=false"]
        settings_section = "metals"
        [metals.settings.metals]
        icons = "unicode"
        isHttpEnabled = true
        statusBarProvider = "show-message"
        compilerOptions = { overrideDefFormat = "unicode" }
        inlayHints.hintsInPatternMatch.enable = true
        inlayHints.implicitArguments.enable = true
        inlayHints.implicitConversions.enable = true
        inlayHints.inferredTypes.enable = true
        inlayHints.typeParameters.enable = true
    }
}

hook -group lsp-filetype-sh global BufSetOption filetype=sh %{
    set-option buffer lsp_servers %{
        [bash-language-server]
        root_globs = [".git", ".hg"]
        args = ["start"]
    }
}

hook -group lsp-filetype-svelte global BufSetOption filetype=svelte %{
    set-option buffer lsp_servers %{
        [svelteserver]
        root_globs = ["package.json", "tsconfig.json", "jsconfig.json", ".git", ".hg"]
        args = ["--stdio"]
    }
}

hook -group lsp-filetype-terraform global BufSetOption filetype=terraform %{
    set-option buffer lsp_servers %{
        [terraform-ls]
        root_globs = ["*.tf"]
        args = ["serve"]
        [terraform-ls.settings.terraform-ls]
        # See https://github.com/hashicorp/terraform-ls/blob/main/docs/SETTINGS.md
        # rootModulePaths = []
    }
}

hook -group lsp-filetype-toml global BufSetOption filetype=toml %{
    set-option buffer lsp_servers %{
        [taplo]
        root_globs = [".git", ".hg"]
        args = ["lsp", "stdio"]
    }
}

hook -group lsp-filetype-yaml global BufSetOption filetype=yaml %{
    set-option buffer lsp_servers %{
        [yaml-language-server]
        root_globs = [".git", ".hg"]
        args = ["--stdio"]
        settings_section = "yaml"
        [yaml-language-server.settings.yaml]
        # See https://github.com/redhat-developer/yaml-language-server#language-server-settings
        # Defaults are at https://github.com/redhat-developer/yaml-language-server/blob/master/src/yamlSettings.ts
        # format.enable = true
    }
}

hook -group lsp-filetype-zig global BufSetOption filetype=zig %{
    set-option buffer lsp_servers %{
        [zls]
        root_globs = ["build.zig"]
    }
}

}

### Language ID ###

hook -group lsp-language-id global BufSetOption filetype=(.*) %{
    set-option buffer lsp_language_id %val{hook_param_capture_1}
}

hook -group lsp-language-id global BufSetOption filetype=(?:c|cpp) %{
    set-option buffer lsp_language_id c_cpp
}
hook -group lsp-language-id global BufSetOption filetype=javascript %{
    set-option buffer lsp_language_id javascriptreact
}
hook -group lsp-language-id global BufSetOption filetype=protobuf %{
    set-option buffer lsp_language_id proto
}
hook -group lsp-language-id global BufSetOption filetype=sh %{
    set-option buffer lsp_language_id shellscript
}
hook -group lsp-language-id global BufSetOption filetype=typescript %{
    set-option buffer lsp_language_id typescriptreact
}
