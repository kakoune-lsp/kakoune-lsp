#!/bin/sh

# Integration test for the LSP diagnostic pull model against the modern
# vscode-eslint-language-server (advertises diagnosticProvider).

# REQUIRES: command -v vscode-eslint-language-server
# REQUIRES: command -v eslint
# REQUIRES: command -v npm

. test/lib.sh

cat >> .config/kak/kakrc << 'KAK'
set-option global lsp_diagnostic_line_error_sign   ' X'
set-option global lsp_diagnostic_line_warning_sign 'W '

hook global BufSetOption filetype=javascript %{
	set-option buffer lsp_servers %{
		[eslint-language-server]
		root_globs = ["package.json", "eslint.config.js", ".eslintrc.json"]
		command = "vscode-eslint-language-server"
		args = ["--stdio"]
		workaround_eslint = true
		[eslint-language-server.settings]
		codeActionsOnSave = { mode = "all", "source.fixAll.eslint" = true }
		format = { enable = false }
		quiet = false
		rulesCustomizations = []
		run = "onType"
		validate = "on"
		packageManager = "npm"
		experimental = {}
		problems = { shortenToSingleLine = false }
		codeAction.disableRuleComment = { enable = true, location = "separateLine" }
		codeAction.showDocumentation = { enable = false }
		workingDirectory.mode = "auto"
	}
}
KAK

# Local eslint so the language server can resolve the library from the project.
npm init -y >/dev/null 2>&1
npm install --no-save --no-package-lock eslint@9 @eslint/js@9 >/dev/null 2>&1

cat > eslint.config.js << 'ESLINT'
import js from "@eslint/js";

export default [
	js.configs.recommended,
	{
		rules: {
			"no-unused-vars": "error",
			"no-undef": "error",
		},
	},
];
ESLINT

# Ensure package.json marks the project as ESM for the flat config.
python3 - <<'PY'
import json
from pathlib import Path
p = Path("package.json")
data = json.loads(p.read_text())
data["type"] = "module"
data["private"] = True
p.write_text(json.dumps(data, indent=2) + "\n")
PY

cat > main.js << 'JS'
const unused = 1;
console.log(missing);
JS

test_tmux_kak_start 'edit main.js'
test_sleep_until 'test_tmux capture-pane -p | grep -qE "X|W "'
test_tmux capture-pane -p | sed 2q
# CHECK: {{( X|W )}}const unused = 1;
# CHECK: {{( X|W )}}console.log(missing);

# Fix the diagnostics and ensure pull clears the gutter.
cat > main.js << 'JS'
console.log(1);
JS
printf '%s\n' 'evaluate-commands -client client0 %{
	edit! main.js
}' | kak -p "$test_kak_session"
test_sleep_until '! test_tmux capture-pane -p | grep -qE "X|W "'
test_tmux capture-pane -p | sed 1q
# CHECK: {{  }}console.log(1);
