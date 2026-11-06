//! CPU and memory per group, from one snapshot of the processes after another.
//!
//! The CPU of a process is its own time (utime + stime) plus the time of the children it has
//! reaped (cutime + cstime): the kernel adds a child's whole CPU, its own reaped children
//! included, to the parent when the parent waits for it. So a process that lives between two
//! reads is not lost: its CPU shows up in its parent's cutime. Every read adds
//! - the growth of each live process's own time, to its group;
//! - the growth of its cutime, matched to the children that vanished since the last read: what
//!   was already counted of them (their own and cutime as last seen) is taken off, and the rest,
//!   their unseen tail, goes to their groups;
//! - cutime growth with no vanished child, after `HOLD_READS` reads, to the parent's group: those
//!   children were born and reaped between two reads.
//!
//! A vanished child is matched to its nearest live ancestor, so a child and its parent that end
//! in the same interval are counted once. The parent's counter and the child's disappearance can
//! be seen in either order (each process is read at its own instant), so both wait for each other
//! for `HOLD_READS` reads.

use std::collections::{BTreeMap, HashMap};

use crate::groups::{Group, Rules};
use crate::procfs::{Key, Proc};

/// How many reads cutime growth and a vanished child wait for each other.
const HOLD_READS: u32 = 2;
/// How far up the tree a vanished process looks for a live ancestor.
const MAX_HOPS: usize = 64;

#[derive(Debug)]
struct Pending {
    group: String,
    /// own + child of the vanished process when it was last seen: already counted.
    seen: u64,
    age: u32,
}

#[derive(Debug, Default)]
struct Credit {
    amount: u64,
    age: u32,
}

#[derive(Debug)]
struct Known {
    group: Group,
    /// The command line it was grouped by: an open process that execs is grouped again.
    cmd: String,
}

#[derive(Debug, Default)]
pub struct Account {
    last: HashMap<Key, Proc>,
    known: HashMap<Key, Known>,
    pending: HashMap<Key, Vec<Pending>>,
    credit: HashMap<Key, Credit>,
    /// CPU per group, in microseconds, since the first read.
    pub cpu: BTreeMap<String, u64>,
    /// Anonymous memory per group at the last read, in bytes.
    pub anon: BTreeMap<String, u64>,
    /// The highest `anon` of each group at any read.
    pub anon_peak: BTreeMap<String, u64>,
    /// Live processes per group at the last read.
    pub processes: BTreeMap<String, u64>,
}

impl Account {
    pub fn step(&mut self, mut now: Vec<Proc>, rules: &Rules) {
        // A parent starts before its children: group in start order so the parent comes first.
        now.sort_by_key(|p| p.key);
        let now_by_pid: HashMap<u32, Key> = now.iter().map(|p| (p.key.pid, p.key)).collect();
        let keys: std::collections::HashSet<Key> = now.iter().map(|p| p.key).collect();
        self.settle_vanished(&keys, &now_by_pid);
        for gauge in [&mut self.anon, &mut self.processes] {
            gauge.values_mut().for_each(|v| *v = 0);
        }
        for p in &now {
            let group = self.group_of(p, &now_by_pid, rules);
            self.count(p, &group);
        }
        for (name, bytes) in &self.anon {
            let peak = self.anon_peak.entry(name.clone()).or_default();
            *peak = (*peak).max(*bytes);
        }
        self.credit.retain(|k, _| keys.contains(k));
        self.pending.retain(|k, _| keys.contains(k));
        self.last = now.into_iter().map(|p| (p.key, p)).collect();
    }

    fn add(&mut self, group: &str, micros: u64) {
        *self.cpu.entry(group.to_string()).or_default() += micros;
    }

    /// Each process gone since the last read hands what was counted of it to its nearest live
    /// ancestor, whose cutime will hold its final total.
    fn settle_vanished(
        &mut self,
        keys: &std::collections::HashSet<Key>,
        now_by_pid: &HashMap<u32, Key>,
    ) {
        let gone: Vec<Proc> = self
            .last
            .values()
            .filter(|p| !keys.contains(&p.key))
            .cloned()
            .collect();
        let last_by_pid: HashMap<u32, Key> = self.last.keys().map(|k| (k.pid, *k)).collect();
        for p in gone {
            let group = self
                .known
                .remove(&p.key)
                .map_or_else(|| Group::other().name, |k| k.group.name);
            // Its own cutime growth that no child took: children seen by nobody.
            if let Some(credit) = self.credit.remove(&p.key).filter(|c| c.amount > 0) {
                self.add(&group, credit.amount);
            }
            let mut moved = vec![Pending {
                group,
                seen: p.own + p.child,
                age: 0,
            }];
            moved.extend(self.pending.remove(&p.key).unwrap_or_default());
            if let Some(ancestor) = self.live_ancestor(&p, now_by_pid, &last_by_pid) {
                self.pending.entry(ancestor).or_default().extend(moved);
            }
            // Without a live ancestor (reaped outside the read) its unseen tail is lost.
        }
    }

    fn live_ancestor(
        &self,
        p: &Proc,
        now_by_pid: &HashMap<u32, Key>,
        last_by_pid: &HashMap<u32, Key>,
    ) -> Option<Key> {
        let mut ppid = p.ppid;
        for _ in 0..MAX_HOPS {
            let live = now_by_pid.get(&ppid);
            let old = last_by_pid.get(&ppid);
            match (live, old) {
                (Some(live), None) => return Some(*live),
                (Some(live), Some(old)) if live == old => return Some(*live),
                (_, Some(old)) => ppid = self.last.get(old)?.ppid,
                (None, None) => return None,
            }
        }
        None
    }

    fn group_of(&mut self, p: &Proc, now_by_pid: &HashMap<u32, Key>, rules: &Rules) -> Group {
        if let Some(known) = self.known.get(&p.key)
            && (known.group.sticky || known.cmd == p.cmd)
        {
            return known.group.clone();
        }
        let parent = now_by_pid
            .get(&p.ppid)
            .and_then(|k| self.known.get(k))
            .map(|k| &k.group);
        let group = rules.classify(&p.cmd, parent);
        self.known.insert(
            p.key,
            Known {
                group: group.clone(),
                cmd: p.cmd.clone(),
            },
        );
        group
    }

    fn count(&mut self, p: &Proc, group: &Group) {
        let before = self.last.get(&p.key);
        let own = p.own.saturating_sub(before.map_or(0, |b| b.own));
        let child = p.child.saturating_sub(before.map_or(0, |b| b.child));
        self.add(&group.name, own);
        *self.anon.entry(group.name.clone()).or_default() += p.anon;
        *self.processes.entry(group.name.clone()).or_default() += 1;

        let mut credit = self.credit.remove(&p.key).unwrap_or_default();
        credit.amount += child;
        let mut kids = self.pending.remove(&p.key).unwrap_or_default();
        let seen: u64 = kids.iter().map(|k| k.seen).sum();
        if !kids.is_empty() && credit.amount >= seen {
            // The vanished children's final totals arrived: the rest is their unseen tail.
            self.split(&kids, credit.amount - seen, seen);
            credit = Credit::default();
            kids.clear();
        } else if kids.is_empty() && credit.amount > 0 && credit.age >= HOLD_READS {
            self.add(&group.name, credit.amount);
            credit = Credit::default();
        } else {
            if credit.amount > 0 {
                credit.age += 1;
            }
            kids.iter_mut().for_each(|k| k.age += 1);
            // Children reaped by another process, or a counter that never grows: stop waiting.
            if kids.iter().all(|k| k.age > HOLD_READS + 1) {
                kids.clear();
            }
        }
        self.credit.insert(p.key, credit);
        if !kids.is_empty() {
            self.pending.insert(p.key, kids);
        }
    }

    /// Splits the tail of several vanished children by what was seen of each.
    fn split(&mut self, kids: &[Pending], tail: u64, seen: u64) {
        let mut left = tail;
        for (i, kid) in kids.iter().enumerate() {
            let share = if i + 1 == kids.len() {
                left
            } else if seen == 0 {
                tail / kids.len() as u64
            } else {
                (u128::from(tail) * u128::from(kid.seen) / u128::from(seen)) as u64
            };
            left -= share;
            self.add(&kid.group, share);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    const RULES: &str = "
        claude open   ^claude( |$)
        test   sticky \\bbun test\\b
        tsc    sticky \\btsgo\\b
        git    sticky ^git( |$)
    ";

    fn proc(pid: u32, ppid: u32, cmd: &str, own: u64, child: u64) -> Proc {
        Proc {
            key: Key {
                start: u64::from(pid),
                pid,
            },
            ppid,
            cmd: cmd.to_string(),
            own,
            child,
            anon: 0,
        }
    }

    fn run(reads: Vec<Vec<Proc>>) -> Account {
        let rules = Rules::parse(RULES).unwrap();
        let mut account = Account::default();
        for read in reads {
            account.step(read, &rules);
        }
        account
    }

    fn cpu(account: &Account, group: &str) -> u64 {
        account.cpu.get(group).copied().unwrap_or(0)
    }

    fn total(account: &Account) -> u64 {
        account.cpu.values().sum()
    }

    #[test]
    fn a_reaped_child_is_counted_once_with_its_unseen_tail() {
        let a = run(vec![
            vec![proc(10, 1, "claude", 0, 0), proc(20, 10, "bun test", S, 0)],
            vec![proc(10, 1, "claude", 0, 3 * S / 2)],
        ]);
        assert_eq!(cpu(&a, "test"), 3 * S / 2);
        assert_eq!(cpu(&a, "claude"), 0);
    }

    #[test]
    fn children_never_seen_count_in_the_parents_group_after_the_hold() {
        let base = |child| vec![proc(10, 1, "claude", 0, child)];
        let a = run(vec![base(0), base(2 * S), base(2 * S), base(2 * S)]);
        assert_eq!(cpu(&a, "claude"), 2 * S);
    }

    #[test]
    fn a_parent_counter_seen_before_the_child_vanishes_is_not_counted_twice() {
        // The child is read alive, then reaped, then the parent is read: one read shows both.
        let a = run(vec![
            vec![proc(10, 1, "claude", 0, 0), proc(20, 10, "bun test", S, 0)],
            vec![
                proc(10, 1, "claude", 0, 3 * S / 2),
                proc(20, 10, "bun test", S, 0),
            ],
            vec![proc(10, 1, "claude", 0, 3 * S / 2)],
        ]);
        assert_eq!(cpu(&a, "test"), 3 * S / 2);
        assert_eq!(total(&a), 3 * S / 2);
    }

    #[test]
    fn a_child_and_its_parent_ending_together_are_counted_once() {
        // bash runs bunx, bunx runs tsgo; bunx and tsgo end in the same interval.
        let a = run(vec![
            vec![
                proc(10, 1, "bash -c x", 0, 0),
                proc(20, 10, "bunx tsgo", S / 2, 0),
                proc(30, 20, "tsgo --noEmit", 2 * S, 0),
            ],
            vec![proc(10, 1, "bash -c x", 0, 3 * S)],
        ]);
        assert_eq!(cpu(&a, "tsc"), 3 * S);
        assert_eq!(total(&a), 3 * S);
    }

    #[test]
    fn a_vanished_process_hands_its_unmatched_counter_to_its_group() {
        // tsgo's own child was reaped unseen; then tsgo ends before that growth was matched.
        let a = run(vec![
            vec![proc(10, 1, "bash -c x", 0, 0), proc(20, 10, "tsgo", S, 0)],
            vec![proc(10, 1, "bash -c x", 0, 0), proc(20, 10, "tsgo", S, S)],
            vec![proc(10, 1, "bash -c x", 0, 2 * S)],
        ]);
        assert_eq!(cpu(&a, "tsc"), 2 * S);
        assert_eq!(total(&a), 2 * S);
    }

    #[test]
    fn an_open_parent_lets_a_matching_child_take_its_own_group() {
        let a = run(vec![vec![
            proc(10, 1, "claude", S, 0),
            proc(20, 10, "git log", S, 0),
            proc(30, 10, "grep x", S, 0),
        ]]);
        assert_eq!(cpu(&a, "claude"), 2 * S);
        assert_eq!(cpu(&a, "git"), S);
    }

    #[test]
    fn a_sticky_parent_keeps_its_children() {
        let a = run(vec![vec![
            proc(10, 1, "bun test", S, 0),
            proc(20, 10, "git init", S, 0),
        ]]);
        assert_eq!(cpu(&a, "test"), 2 * S);
    }

    #[test]
    fn an_open_process_that_execs_is_grouped_again() {
        let a = run(vec![
            vec![
                proc(10, 1, "claude", 0, 0),
                proc(20, 10, "bash -c exec git log", 0, 0),
            ],
            vec![proc(10, 1, "claude", 0, 0), proc(20, 10, "git log", S, 0)],
        ]);
        assert_eq!(cpu(&a, "git"), S);
    }

    #[test]
    fn a_reused_pid_is_another_process() {
        let mut later = proc(20, 10, "bun test", S / 4, 0);
        later.key.start = 999;
        let a = run(vec![
            vec![proc(10, 1, "claude", 0, 0), proc(20, 10, "git log", S, 0)],
            vec![proc(10, 1, "claude", 0, S), later],
        ]);
        assert_eq!(cpu(&a, "git"), S);
        assert_eq!(cpu(&a, "test"), S / 4);
    }

    #[test]
    fn memory_is_summed_per_group_and_its_peak_kept() {
        let mut big = proc(20, 10, "bun test", 0, 0);
        big.anon = 300;
        let mut small = proc(20, 10, "bun test", 0, 0);
        small.anon = 100;
        let a = run(vec![
            vec![proc(10, 1, "claude", 0, 0), big],
            vec![proc(10, 1, "claude", 0, 0), small],
        ]);
        assert_eq!(a.anon.get("test"), Some(&100));
        assert_eq!(a.anon_peak.get("test"), Some(&300));
        assert_eq!(a.processes.get("claude"), Some(&1));
    }
}
