//! Benchmarks of the shell policy lexer (`shellparse::analyze`): it runs on every command an agent
//! wants to execute, before any rule is looked at.
//!
//! No model, no process, no I/O. Run with `cargo bench -p aiw-agent --bench shellparse`; the CI only
//! compiles it (`cargo bench --no-run`). The numbers depend on the machine: see docs/benchmarks.md.

use aiw_agent::shellparse::{analyze, Flavor};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// Command lines of growing difficulty, all taken from what agents really write.
fn posix_lines() -> Vec<(&'static str, String)> {
    let long_chain = (0..100).map(|i| format!("echo step{i} > out{i}.txt")).collect::<Vec<_>>().join(" && ");
    vec![
        ("simple", "ls -la src".to_string()),
        ("pipeline", "cat Cargo.toml | grep -n version | sort | uniq -c | head -n 20".to_string()),
        ("and_or_list", "cd crates/aiw-engine && cargo test -p aiw-engine kb 2>&1 | tail -n 30 || echo failed".to_string()),
        (
            "quoting",
            r#"git commit -m "fix: parser (don't break \"quotes\")" && echo 'it'\''s done' && printf "%s\n" "$HOME/a b""#.to_string(),
        ),
        ("substitution", "echo $(git rev-parse HEAD) `date +%s` ${HOME:-/tmp} $((1 + 2))".to_string()),
        ("wrappers", "sudo env FOO=1 nice -n 5 xargs -I{} sh -c 'rm -rf {} && echo removed {}'".to_string()),
        ("nested_shell", r#"bash -c "sh -c 'cd /tmp && (ls; pwd) | wc -l'""#.to_string()),
        ("chain_100_commands", long_chain),
    ]
}

fn powershell_lines() -> Vec<(&'static str, String)> {
    vec![
        ("simple", "Get-ChildItem -Recurse".to_string()),
        ("pipeline", "Get-Process | Where-Object { $_.CPU -gt 10 } | Sort-Object CPU -Descending | Select-Object -First 5".to_string()),
        ("statements", "$p = Get-Location; Set-Location C:\\Users; Write-Output \"in $p\"; Remove-Item -Force old.txt".to_string()),
    ]
}

fn bench_flavor(c: &mut Criterion, group: &str, flavor: Flavor, lines: Vec<(&'static str, String)>) {
    let mut g = c.benchmark_group(group);
    for (name, line) in &lines {
        g.throughput(Throughput::Bytes(line.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(name), line, |b, line| b.iter(|| analyze(black_box(line), flavor)));
    }
    g.finish();
}

fn shellparse(c: &mut Criterion) {
    bench_flavor(c, "shellparse/posix", Flavor::Posix, posix_lines());
    bench_flavor(c, "shellparse/powershell", Flavor::PowerShell, powershell_lines());
}

criterion_group!(benches, shellparse);
criterion_main!(benches);
