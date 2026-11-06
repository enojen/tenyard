//! The Prometheus text of an account.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::account::Account;

const MICROS_PER_SECOND: f64 = 1_000_000.0;

pub fn render(account: &Account) -> String {
    let mut out = String::new();
    let seconds: BTreeMap<String, f64> = account
        .cpu
        .iter()
        .map(|(g, us)| (g.clone(), *us as f64 / MICROS_PER_SECOND))
        .collect();
    family(
        &mut out,
        "tenyard_procs_cpu_seconds_total",
        "counter",
        "CPU of the group's processes, their reaped children included.",
        &seconds,
    );
    let as_f64 =
        |m: &BTreeMap<String, u64>| m.iter().map(|(g, v)| (g.clone(), *v as f64)).collect();
    family(
        &mut out,
        "tenyard_procs_anon_bytes",
        "gauge",
        "Anonymous memory (heap, stacks) of the group's live processes; no page of a file.",
        &as_f64(&account.anon),
    );
    family(
        &mut out,
        "tenyard_procs_anon_peak_bytes",
        "gauge",
        "Highest anonymous memory of the group at any read.",
        &as_f64(&account.anon_peak),
    );
    family(
        &mut out,
        "tenyard_procs_processes",
        "gauge",
        "Live processes of the group.",
        &as_f64(&account.processes),
    );
    out
}

fn family(out: &mut String, name: &str, kind: &str, help: &str, values: &BTreeMap<String, f64>) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
    for (group, value) in values {
        let _ = writeln!(out, "{name}{{group=\"{group}\"}} {value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_group_is_one_line_per_family() {
        let mut account = Account::default();
        account.cpu.insert("test".to_string(), 1_500_000);
        account.anon.insert("test".to_string(), 4096);
        let text = render(&account);
        assert!(
            text.contains("# TYPE tenyard_procs_cpu_seconds_total counter\n"),
            "{text}"
        );
        assert!(
            text.contains("tenyard_procs_cpu_seconds_total{group=\"test\"} 1.5\n"),
            "{text}"
        );
        assert!(
            text.contains("tenyard_procs_anon_bytes{group=\"test\"} 4096\n"),
            "{text}"
        );
    }
}
