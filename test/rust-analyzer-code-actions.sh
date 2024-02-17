#!/bin/sh

# REQUIRES: command -v rust-analyzer

user_home=$HOME

. test/lib.sh

cat > .config/kak-lsp/kak-lsp.toml << EOF
[language.rust]
filetypes = ["rust"]
roots = ["Cargo.toml"]
command = "sh"
args = ["-c", "RUSTUP_HOME=$user_home/.rustup rust-analyzer"]
EOF

cat >> .config/kak/kakrc << EOF
set-option global lsp_auto_show_code_actions true
EOF

cat > main.rs << EOF
enum Test {
    Foo,
    Bar,
}
fn main() {
    let foo: Test = Test::Foo;
    match foo {
        Test::Foo => println!("foo"),
        _ => (),
    }
}
EOF

test_tmux_kak_start 'edit main.rs'

test_tmux send-keys j/foo Enter vtj
test_sleep_until 'test_tmux capture-pane -p | grep -E "main (💡|\[A\]) "'
# CHECK: main {{💡|\[A\]}} main.rs 7:11  1 sel - client0@[session]

test_tmux send-keys :lsp-code-actions Enter
test_sleep_until 'test_tmux capture-pane -p | grep Replace'
# CHECK: {{.*}}Replace match with if let{{.*}}

test_tmux send-keys iflet
test_sleep
test_tmux send-keys Enter
test_sleep_until 'test_tmux capture-pane -p | grep -q if.let.Test'
test_tmux capture-pane -p
# CHECK:     let foo: Test = Test::Foo;
# CHECK:     if let Test::Foo = foo {
# CHECK:         println!("foo")
# CHECK:     }
# CHECK: }
# CHECK: ~
# CHECK: main {{💡|\[A\]}} main.rs 7:27 [+] 1 sel - client0@[session]

test_tmux send-keys :lsp-code-actions Enter
test_sleep_until 'test_tmux capture-pane -p | grep Replace'
# CHECK: {{.*}}Replace if let with match{{.*}}

test_tmux send-keys iflet
test_sleep
test_tmux send-keys Enter
test_sleep_until 'test_tmux capture-pane -p | grep -q match.foo'
test_tmux capture-pane -p
# CHECK:      let foo: Test = Test::Foo;
# CHECK:      match foo {
# CHECK:          Test::Foo => println!("foo"),
# CHECK:          _ => (),
# CHECK:      }
# CHECK: }
# CHECK: main {{💡|\[A\]}} main.rs 7:14 [+] 1 sel - client0@[session]
