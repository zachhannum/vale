//! Seeded local search (simulated annealing after Christensen, Marks, and
//! Shieber 1995), changed so that labels never overlap during the search: a
//! move that needs occupied space removes the labels in the way.

use crate::engine::{Instance, State};
use std::collections::HashMap;

/// SplitMix64, a small seeded generator.
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1) from the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, k: usize) -> usize {
        (self.next_u64() % k as u64) as usize
    }
}

/// The duplicate rule: same class and text, different instances, and centers
/// of the label bounds closer than `distance`.
pub(crate) struct Dup {
    distance: Option<f64>,
    /// Instances that share a (class, text) pair, by group.
    members: Vec<Vec<usize>>,
    group: Vec<usize>,
}

impl Dup {
    pub fn new(keys: Vec<(usize, String)>, distance: Option<f64>) -> Self {
        let mut ids: HashMap<(usize, String), usize> = HashMap::new();
        let mut members: Vec<Vec<usize>> = Vec::new();
        let mut group = Vec::with_capacity(keys.len());
        for (i, k) in keys.into_iter().enumerate() {
            let g = *ids.entry(k).or_insert_with(|| {
                members.push(Vec::new());
                members.len() - 1
            });
            members[g].push(i);
            group.push(g);
        }
        Dup {
            distance,
            members,
            group,
        }
    }

    fn add_conflicts(
        &self,
        instances: &[Instance],
        state: &State,
        inst: usize,
        cand: usize,
        out: &mut Vec<usize>,
    ) {
        let Some(d) = self.distance else { return };
        let c = instances[inst].candidates[cand].bounds.center();
        for &m in &self.members[self.group[inst]] {
            if m == inst {
                continue;
            }
            if let Some(mc) = state.chosen[m]
                && instances[m].candidates[mc].bounds.center().distance(c) < d
            {
                out.push(m);
            }
        }
    }
}

/// Every placed instance other than `inst` that conflicts with candidate
/// `cand` of `inst`, sorted and without repeats.
pub(crate) fn conflicts(
    instances: &[Instance],
    state: &mut State,
    dup: &Dup,
    inst: usize,
    cand: usize,
    out: &mut Vec<usize>,
) {
    out.clear();
    state.hits(instances, &instances[inst].candidates[cand], inst, out);
    dup.add_conflicts(instances, state, inst, cand, out);
    out.sort_unstable();
    out.dedup();
}

/// Penalty of leaving instance `i` unplaced, indexed by instance.
pub(crate) fn penalties(instances: &[Instance], order: &[usize]) -> Vec<f64> {
    let active: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| !instances[i].candidates.is_empty())
        .collect();
    let n = active.len() as f64;
    let mut pen = vec![0.0; instances.len()];
    for (rank, &i) in active.iter().enumerate() {
        pen[i] = 100.0 + 100.0 * (n - rank as f64) / n;
    }
    pen
}

/// The energy: costs of placed labels plus penalties of unplaced ones that
/// had candidates.
pub(crate) fn energy(instances: &[Instance], state: &State, pen: &[f64]) -> f64 {
    let mut e = 0.0;
    for (i, inst) in instances.iter().enumerate() {
        match state.chosen[i] {
            Some(ci) => e += inst.candidates[ci].cost,
            None if !inst.candidates.is_empty() => e += pen[i],
            None => {}
        }
    }
    e
}

fn restore(instances: &[Instance], state: &mut State, best: &[Option<usize>]) {
    for i in 0..instances.len() {
        state.remove(instances, i);
    }
    for (i, c) in best.iter().enumerate() {
        if let Some(c) = *c {
            state.insert(instances, i, c);
        }
    }
}

/// Improve the greedy state in place. `order` is the priority order.
pub(crate) fn improve(
    instances: &[Instance],
    state: &mut State,
    order: &[usize],
    dup: &Dup,
    pen: &[f64],
    seed: u64,
) {
    let active: Vec<usize> = (0..instances.len())
        .filter(|&i| !instances[i].candidates.is_empty())
        .collect();
    let n = active.len();
    if n < 2 {
        return;
    }
    let mut rng = SplitMix64::new(seed);
    let mut e = energy(instances, state, pen);
    let mut best_e = e;
    let mut best: Vec<Option<usize>> = state.chosen.clone();
    let mut temp = 2.466_f64;
    let mut total = 0usize;
    let mut k: Vec<usize> = Vec::new();

    'stages: for _ in 0..50 {
        let (mut proposed, mut accepted) = (0usize, 0usize);
        while proposed < 20 * n && accepted < 5 * n {
            if total >= 2_000_000 {
                break 'stages;
            }
            proposed += 1;
            total += 1;
            let i = active[rng.below(n)];
            let c = rng.below(instances[i].candidates.len());
            if state.chosen[i] == Some(c) {
                continue;
            }
            conflicts(instances, state, dup, i, c, &mut k);
            let current = match state.chosen[i] {
                Some(ci) => instances[i].candidates[ci].cost,
                None => pen[i],
            };
            let mut delta = instances[i].candidates[c].cost - current;
            for &kk in &k {
                let kc = state.chosen[kk].expect("conflicts are placed");
                delta += pen[kk] - instances[kk].candidates[kc].cost;
            }
            if !(delta < 0.0 || rng.next_f64() < (-delta / temp).exp()) {
                continue;
            }
            state.remove(instances, i);
            for &kk in &k {
                state.remove(instances, kk);
            }
            state.insert(instances, i, c);
            accepted += 1;
            e += delta;
            if e < best_e {
                best_e = e;
                best.clone_from(&state.chosen);
            }
        }
        temp *= 0.9;
        if accepted == 0 {
            break;
        }
    }
    restore(instances, state, &best);

    // Fill: place what still fits.
    for &i in order {
        if instances[i].candidates.is_empty() || state.chosen[i].is_some() {
            continue;
        }
        for c in 0..instances[i].candidates.len() {
            conflicts(instances, state, dup, i, c, &mut k);
            if k.is_empty() {
                state.insert(instances, i, c);
                break;
            }
        }
    }

    // Descend: one pass to a cheaper free candidate.
    for &i in order {
        let Some(cur) = state.chosen[i] else { continue };
        let cur_cost = instances[i].candidates[cur].cost;
        for c in 0..cur {
            if instances[i].candidates[c].cost >= cur_cost {
                continue;
            }
            conflicts(instances, state, dup, i, c, &mut k);
            if k.is_empty() {
                state.remove(instances, i);
                state.insert(instances, i, c);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_published_value() {
        assert_eq!(SplitMix64::new(0).next_u64(), 0xE220A8397B1DCDAF);
        let mut r = SplitMix64::new(7);
        for _ in 0..100 {
            let f = r.next_f64();
            assert!((0.0..1.0).contains(&f));
            assert!(r.below(5) < 5);
        }
    }
}
