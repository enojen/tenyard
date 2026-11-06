//! One read of the processes under a procfs root: what `account` needs of each process.

use std::fs;
use std::path::Path;

/// `/proc/<pid>/stat` counts CPU in clock ticks of USER_HZ, which is 100 on Linux.
const TICK_MICROS: u64 = 10_000;
/// `rss` of `/proc/<pid>/stat` counts pages; 4 KiB on the x86_64 and arm64 kernels we run.
const PAGE_BYTES: u64 = 4096;

/// A process is its pid and its start time: a reused pid is another process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub start: u64,
    pub pid: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub key: Key,
    pub ppid: u32,
    /// The command line, its words joined by spaces; `[comm]` when it has none.
    pub cmd: String,
    /// Its own CPU: utime + stime, in microseconds.
    pub own: u64,
    /// The CPU of its children it has reaped: cutime + cstime, in microseconds.
    pub child: u64,
    /// Resident memory in bytes.
    pub rss: u64,
}

/// The fields of `/proc/<pid>/stat` that `account` uses.
#[derive(Debug, PartialEq)]
pub struct Stat {
    pub comm: String,
    pub ppid: u32,
    pub own: u64,
    pub child: u64,
    pub start: u64,
    pub rss: u64,
}

/// Parses one `/proc/<pid>/stat` line. The comm can hold spaces and parentheses, so the
/// fields after it are counted from the last `)`.
pub fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_string();
    let rest: Vec<&str> = text.get(close + 2..)?.split_ascii_whitespace().collect();
    // rest[0] is field 3 (state), so field n is rest[n - 3].
    let field = |n: usize| -> Option<u64> {
        rest.get(n - 3)?
            .parse::<i64>()
            .ok()
            .map(|v| v.max(0) as u64)
    };
    Some(Stat {
        comm,
        ppid: u32::try_from(field(4)?).ok()?,
        own: (field(14)? + field(15)?) * TICK_MICROS,
        child: (field(16)? + field(17)?) * TICK_MICROS,
        start: field(22)?,
        rss: field(24)? * PAGE_BYTES,
    })
}

/// Joins the NUL-separated words of `/proc/<pid>/cmdline`.
pub fn parse_cmdline(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let words: Vec<&str> = text.split('\0').filter(|w| !w.is_empty()).collect();
    words.join(" ")
}

/// Whether `/proc/<pid>/cgroup` (cgroup v2: `0::<path>`) puts the process under `under`.
pub fn in_cgroup(cgroup_file: &str, under: &str) -> bool {
    cgroup_file
        .lines()
        .filter_map(|line| line.strip_prefix("0::"))
        .any(|path| path == under || path.starts_with(&format!("{under}/")))
}

/// Every process under `root` that can still be read; with `cgroup`, only those in it.
pub fn snapshot(root: &Path, cgroup: Option<&str>) -> Vec<Proc> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut procs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let dir = entry.path();
        if let Some(under) = cgroup {
            let Ok(file) = fs::read_to_string(dir.join("cgroup")) else {
                continue;
            };
            if !in_cgroup(&file, under) {
                continue;
            }
        }
        // A process that ends while we read it is skipped: the next read misses it anyway.
        let Some(stat) = fs::read_to_string(dir.join("stat"))
            .ok()
            .and_then(|t| parse_stat(&t))
        else {
            continue;
        };
        let words = fs::read(dir.join("cmdline"))
            .map(|b| parse_cmdline(&b))
            .unwrap_or_default();
        let cmd = if words.is_empty() {
            format!("[{}]", stat.comm)
        } else {
            words
        };
        procs.push(Proc {
            key: Key {
                start: stat.start,
                pid,
            },
            ppid: stat.ppid,
            cmd,
            own: stat.own,
            child: stat.child,
            rss: stat.rss,
        });
    }
    procs
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "4242 (bun test (x)) S 7 4242 7 0 -1 4194560 100 0 0 0 150 50 30 20 20 0 4 0 98765 1000000 2500 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 1 0 0 0 0 0";

    #[test]
    fn stat_fields_are_counted_after_the_last_parenthesis() {
        let stat = parse_stat(STAT).unwrap();
        assert_eq!(stat.comm, "bun test (x)");
        assert_eq!(stat.ppid, 7);
        assert_eq!(stat.own, 2_000_000);
        assert_eq!(stat.child, 500_000);
        assert_eq!(stat.start, 98765);
        assert_eq!(stat.rss, 2500 * 4096);
    }

    #[test]
    fn a_cut_stat_line_gives_none() {
        assert_eq!(parse_stat("4242 (bun) S 7 4242"), None);
    }

    #[test]
    fn cmdline_words_are_joined_by_spaces() {
        assert_eq!(parse_cmdline(b"bun\0test\0sim\0"), "bun test sim");
        assert_eq!(parse_cmdline(b""), "");
    }

    #[test]
    fn cgroup_matches_the_path_and_its_children_only() {
        assert!(in_cgroup("0::/a/b.scope\n", "/a/b.scope"));
        assert!(in_cgroup("0::/a/b.scope/test\n", "/a/b.scope"));
        assert!(!in_cgroup("0::/a/b.scope2\n", "/a/b.scope"));
    }

    #[test]
    fn snapshot_reads_every_process_dir_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!("procs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("4242")).unwrap();
        fs::write(root.join("4242/stat"), STAT).unwrap();
        fs::write(root.join("4242/cmdline"), b"bun\0test\0").unwrap();
        fs::create_dir_all(root.join("7")).unwrap();
        fs::write(
            root.join("7/stat"),
            STAT.replacen("4242 (bun test (x))", "7 (pause)", 1),
        )
        .unwrap();
        fs::write(root.join("7/cmdline"), b"").unwrap();
        fs::create_dir_all(root.join("self")).unwrap();
        fs::create_dir_all(root.join("99")).unwrap(); // gone between readdir and read

        let mut procs = snapshot(&root, None);
        procs.sort_by_key(|p| p.key.pid);
        let cmds: Vec<&str> = procs.iter().map(|p| p.cmd.as_str()).collect();
        assert_eq!(cmds, ["[pause]", "bun test"]);
        fs::remove_dir_all(&root).unwrap();
    }
}
